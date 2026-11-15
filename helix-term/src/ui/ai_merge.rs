//! A read-only preview of the exact block replacement, separate from the draft.
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    handlers::ai_merge,
    key,
};
use anyhow::{ensure, Result};
use helix_core::Position;
use helix_view::{
    ai_merge::{Suggestion, Target},
    graphics::{CursorKind, Rect},
    Editor,
};
use tui::{
    buffer::Buffer,
    text::Text,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

pub(crate) struct Preview {
    id: u64,
    title: String,
    text: Text<'static>,
    can_accept: bool,
    scroll: usize,
}

fn display(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\t' {
                ' '
            } else if c.is_control()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                '�'
            } else {
                c
            }
        })
        .collect()
}

impl Preview {
    pub(crate) fn new(id: u64, target: &Target, suggestion: &Suggestion) -> Result<Self> {
        let status = if suggestion.code.is_some() {
            "候选待复核"
        } else {
            "需人工处理"
        };
        let mut lines = vec![status.to_owned(), String::new(), "AI 建议：".into()];
        lines.extend(suggestion.explanation.lines().map(display));
        if let Some(code) = &suggestion.code {
            lines.extend([
                String::new(),
                "--- 当前 Result 冲突块".into(),
                "+++ AI 候选".into(),
            ]);
            lines.extend(
                target
                    .result
                    .split_inclusive('\n')
                    .map(|line| format!("- {}", display(line.trim_end_matches('\n')))),
            );
            if !target.result.is_empty() && !target.result.ends_with('\n') {
                lines.push("  [当前块末尾无换行]".into());
            }
            if code.is_empty() {
                lines.push("+ [删除此冲突块]".into());
            } else {
                lines.extend(
                    code.split_inclusive('\n')
                        .map(|line| format!("+ {}", display(line.trim_end_matches('\n')))),
                );
            }
            if !code.is_empty() && !code.ends_with('\n') {
                lines.push("  [候选末尾无换行]".into());
            }
            lines.extend([
                String::new(),
                "接受到 Result 后仍需 Review / Apply。".into(),
            ]);
        }
        let text = lines.join("\n");
        // At one display column, the UTF-8 byte count also bounds the wrapped
        // row count. Reserve headroom below Paragraph's u16 size/scroll limit.
        ensure!(
            text.len() <= 60_000,
            "AI 候选过大，无法完整预览；请缩小冲突块或减少 max-tokens，草稿已保留"
        );
        Ok(Self {
            id,
            title: format!(
                " AI Merge · {} · block {} ",
                display(&target.path),
                target.block + 1
            ),
            text: Text::from(text),
            can_accept: suggestion.code.is_some(),
            scroll: 0,
        })
    }
}

impl Component for Preview {
    fn id(&self) -> Option<&'static str> {
        Some("ai-merge")
    }

    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if !cx
            .editor
            .ai_merge
            .as_ref()
            .is_some_and(|op| op.id == self.id && op.is_valid(cx.editor))
        {
            return EventResult::Consumed(Some(Box::new(|c, cx| {
                ai_merge::cancel(cx.editor);
                c.remove("ai-merge");
            })));
        }
        let Event::Key(key) = event else {
            return EventResult::Consumed(None);
        };
        let action = match *key {
            key!(Enter) if self.can_accept => 0,
            key!(Esc) | ctrl!('c') => 1,
            key!('r') => 2,
            key!('j') | key!(Down) => {
                self.scroll = self.scroll.saturating_add(1);
                return EventResult::Consumed(None);
            }
            key!('k') | key!(Up) => {
                self.scroll = self.scroll.saturating_sub(1);
                return EventResult::Consumed(None);
            }
            key!(PageDown) | ctrl!('d') => {
                self.scroll = self.scroll.saturating_add(10);
                return EventResult::Consumed(None);
            }
            key!(PageUp) | ctrl!('u') => {
                self.scroll = self.scroll.saturating_sub(10);
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Consumed(None),
        };
        EventResult::Consumed(Some(Box::new(move |c, cx| {
            c.remove("ai-merge");
            let result = match action {
                0 => ai_merge::accept(cx.editor),
                2 => ai_merge::retry(cx.editor),
                _ => {
                    ai_merge::cancel(cx.editor);
                    Ok(())
                }
            };
            if let Err(error) = result {
                ai_merge::cancel(cx.editor);
                cx.editor.set_error(error.to_string());
            }
        })))
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        if area.area() == 0 {
            return;
        }
        let margin = if area.width < 50 || area.height < 12 {
            0
        } else {
            4
        };
        let width = area
            .width
            .saturating_sub(margin)
            .clamp(1, 110)
            .min(area.width);
        let height = area
            .height
            .saturating_sub(margin)
            .clamp(1, 32)
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
            .borders(Borders::ALL)
            .title(self.title.as_str())
            .style(style);
        let inner = block.inner(area);
        block.render(area, surface);
        let help: Vec<&str> = if inner.width >= 46 {
            vec![if self.can_accept {
                "Enter 接受 · r 重试 · Esc 取消 · j/k 滚动"
            } else {
                "需人工处理 · r 重试 · Esc 取消 · j/k 滚动"
            }]
        } else if inner.width >= 30 {
            vec![if self.can_accept {
                "Enter 接受  r 重试  Esc 取消"
            } else {
                "需人工处理  r 重试  Esc 取消"
            }]
        } else if inner.width >= 18 {
            vec![
                if self.can_accept {
                    "Enter 接受  r 重试"
                } else {
                    "需人工处理  r 重试"
                },
                "Esc 取消  j/k 滚动",
            ]
        } else {
            vec![
                if self.can_accept {
                    "Enter:OK"
                } else {
                    "Manual"
                },
                "Esc:No r:R",
            ]
        };
        let footer_height = inner.height.min(help.len() as u16);
        let body = inner.clip_bottom(footer_height);
        let paragraph = Paragraph::new(&self.text)
            .style(style)
            .wrap(Wrap { trim: false });
        let rows = paragraph.required_size(body.width.max(1)).1 as usize;
        self.scroll = self.scroll.min(rows.saturating_sub(body.height as usize));
        paragraph
            .scroll((self.scroll.min(u16::MAX as usize) as u16, 0))
            .render(body, surface);
        for (row, help) in help.into_iter().take(footer_height as usize).enumerate() {
            surface.set_stringn(
                inner.x,
                inner.bottom() - footer_height + row as u16,
                help,
                inner.width as usize,
                style,
            );
        }
    }
    fn cursor(&self, area: Rect, _editor: &Editor) -> (Option<Position>, CursorKind) {
        (
            Some(Position::new(area.y as usize, area.x as usize)),
            CursorKind::Hidden,
        )
    }
}
