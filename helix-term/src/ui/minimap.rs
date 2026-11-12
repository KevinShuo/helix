//! The minimap painter reads prepared grids; source traversal lives in the handler.
use std::ops::Range;

use helix_view::{
    graphics::{Color, Rect},
    minimap::{LineMap, MinimapRender, ProjectionKey},
    Document, Editor, View,
};
use tui::buffer::Buffer as Surface;

fn symbol(dots: u8, render: MinimapRender) -> char {
    if dots == 0 {
        return ' ';
    }
    match render {
        MinimapRender::Braille => {
            let mut code = tui::symbols::braille::BLANK;
            for y in 0..4 {
                for x in 0..2 {
                    if dots & (1 << (y * 2 + x)) != 0 {
                        code |= tui::symbols::braille::DOTS[y][x];
                    }
                }
            }
            char::from_u32(code as u32).unwrap()
        }
        MinimapRender::Blocks => [
            ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
        ][dots as usize & 15],
    }
}

pub(super) fn viewport_rows(doc: &Document, view: &View, area: Rect) -> Range<u16> {
    let text = doc.text();
    let visible = view.visible_text_ranges(doc);
    let anchor = doc.view_offset(view.id).anchor.min(text.len_chars());
    let start = visible.first().map_or(anchor, |r| r.start);
    let end = visible.last().map_or(anchor, |r| r.end.saturating_sub(1));
    let map = LineMap::new(
        text.len_lines(),
        area.height,
        doc.config.load().minimap.render,
    );
    map.row(text.char_to_line(start))
        ..map
            .row(text.char_to_line(end))
            .saturating_add(1)
            .min(area.height)
}

fn mark_range(out: &mut [u8], map: LineMap, start: usize, end: usize, priority: u8) {
    for row in map.row(start)..=map.row(end.max(start)) {
        out[row as usize] = out[row as usize].max(priority);
    }
}

pub(super) fn render(editor: &Editor, doc: &Document, view: &View, surface: &mut Surface) {
    let layout = view.minimap_layout(doc);
    let (Some(area), Some(content)) = (layout.map, layout.content) else {
        return;
    };
    let config = doc.config.load().minimap;
    let theme = &editor.theme;
    let base = theme
        .get("ui.background")
        .patch(theme.get("ui.text"))
        .patch(theme.try_get_exact("ui.minimap").unwrap_or_default());
    surface.set_style(area, base);
    let current = doc.minimap.summary.as_ref().is_some_and(|s| {
        s.key.version == doc.version()
            && s.key.epoch == doc.minimap.epoch
            && s.key.tab_width == doc.tab_width()
    });
    let key = ProjectionKey {
        width: content.width,
        height: content.height,
        render: config.render,
    };
    if let Some(projection) = current
        .then(|| doc.minimap.projections.iter().find(|p| p.key == key))
        .flatten()
    {
        for y in 0..content.height {
            for x in 0..content.width {
                let cell = &projection.cells[y as usize * content.width as usize + x as usize];
                let mut style = base;
                if let Some(color) = cell.highlight.and_then(|h| theme.highlight(h).fg) {
                    style = style.fg(color);
                }
                surface[(content.x + x, content.y + y)]
                    .set_char(symbol(cell.dots, config.render))
                    .set_style(style);
            }
        }
    }
    let viewport = viewport_rows(doc, view, area);
    let viewport_style = theme
        .try_get_exact("ui.minimap.viewport")
        .unwrap_or_else(|| theme.get("ui.selection"));
    let bg = viewport_style.bg.unwrap_or(Color::Rgb(48, 48, 48));
    for y in viewport {
        for x in content.x..content.right() {
            surface[(x, content.y + y)].set_bg(bg);
        }
    }
    let map = LineMap::new(doc.text().len_lines(), area.height, config.render);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let cursor_row = map.row(doc.text().char_to_line(cursor));
    let cursor_style = theme
        .try_get_exact("ui.minimap.cursor")
        .unwrap_or_else(|| theme.get("ui.cursor.primary"));
    for y in 0..area.height {
        surface[(area.x, area.y + y)]
            .set_char(if y == cursor_row { '◆' } else { '│' })
            .set_style(if y == cursor_row {
                cursor_style
            } else {
                theme.get("ui.window")
            });
    }
    let mut diagnostics = vec![0; area.height as usize];
    if config.diagnostics {
        for d in doc.diagnostics() {
            let start = doc
                .text()
                .char_to_line(d.range.start.min(doc.text().len_chars()));
            let end = doc
                .text()
                .char_to_line(d.range.end.saturating_sub(1).min(doc.text().len_chars()));
            mark_range(&mut diagnostics, map, start, end, d.severity() as u8 + 1);
        }
    }
    let mut diff = vec![0; area.height as usize];
    if config.git_diff {
        if let Some(handle) = doc.diff_handle() {
            let hunks = handle.load();
            // The diff worker publishes asynchronously. Match the exact Rope snapshot cheaply.
            if hunks.doc().is_instance(doc.text()) {
                for i in 0..hunks.len() {
                    let hunk = hunks.nth_hunk(i);
                    let rank = if hunk.is_pure_removal() {
                        3
                    } else if hunk.is_pure_insertion() {
                        1
                    } else {
                        2
                    };
                    mark_range(
                        &mut diff,
                        map,
                        hunk.after.start as usize,
                        hunk.after.end.saturating_sub(1).max(hunk.after.start) as usize,
                        rank,
                    );
                }
            }
        }
    }
    for y in 0..area.height {
        let d = diagnostics[y as usize] as usize;
        if d > 0 {
            let scope = [
                "",
                "diagnostic.hint",
                "diagnostic.info",
                "diagnostic.warning",
                "diagnostic.error",
            ][d];
            surface[(area.right() - 1, area.y + y)]
                .set_char('┃')
                .set_style(theme.get(scope));
        }
        let d = diff[y as usize] as usize;
        if d > 0 {
            let scope = [
                "",
                "diff.plus.gutter",
                "diff.delta.gutter",
                "diff.minus.gutter",
            ][d];
            surface[(area.right() - 2, area.y + y)]
                .set_char(if d == 3 { '▔' } else { '▍' })
                .set_style(theme.get(scope));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::minimap::MinimapRender;

    #[test]
    fn minimap_dot_encodings_cover_both_modes() {
        assert_eq!(symbol(0, MinimapRender::Braille), ' ');
        assert_eq!(symbol(0xff, MinimapRender::Braille), '⣿');
        assert_eq!(symbol(1, MinimapRender::Braille), '⠁');
        assert_eq!(symbol(2, MinimapRender::Braille), '⠈');
        assert_eq!(symbol(0, MinimapRender::Blocks), ' ');
        assert_eq!(symbol(0x0f, MinimapRender::Blocks), '█');
        assert_eq!(symbol(0x03, MinimapRender::Blocks), '▀');
        assert_eq!(symbol(0x0c, MinimapRender::Blocks), '▄');
    }

    #[test]
    fn minimap_marker_buckets_keep_highest_priority_and_eof() {
        let map = LineMap::new(10000, 10, MinimapRender::Braille);
        let mut marks = vec![0; 10];
        mark_range(&mut marks, map, 4000, 8000, 4);
        mark_range(&mut marks, map, 0, 9999, 1);
        mark_range(&mut marks, map, 10000, 10000, 3);
        assert_eq!(marks[0], 1);
        assert_eq!(marks[5], 4);
        assert_eq!(marks[9], 3);
    }
}
