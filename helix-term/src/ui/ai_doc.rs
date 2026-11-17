//! A modal, nonediting preview of the exact documentation insertion.
use super::ai_preview_text::{scroll_key, PreviewText};
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    handlers::ai_doc,
    key,
};
use helix_core::Position;
use helix_view::{
    ai_doc::Target,
    graphics::{CursorKind, Rect},
    Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(crate) struct Preview {
    id: u64,
    title: String,
    text: PreviewText,
    scroll: usize,
}

impl Preview {
    pub(crate) fn new(id: u64, target: &Target, text: &str) -> Self {
        Self {
            id,
            title: format!(
                " AI 注释 · {} · 第 {} 行 | Enter 插入 · Esc 取消 · r 重试 ",
                target.name, target.line
            ),
            text: PreviewText::new(text.to_owned()),
            scroll: 0,
        }
    }
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
        let width = area.width.saturating_sub(4).min(110).max(1).min(area.width);
        let height = area
            .height
            .saturating_sub(4)
            .min(30)
            .max(1)
            .min(area.height);
        let area = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        surface.clear_with(area, cx.editor.theme.get("ui.popup"));
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.title.as_str());
        let body = block.inner(area);
        block.render(area, surface);
        self.text.render(
            body,
            surface,
            cx.editor.theme.get("ui.popup"),
            &mut self.scroll,
        );
    }
    fn cursor(&self, _area: Rect, _editor: &Editor) -> (Option<Position>, CursorKind) {
        (Some(Position::new(0, 0)), CursorKind::Hidden)
    }
    fn id(&self) -> Option<&'static str> {
        Some("ai-doc")
    }
}
