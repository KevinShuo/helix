use crate::ui::ai_preview_text::{display, scroll_key_by, PreviewText};
use helix_view::{ai_chat::Session, graphics::Rect, input::KeyEvent, Theme};
use tui::buffer::Buffer;

pub(super) struct Trace {
    text: PreviewText,
    revision: Option<u64>,
    scroll: usize,
    follow: bool,
    page: usize,
}

impl Trace {
    pub(super) fn new(running: bool) -> Self {
        Self {
            text: PreviewText::new(String::new()),
            revision: None,
            scroll: 0,
            follow: running,
            page: 1,
        }
    }

    pub(super) fn scroll(&mut self, key: KeyEvent) {
        if scroll_key_by(key, &mut self.scroll, self.page) {
            self.follow = self.scroll == usize::MAX;
        }
    }

    pub(super) fn render(
        &mut self,
        area: Rect,
        surface: &mut Buffer,
        session: &Session,
        theme: &Theme,
    ) {
        if area.area() == 0 {
            return;
        }
        let info = theme.get("ui.text.info");
        let heading = session
            .explore_root
            .as_deref()
            .or(session.project_root.as_deref())
            .map(|p| {
                format!(
                    "查阅记录 · {}",
                    display(&helix_stdx::path::normalize(p).to_string_lossy())
                )
            })
            .unwrap_or_else(|| "查阅记录".to_owned());
        surface.set_stringn(area.x, area.y, &heading, area.width as usize, info);
        let mut body = area.clip_top(1).clip_bottom(1);
        if session.pending.is_some()
            && !session.agent_progress.is_empty()
            && session.agent_trace.last() != Some(&session.agent_progress)
            && body.height >= 2
        {
            surface.set_stringn(
                body.x,
                body.y,
                &display(&session.agent_progress),
                body.width as usize,
                info,
            );
            body = body.clip_top(1);
        }
        if self.revision != Some(session.revision) {
            let entries: Vec<_> = session
                .agent_trace
                .iter()
                .map(|line| display(line))
                .collect();
            self.text = PreviewText::new(if entries.is_empty() {
                if session.pending.is_some() {
                    "等待第一条查阅记录…"
                } else {
                    "本轮尚无查阅记录"
                }
                .into()
            } else {
                entries.join("\n")
            });
            self.revision = Some(session.revision);
        }
        self.page = usize::from(body.height.max(1));
        if self.follow {
            self.scroll = usize::MAX;
        }
        self.text
            .render(body, surface, theme.get("ui.text"), &mut self.scroll);
        if area.height > 1 {
            let help = match (session.pending.is_some(), area.width < 48) {
                (true, true) => "Ctrl-C 停止 · Esc 返回",
                (true, false) => "Ctrl-C 停止 · PgUp/PgDn 翻页 · Alt+T / Esc 返回",
                (false, true) => "↑↓ 翻阅 · Esc 返回",
                (false, false) => "↑↓ / PgUp/PgDn 翻阅 · Alt+T / Esc 返回",
            };
            surface.set_stringn(area.x, area.bottom() - 1, help, area.width as usize, info);
        }
    }
}
