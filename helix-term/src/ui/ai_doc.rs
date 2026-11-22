//! A modal, nonediting preview of the exact documentation insertion.
use super::ai_preview_text::{display, scroll_key, PreviewText};
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    handlers::ai_doc,
    key,
};
use helix_core::{graphemes::tab_width_at, unicode::width::UnicodeWidthStr, Position};
use helix_view::{
    ai_doc::Target,
    graphics::{CursorKind, Rect},
    Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

const MAX_WIDTH: u16 = 88;

pub(crate) struct Preview {
    id: u64,
    title: String,
    text: PreviewText,
    scroll: usize,
}

impl Preview {
    pub(crate) fn new(id: u64, target: &Target, text: &str, tab_width: usize) -> Self {
        Self {
            id,
            title: format!(
                " 待插入注释 · {} · 第 {} 行 ",
                display(&target.name),
                target.line
            ),
            text: PreviewText::new(expand_tabs(text, tab_width)),
            scroll: 0,
        }
    }
}

// Only the preview expands tabs. Accepting still inserts the original text.
fn expand_tabs(text: &str, tab_width: usize) -> String {
    // A tab stop wider than the entire panel cannot be shown to scale.
    if tab_width > usize::from(MAX_WIDTH) {
        return text.replace('\t', "⇥");
    }
    let tab_width = tab_width.max(1) as u16;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let mut column = 0;
        for segment in line.split_inclusive('\t') {
            if let Some(prefix) = segment.strip_suffix('\t') {
                out.push_str(prefix);
                column += prefix.width();
                let spaces = tab_width_at(column, tab_width);
                out.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            } else {
                out.push_str(segment);
            }
        }
    }
    out
}

impl Component for Preview {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if !cx
            .editor
            .ai_doc
            .as_ref()
            .is_some_and(|op| op.id == self.id && op.is_valid(cx.editor))
        {
            return EventResult::Consumed(Some(Box::new(|compositor, cx| {
                ai_doc::cancel(cx.editor);
                compositor.remove("ai-doc");
            })));
        }
        let Event::Key(key) = event else {
            return EventResult::Consumed(None);
        };
        if scroll_key(*key, &mut self.scroll) {
            return EventResult::Consumed(None);
        }
        let action = match *key {
            key!(Enter) => 0,
            key!(Esc) | ctrl!('c') => 1,
            key!('r') => 2,
            _ => return EventResult::Consumed(None),
        };
        EventResult::Consumed(Some(Box::new(move |compositor, cx| {
            compositor.remove("ai-doc");
            let result = match action {
                0 => ai_doc::accept(cx.editor),
                2 => ai_doc::retry(cx.editor),
                _ => {
                    ai_doc::cancel(cx.editor);
                    Ok(())
                }
            };
            if let Err(error) = result {
                ai_doc::cancel(cx.editor);
                cx.editor.set_error(error.to_string());
            }
        })))
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        if area.area() == 0 {
            return;
        }
        let width = area.width.saturating_sub(2).clamp(1, MAX_WIDTH);
        let inner_width = width.saturating_sub(2);
        let inset = if inner_width >= 12 { 1 } else { 0 };
        let rows = self.text.row_count(inner_width.saturating_sub(2 * inset));
        let height = area
            .height
            .saturating_sub(4)
            .min((rows.min(27) + 3) as u16)
            .max(1)
            .min(area.height);
        let area = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        let style = cx.editor.theme.get("ui.popup");
        surface.clear_with(area, style);
        let block = Block::default()
            .style(style)
            .borders(Borders::ALL)
            .title(self.title.as_str());
        let body = block.inner(area);
        block.render(area, surface);
        let mut content = body;
        if body.height >= 2 {
            let hint = if body.width >= 52 {
                "Enter 插入   r 重试   PgUp/PgDn 翻页   Esc 取消"
            } else {
                "Enter 插入  Esc 取消  r 重试"
            };
            surface.set_stringn(
                body.x,
                body.bottom() - 1,
                hint,
                body.width as usize,
                style.patch(cx.editor.theme.get("ui.text.info")),
            );
            content = content.clip_bottom(1);
        }
        content = content.clip_left(inset).clip_right(inset);
        self.text.render(content, surface, style, &mut self.scroll);
    }
    fn cursor(&self, _area: Rect, _editor: &Editor) -> (Option<Position>, CursorKind) {
        (Some(Position::new(0, 0)), CursorKind::Hidden)
    }
    fn id(&self) -> Option<&'static str> {
        Some("ai-doc")
    }
}
