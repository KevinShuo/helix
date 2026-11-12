use helix_core::{char_idx_at_visual_offset, movement::Direction, Range};
use helix_view::{
    document::Mode,
    graphics::Rect,
    input::{MouseButton, MouseEvent, MouseEventKind},
    minimap::{LineMap, MinimapRender},
    DocumentId, Editor, ViewId,
};

use crate::commands;

struct Drag {
    view: ViewId,
    doc: DocumentId,
    area: Rect,
    version: i32,
    epoch: u64,
    render: MinimapRender,
    grab: u16,
    last_row: u16,
}

#[derive(Default)]
pub(crate) struct MinimapInteraction {
    drag: Option<Drag>,
    // Keep this after cancellation until release, so a release in the body cannot yank.
    release: bool,
}

fn contains(area: Rect, event: &MouseEvent) -> bool {
    (area.x..area.right()).contains(&event.column) && (area.y..area.bottom()).contains(&event.row)
}

impl MinimapInteraction {
    pub(crate) fn cancel(&mut self) {
        self.drag = None;
    }

    pub(crate) fn validate(&mut self, editor: &Editor) {
        let valid = self.drag.as_ref().is_none_or(|d| {
            editor.tree.try_get(d.view).is_some_and(|v| {
                v.doc == d.doc
                    && editor.document(d.doc).is_some_and(|doc| {
                        doc.version() == d.version
                            && doc.minimap.epoch == d.epoch
                            && doc.config.load().mouse
                            && doc.config.load().minimap.render == d.render
                            && v.minimap_layout(doc).map == Some(d.area)
                    })
            })
        });
        if !valid {
            self.cancel();
        }
    }

    pub(crate) fn handle(&mut self, event: &MouseEvent, cx: &mut commands::Context) -> bool {
        self.validate(cx.editor);
        if event.kind == MouseEventKind::Up(MouseButton::Left) && self.release {
            self.release = false;
            self.drag = None;
            cx.editor.mouse_down_range = None;
            return true;
        }
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            self.drag = None;
            self.release = false;
        }
        if event.kind == MouseEventKind::Drag(MouseButton::Left) && self.release {
            if let Some(drag) = &mut self.drag {
                let row = event
                    .row
                    .saturating_sub(drag.area.y)
                    .min(drag.area.height - 1);
                if row != drag.last_row {
                    drag.last_row = row;
                    let doc = cx.editor.document(drag.doc).unwrap();
                    let map = LineMap::new(doc.text().len_lines(), drag.area.height, drag.render);
                    let line = map.row_lines(row.saturating_sub(drag.grab)).start;
                    navigate(cx.editor, drag.view, line, false);
                }
            }
            return true;
        }
        if !cx.editor.config().mouse {
            return false;
        }
        let hit = cx.editor.tree.views().find_map(|(view, _)| {
            let doc = &cx.editor.documents[&view.doc];
            view.minimap_layout(doc)
                .map
                .filter(|r| contains(*r, event))
                .map(|r| (view.id, r))
        });
        let Some((view_id, area)) = hit else {
            return false;
        };
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                cx.editor.focus(view_id);
                cx.editor.mouse_down_range = None;
                let view = cx.editor.tree.get(view_id);
                let doc = &cx.editor.documents[&view.doc];
                let render = doc.config.load().minimap.render;
                let row = event.row - area.y;
                let viewport = super::viewport_rows(doc, view, area);
                let map = LineMap::new(doc.text().len_lines(), area.height, render);
                let line = map.target_line(row);
                let target = doc.text().line_to_char(line);
                let hidden = doc.folds(view_id).iter().any(|fold| fold.contains(&target));
                if hidden || !viewport.contains(&row) {
                    navigate(cx.editor, view_id, line, true);
                }
                let view = cx.editor.tree.get(view_id);
                let doc = &cx.editor.documents[&view.doc];
                let viewport = super::viewport_rows(doc, view, area);
                self.drag = Some(Drag {
                    view: view_id,
                    doc: doc.id(),
                    area,
                    version: doc.version(),
                    epoch: doc.minimap.epoch,
                    render,
                    grab: row.saturating_sub(viewport.start),
                    last_row: row,
                });
                self.release = true;
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let old_focus = cx.editor.tree.focus;
                cx.editor.tree.focus = view_id;
                let config = cx.editor.config();
                let direction = if event.kind == MouseEventKind::ScrollUp {
                    Direction::Backward
                } else {
                    Direction::Forward
                };
                commands::scroll(cx, config.scroll_lines.unsigned_abs(), direction, false);
                cx.editor.tree.focus = old_focus;
                cx.editor.ensure_cursor_in_view(old_focus);
            }
            // Consume all map events, including right/middle release and unowned drags.
            _ => {}
        }
        true
    }
}

/// Absolute source navigation: bounded work near the destination rather than scrolling from start.
fn navigate(editor: &mut Editor, view_id: ViewId, line: usize, center: bool) {
    let mode = editor.mode();
    let scrolloff = editor.config().scrolloff;
    let view = editor.tree.get_mut(view_id);
    let doc = editor.documents.get_mut(&view.doc).unwrap();
    let target = doc
        .text()
        .line_to_char(line.min(doc.text().len_lines() - 1));
    while doc
        .folds(view_id)
        .iter()
        .any(|range| range.contains(&target))
    {
        if !doc.unfold_at(view_id, target) {
            break;
        }
    }
    let height = view.inner_height();
    let format = doc.text_format(view.inner_width(doc), None);
    let annotations = view.text_annotations(doc, None);
    let text = doc.text().slice(..);
    let old = doc.view_offset(view_id);
    let (anchor, vertical_offset) = char_idx_at_visual_offset(
        text,
        target,
        if center {
            -(height.saturating_sub(1) as isize / 2)
        } else {
            0
        },
        0,
        &format,
        &annotations,
    );
    drop(annotations);
    doc.set_view_offset(
        view_id,
        helix_view::view::ViewPosition {
            anchor,
            vertical_offset,
            ..old
        },
    );

    let text = doc.text().slice(..);
    let annotations = view.text_annotations(doc, None);
    let off = scrolloff.min(height.saturating_sub(1) / 2);
    let first = char_idx_at_visual_offset(
        text,
        anchor,
        (vertical_offset + off) as isize,
        0,
        &format,
        &annotations,
    )
    .0;
    let last = char_idx_at_visual_offset(
        text,
        anchor,
        (vertical_offset + height.saturating_sub(off + 1)) as isize,
        0,
        &format,
        &annotations,
    )
    .0;
    let selection = doc.selection(view_id).clone();
    let primary = selection.primary();
    let cursor = primary.cursor(text);
    let head = cursor.clamp(first.min(last), first.max(last));
    drop(annotations);
    if head != cursor {
        let anchor = if mode == Mode::Select {
            primary.anchor
        } else {
            head
        };
        let index = selection.primary_index();
        let selection = selection.replace(index, Range::new(anchor, head));
        doc.set_selection(view_id, selection);
    }
    // SelectionDidChange makes an old AI suggestion invalid without changing document text.
    if doc
        .ai_completion
        .as_ref()
        .is_some_and(|s| !s.is_valid(doc, view_id))
    {
        doc.ai_completion = None;
        doc.ai_completion_controller.cancel();
    }
}
