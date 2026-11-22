use super::ai_preview_text::{display, PreviewText};
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    handlers::ai_chat,
    key,
    ui::Prompt,
};
use helix_core::Position;
use helix_view::{
    graphics::{CursorKind, Rect},
    Editor,
};
use std::fmt::Write;
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(crate) struct Panel {
    input: Prompt,
    transcript: PreviewText,
    revision: Option<u64>,
    awaiting_reply: bool,
    scroll: usize,
    page_rows: usize,
}

impl Panel {
    pub(crate) fn new(editor: &Editor) -> Self {
        let draft = editor.ai_chat.as_ref().map_or("", |s| s.draft.as_str());
        let input = Prompt::new("> ".into(), None, |_, _| Vec::new(), |_, _, _| {})
            .with_line(draft.to_owned(), editor);
        Self {
            input,
            transcript: PreviewText::new(String::new()),
            revision: None,
            awaiting_reply: false,
            scroll: usize::MAX,
            page_rows: 1,
        }
    }
}

impl Component for Panel {
    fn id(&self) -> Option<&'static str> {
        Some("ai-chat")
    }

    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Some(session) = cx.editor.ai_chat.as_ref() else {
            return EventResult::Ignored(None);
        };
        if let Event::Key(key) = event {
            match *key {
                key!(Esc) => {
                    return EventResult::Consumed(Some(Box::new(|compositor, _| {
                        compositor.remove("ai-chat");
                    })));
                }
                ctrl!('c') => {
                    ai_chat::cancel(cx.editor);
                    return EventResult::Consumed(None);
                }
                key!(PageUp) | key!(PageDown) => {
                    self.scroll = if *key == key!(PageUp) {
                        self.scroll.saturating_sub(self.page_rows)
                    } else {
                        self.scroll.saturating_add(self.page_rows)
                    };
                    return EventResult::Consumed(None);
                }
                key!(Enter) if session.pending.is_none() => {
                    let question = self.input.line().clone();
                    match ai_chat::submit(cx.editor, &question) {
                        Ok(()) => self.input.clear(cx.editor),
                        Err(error) => {
                            let session = cx.editor.ai_chat.as_mut().unwrap();
                            session.notice = error.to_string();
                            session.revision = session.revision.wrapping_add(1);
                        }
                    }
                    return EventResult::Consumed(None);
                }
                _ => {}
            }
        }
        if session.pending.is_none() {
            let result = self.input.handle_event(event, cx);
            cx.editor
                .ai_chat
                .as_mut()
                .unwrap()
                .draft
                .clone_from(self.input.line());
            result
        } else {
            EventResult::Consumed(None)
        }
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        let Some(session) = cx.editor.ai_chat.as_ref() else {
            return;
        };
        if area.width < 6 || area.height < 3 {
            return;
        }
        if self.revision != Some(session.revision) {
            let mut text = String::new();
            for (question, answer) in &session.turns {
                let _ = writeln!(text, "你　{question}\n\nAI　{answer}\n");
            }
            if let Some(question) = &session.pending {
                let _ = write!(text, "你　{question}\n\n正在回答…");
            } else if !session.notice.is_empty() {
                text.push_str(&session.notice);
            } else if session.turns.is_empty() {
                text.push_str("想了解这段代码的哪一部分？");
            }
            self.input.set_line(session.draft.clone(), cx.editor);
            self.transcript = PreviewText::new(text);
            self.revision = Some(session.revision);
            // Keep the beginning of the new answer in view when it replaces
            // the waiting indicator, even when it spans several pages.
            if !self.awaiting_reply || session.pending.is_some() {
                self.scroll = usize::MAX;
            }
            self.awaiting_reply = session.pending.is_some();
        }
        let height = (area.height / 2).clamp(6, 22).min(area.height);
        let panel = Rect::new(area.x, area.bottom() - height, area.width, height - 1);
        let style = cx.editor.theme.get("ui.popup");
        surface.clear_with(panel, style);
        let title = format!(" 选区问答 · {} ", display(&session.label));
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title.as_str())
            .style(style);
        let body = block.inner(panel);
        block.render(panel, surface);
        if body.height > 0 {
            self.page_rows = usize::from(body.height.saturating_sub(2).max(1));
            self.transcript
                .render(body.clip_bottom(1), surface, style, &mut self.scroll);
            let hint = if session.pending.is_some() {
                "Ctrl-C 停止 · Esc 收起"
            } else {
                "Enter 提问 · PgUp/PgDn 翻页 · Esc 收起"
            };
            surface.set_stringn(
                body.x,
                body.bottom() - 1,
                hint,
                body.width as usize,
                cx.editor.theme.get("ui.text.info"),
            );
        }
        self.input.render_prompt(area, surface, cx);
    }

    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        if area.height < 3
            || area.width < 6
            || editor.ai_chat.as_ref().is_some_and(|s| s.pending.is_some())
        {
            (None, CursorKind::Hidden)
        } else {
            self.input.cursor(area, editor)
        }
    }
}
