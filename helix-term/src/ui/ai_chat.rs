mod choices;
mod trace;
mod transcript;

use self::choices::{Choices, Kind};
use self::trace::Trace;
use self::transcript::Transcript;
use super::ai_preview_text::display;
use crate::{
    alt,
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    handlers::ai_chat,
    key,
    ui::Prompt,
};
use helix_core::{unicode::width::UnicodeWidthStr, Position};
use helix_view::{
    ai_chat::{Scope, Session},
    graphics::{CursorKind, Rect},
    Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(crate) struct Panel {
    input: Prompt,
    transcript: Transcript,
    revision: Option<u64>,
    follow: bool,
    scroll: usize,
    page_rows: usize,
    choices: Option<Choices>,
    trace: Option<Trace>,
}

impl Panel {
    pub(crate) fn new(editor: &Editor) -> Self {
        let draft = editor.ai_chat.as_ref().map_or("", |s| s.draft.as_str());
        let input = Prompt::new("> ".into(), None, |_, _| Vec::new(), |_, _, _| {})
            .with_line(display(draft), editor);
        Self {
            input,
            transcript: Transcript::default(),
            revision: None,
            follow: true,
            scroll: 0,
            page_rows: 1,
            choices: None,
            trace: None,
        }
    }
}

fn can_explore(session: &Session) -> bool {
    session.scope != Scope::Project
        && session.explore_root.is_none()
        && (!session.optimize || !session.turns.is_empty())
}

fn link_rows(session: &Session, width: u16) -> Vec<String> {
    let idle = session.pending.is_none();
    let mut rows = Vec::<String>::new();
    for (visible, text) in [
        (idle && can_explore(session), "Alt+E 查工程"),
        (
            !session.agent_trace.is_empty() || !session.agent_progress.is_empty(),
            "Alt+T 记录",
        ),
        (idle && !session.locations.is_empty(), "Alt+G 源码"),
        (idle && !session.suggestions.is_empty(), "Alt+P 建议"),
    ] {
        if !visible {
            continue;
        }
        if let Some(row) = rows
            .last_mut()
            .filter(|row| row.width() + 3 + text.width() <= width as usize)
        {
            row.push_str(" · ");
            row.push_str(text);
        } else {
            rows.push(text.to_owned());
        }
    }
    rows
}

fn panel_area(area: Rect) -> Rect {
    let width = area.width.min(84);
    let height = (area.height.saturating_mul(2) / 3)
        .clamp(6, 30)
        .min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.bottom() - height,
        width,
        height,
    )
}

impl Component for Panel {
    fn id(&self) -> Option<&'static str> {
        Some("ai-chat")
    }

    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Some(session) = cx.editor.ai_chat.as_ref() else {
            return EventResult::Ignored(None);
        };
        if let Some(trace) = &mut self.trace {
            if let Event::Key(key) = event {
                match *key {
                    key!(Esc) | alt!('t') => self.trace = None,
                    ctrl!('c') if session.pending.is_some() => {
                        ai_chat::cancel(cx.editor);
                        self.trace = None;
                    }
                    key => trace.scroll(key),
                }
            }
            return EventResult::Consumed(None);
        }
        if let Some(choices) = &mut self.choices {
            let Event::Key(key) = event else {
                return EventResult::Consumed(None);
            };
            let last = choices.kind.len(session).saturating_sub(1);
            choices.error.clear();
            match *key {
                key!(Esc) => self.choices = None,
                key!(Up) | key!('k') => choices.index = choices.index.saturating_sub(1),
                key!(Down) | key!('j') => choices.index = (choices.index + 1).min(last),
                key!(Home) => choices.index = 0,
                key!(End) => choices.index = last,
                key!(Enter) => match choices.activate(cx) {
                    Ok(()) => {
                        return EventResult::Consumed(Some(Box::new(|compositor, _| {
                            compositor.remove("ai-chat");
                        })))
                    }
                    Err(error) => choices.error = display(&error.to_string()),
                },
                _ => (),
            }
            return EventResult::Consumed(None);
        }
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
                alt!('t')
                    if !session.agent_trace.is_empty() || !session.agent_progress.is_empty() =>
                {
                    self.trace = Some(Trace::new(session.pending.is_some()));
                    return EventResult::Consumed(None);
                }
                alt!('g') | alt!('p') => {
                    let kind = if *key == alt!('g') {
                        Kind::Location
                    } else {
                        Kind::Suggestion
                    };
                    if session.pending.is_none() && kind.len(session) > 0 {
                        self.choices = Some(Choices::new(kind));
                    }
                    return EventResult::Consumed(None);
                }
                key!(PageUp) | key!(PageDown) => {
                    let end = self.transcript.len().saturating_sub(self.page_rows);
                    self.scroll = if *key == key!(PageUp) {
                        self.scroll.saturating_sub(self.page_rows)
                    } else {
                        self.scroll.saturating_add(self.page_rows).min(end)
                    };
                    self.follow = *key == key!(PageDown) && self.scroll == end;
                    return EventResult::Consumed(None);
                }
                alt!('d') => {
                    if let Some(idx) = self.transcript.detail_turn(self.scroll, self.page_rows) {
                        self.transcript
                            .toggle(idx, cx.editor.theme.get("ui.text.info"));
                        self.follow = false;
                        return EventResult::Consumed(None);
                    }
                }
                key!(Enter) | alt!('e') if session.pending.is_none() => {
                    let question = self.input.line().clone();
                    let result = if *key == alt!('e') {
                        if !can_explore(session) {
                            return EventResult::Consumed(None);
                        }
                        ai_chat::explore(cx.editor, &question)
                    } else {
                        ai_chat::submit(cx.editor, &question)
                    };
                    match result {
                        Ok(()) => {
                            self.follow = true;
                            self.input.clear(cx.editor);
                        }
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
            let safe = display(self.input.line());
            if &safe != self.input.line() {
                self.input.set_line(safe, cx.editor);
            }
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
        let area = panel_area(area);
        let panel = area.clip_bottom(1);
        let style = cx.editor.theme.get("ui.popup");
        surface.clear_with(panel, style);
        let activity = if session.optimize {
            "优化建议"
        } else {
            match session.scope {
                Scope::Function => "函数 / 声明问答",
                Scope::File => "文件问答",
                Scope::Project => "工程问答",
                _ => "选区问答",
            }
        };
        let title = format!(" {activity} · {} ", display(&session.label));
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title.as_str())
            .style(style);
        let body = block.inner(panel);
        block.render(panel, surface);
        if let Some(trace) = &mut self.trace {
            trace.render(body, surface, session, &cx.editor.theme);
            surface.clear_with(area.clip_top(area.height - 1), style);
            return;
        }
        if let Some(choices) = &self.choices {
            choices.render(body, surface, cx.editor, session);
            surface.clear_with(area.clip_top(area.height - 1), style);
            return;
        }
        if body.height > 0 {
            let compact = body.width < 58 && body.height >= 5;
            let base_rows = if compact { 2 } else { 1 };
            let progress =
                session.pending.is_some() && !session.agent_progress.is_empty() && body.height >= 3;
            let mut links = link_rows(session, body.width);
            links.truncate(
                body.height
                    .saturating_sub(base_rows + u16::from(progress) + 1) as usize,
            );
            let footer_rows = base_rows + links.len() as u16 + u16::from(progress);
            let reading = body.clip_left(1).clip_right(1).clip_bottom(footer_rows);
            self.page_rows = usize::from(reading.height.max(1));
            let changed = self.revision != Some(session.revision);
            if changed
                || self.transcript.width() != reading.width
                || self.transcript.theme() != cx.editor.theme.name()
            {
                let removed = self.transcript.update(session, cx.editor, reading.width);
                self.scroll = if self.follow {
                    self.transcript.latest()
                } else {
                    self.scroll.saturating_sub(removed)
                };
                if changed {
                    let draft = display(&session.draft);
                    if &draft != self.input.line() {
                        self.input.set_line(draft, cx.editor);
                    }
                }
                self.revision = Some(session.revision);
            }
            self.scroll = self
                .scroll
                .min(self.transcript.len().saturating_sub(self.page_rows));
            self.transcript.render(reading, surface, self.scroll);
            let action = if session.pending.is_some() {
                "Ctrl-C 停止"
            } else if session.explore_root.is_some() {
                "Enter 追问 · 查工程"
            } else if session.turns.is_empty() {
                "Enter 提问"
            } else {
                "Enter 追问"
            };
            let details = self
                .transcript
                .detail_turn(self.scroll, self.page_rows)
                .is_some();
            let action = if details {
                format!("{action} · Alt+D 细节")
            } else {
                action.to_owned()
            };
            let paging = if body.width < 26 {
                "PgUp/Dn 翻页 · Esc"
            } else {
                "PgUp/PgDn 翻页 · Esc 收起"
            };
            let style = cx.editor.theme.get("ui.text.info");
            let mut y = body.bottom() - footer_rows;
            if progress {
                surface.set_stringn(
                    body.x,
                    y,
                    &display(&session.agent_progress),
                    body.width as usize,
                    style,
                );
                y += 1;
            }
            for hint in links {
                surface.set_stringn(body.x, y, &hint, body.width as usize, style);
                y += 1;
            }
            let hint = if compact {
                surface.set_stringn(
                    body.x,
                    body.bottom() - 2,
                    &action,
                    body.width as usize,
                    style,
                );
                paging.to_owned()
            } else {
                format!("{action} · {paging}")
            };
            surface.set_stringn(body.x, body.bottom() - 1, &hint, body.width as usize, style);
        }
        self.input
            .render_prompt(area.clip_top(area.height - 1), surface, cx);
    }

    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        if area.height < 3
            || area.width < 6
            || self.choices.is_some()
            || self.trace.is_some()
            || editor.ai_chat.as_ref().is_some_and(|s| s.pending.is_some())
        {
            (None, CursorKind::Hidden)
        } else {
            let area = panel_area(area);
            self.input.cursor(area.clip_top(area.height - 1), editor)
        }
    }
}
