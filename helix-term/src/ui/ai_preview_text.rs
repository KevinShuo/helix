//! Complete preview text with usize row positions and viewport-only rendering.
use helix_core::unicode::{segmentation::UnicodeSegmentation, width::UnicodeWidthStr};
use helix_view::graphics::{Rect, Style};
use helix_view::input::KeyEvent;
use tui::buffer::{Buffer, SYMBOL_CAPACITY};

pub(super) struct PreviewText {
    source: String,
    width: u16,
    // Byte offsets are independent of terminal row counts. Provider artifacts
    // are capped at 16 MiB, so compact u32 offsets also bound the layout cache.
    rows: Vec<u32>,
}

pub(super) fn scroll_key(key: KeyEvent, scroll: &mut usize) -> bool {
    *scroll = match key {
        crate::key!(Home) | crate::ctrl!(Home) => 0,
        crate::key!(End) | crate::ctrl!(End) => usize::MAX,
        crate::key!('j') | crate::key!(Down) => scroll.saturating_add(1),
        crate::key!('k') | crate::key!(Up) => scroll.saturating_sub(1),
        crate::key!(PageDown) | crate::ctrl!('d') => scroll.saturating_add(10),
        crate::key!(PageUp) | crate::ctrl!('u') => scroll.saturating_sub(10),
        _ => return false,
    };
    true
}

fn unsafe_control(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

pub(super) fn display(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\t' {
                ' '
            } else if unsafe_control(c) {
                '�'
            } else {
                c
            }
        })
        .collect()
}

impl PreviewText {
    pub(super) fn new(source: String) -> Self {
        // Newlines are layout; all other controls are inert display characters.
        let source = source
            .char_indices()
            .filter_map(|(offset, c)| {
                if c == '\r' && source.as_bytes().get(offset + 1) == Some(&b'\n') {
                    return None;
                }
                Some(if c == '\n' {
                    c
                } else if c == '\t' {
                    ' '
                } else if unsafe_control(c) {
                    '�'
                } else {
                    c
                })
            })
            .collect();
        Self {
            source,
            width: 0,
            rows: Vec::new(),
        }
    }

    fn symbol<'a>(grapheme: &'a str, width: u16) -> &'a str {
        if grapheme.len() > SYMBOL_CAPACITY || grapheme.width() > width as usize {
            "�"
        } else {
            grapheme
        }
    }

    fn layout(&mut self, width: u16) {
        if self.width == width {
            return;
        }
        self.width = width;
        self.rows.clear();
        self.rows.push(0);
        let mut columns = 0;
        for (offset, grapheme) in self.source.grapheme_indices(true) {
            if grapheme == "\n" {
                self.rows.push((offset + 1) as u32);
                columns = 0;
                continue;
            }
            let symbol_width = Self::symbol(grapheme, width).width();
            if columns > 0 && columns + symbol_width > width as usize {
                self.rows.push(offset as u32);
                columns = 0;
            }
            columns += symbol_width;
        }
    }

    pub(super) fn render(
        &mut self,
        area: Rect,
        surface: &mut Buffer,
        style: Style,
        scroll: &mut usize,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.layout(area.width);
        *scroll = (*scroll).min(self.rows.len().saturating_sub(area.height as usize));
        for (y, row) in (*scroll..self.rows.len())
            .take(area.height as usize)
            .enumerate()
        {
            let start = self.rows[row] as usize;
            let end = self
                .rows
                .get(row + 1)
                .map_or(self.source.len(), |&end| end as usize);
            let line = self.source[start..end].trim_end_matches('\n');
            let mut x = 0;
            for grapheme in line.graphemes(true) {
                let symbol = Self::symbol(grapheme, area.width);
                let width = symbol.width();
                if width == 0 || x + width > area.width as usize {
                    continue;
                }
                surface.set_stringn(area.x + x as u16, area.y + y as u16, symbol, width, style);
                x += width;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_preview_tail_after_u16_rows_is_visible_and_resize_keeps_complete_text() {
        let source = format!("{}TAIL", "x".repeat(70_000));
        let mut text = PreviewText::new(source.clone());
        let area = Rect::new(0, 0, 1, 4);
        let mut buffer = Buffer::empty(area);
        let mut scroll = 0;
        assert!(scroll_key(crate::key!(End), &mut scroll));
        text.render(area, &mut buffer, Style::default(), &mut scroll);
        assert_eq!(scroll, 70_000);
        assert_eq!(buffer, Buffer::with_lines(vec!["T", "A", "I", "L"]));
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::empty(area);
        text.render(area, &mut buffer, Style::default(), &mut scroll);
        assert_eq!(buffer, Buffer::with_lines(vec!["TAIL"]));
        assert_eq!(text.source, source);
        assert!(scroll_key(crate::key!(Home), &mut scroll));
        text.render(area, &mut buffer, Style::default(), &mut scroll);
        assert_eq!(buffer, Buffer::with_lines(vec!["xxxx"]));
    }

    #[test]
    fn ai_preview_unicode_and_control_display_is_safe_at_narrow_width() {
        let source = "中e\u{301}\t\u{1b}\u{202e}👩‍💻".to_owned();
        let mut text = PreviewText::new(source.clone());
        let area = Rect::new(0, 0, 2, 6);
        let mut buffer = Buffer::empty(area);
        let mut scroll = 0;
        text.render(area, &mut buffer, Style::default(), &mut scroll);
        assert_eq!(&*buffer[(0, 0)].symbol, "中");
        assert_eq!(&*buffer[(0, 1)].symbol, "e\u{301}");
        assert_eq!(&*buffer[(1, 1)].symbol, " ");
        assert_eq!(&*buffer[(0, 2)].symbol, "�");
        assert_eq!(&*buffer[(1, 2)].symbol, "�");
        // The terminal width implementation counts this ZWJ sequence as four
        // columns. It must stay within the viewport even on narrower terminals.
        assert_eq!(&*buffer[(0, 3)].symbol, "�");
        let area = Rect::new(0, 0, 4, 6);
        let mut buffer = Buffer::empty(area);
        text.render(area, &mut buffer, Style::default(), &mut scroll);
        assert_eq!(&*buffer[(0, 2)].symbol, "👩‍💻");
        let area = Rect::new(0, 0, 1, 8);
        let mut buffer = Buffer::empty(area);
        text.render(area, &mut buffer, Style::default(), &mut scroll);
        assert_eq!(&*buffer[(0, 0)].symbol, "�");
        assert_eq!(&*buffer[(0, 5)].symbol, "�");
        assert_eq!(text.source, "中e\u{301} ��👩‍💻");
        text.render(
            Rect::new(0, 0, 0, 0),
            &mut buffer,
            Style::default(),
            &mut scroll,
        );
    }

    #[test]
    fn ai_preview_crlf_uses_line_boundaries_without_control_markers() {
        let mut text = PreviewText::new("first\r\nlast\r\n".into());
        let area = Rect::new(0, 0, 5, 3);
        let mut buffer = Buffer::empty(area);
        text.render(area, &mut buffer, Style::default(), &mut 0);
        assert_eq!(buffer, Buffer::with_lines(vec!["first", "last ", "     "]));
    }
}
