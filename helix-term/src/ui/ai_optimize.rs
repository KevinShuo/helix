use super::ai_preview_text::{
    display,
    review::{ReviewLines, ReviewText},
    scroll_key_by,
};
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    handlers::{ai_chat, ai_optimize::Patch},
    key,
};
use helix_view::graphics::Rect;
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(crate) struct Preview {
    title: String,
    patch: Option<Patch>,
    diff: ReviewText,
    scroll: usize,
    page: usize,
}

impl Preview {
    pub(crate) fn new(title: String, patch: Patch) -> Self {
        let mut lines = ReviewLines::default();
        lines.diff(&patch.before, &patch.after, true);
        Self {
            title: display(&title),
            patch: Some(patch),
            diff: lines.finish(),
            scroll: 0,
            page: 1,
        }
    }
}

impl Component for Preview {
    fn id(&self) -> Option<&'static str> {
        Some("ai-optimize-preview")
    }

    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Consumed(None);
        };
        if scroll_key_by(*key, &mut self.scroll, self.page) {
            return EventResult::Consumed(None);
        }
        match *key {
            key!(Enter) => {
                if let Some(patch) = self.patch.take() {
                    if let Err(error) = patch.apply(cx.editor) {
                        cx.editor.set_error(error.to_string());
                    }
                }
            }
            key!(Esc) | ctrl!('c') => (),
            _ => return EventResult::Consumed(None),
        }
        EventResult::Consumed(Some(Box::new(|compositor, cx| {
            compositor.remove("ai-optimize-preview");
            let _ = ai_chat::show(cx);
        })))
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        let width = area.width.min(88);
        let height = area.height.min(26);
        let area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
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
        if body.area() == 0 {
            return;
        }
        let reading = body.clip_bottom(1);
        self.page = usize::from(reading.height.max(1));
        self.diff
            .render(reading, surface, &cx.editor.theme, &mut self.scroll);
        surface.set_stringn(
            body.x,
            body.bottom() - 1,
            "Enter 应用 · Esc 返回 · PgUp/PgDn 翻页",
            body.width as usize,
            cx.editor.theme.get("ui.text.info"),
        );
    }
}
