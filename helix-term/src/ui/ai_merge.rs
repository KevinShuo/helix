//! A read-only preview of the exact block replacement, separate from the draft.
use super::ai_preview_text::{
    display,
    review::{ReviewLines, ReviewText, Role},
    scroll_key_by,
};
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
    widgets::{Block, Borders, Widget},
};

pub(crate) struct Preview {
    id: u64,
    title: String,
    text: ReviewText,
    can_accept: bool,
    scroll: usize,
    page_rows: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::theme::Theme;

    fn target() -> Target {
        Target {
            block: 0,
            path: "test.rs".into(),
            language: "rust".into(),
            base: String::new(),
            current: String::new(),
            incoming: String::new(),
            result: "old();\n".into(),
            context: Vec::new(),
            intent: String::new(),
            range: 0..7,
        }
    }

    #[test]
    fn ai_merge_large_preview_requires_explicit_budget_without_changing_candidate() {
        let target = target();
        let suggestion = Suggestion {
            code: Some(format!("{}TAIL\n", "x".repeat(70_000))),
            explanation: "建议".into(),
        };
        let code = suggestion.code.clone();
        assert!(Preview::new(1, &target, &suggestion).is_err());
        let mut preview = Preview::new_with_limit(1, &target, &suggestion, 128 * 1024).unwrap();
        assert!(preview.can_accept);
        let area = Rect::new(0, 0, 1, 4);
        let mut buffer = Buffer::empty(area);
        preview.scroll = 70_000;
        preview
            .text
            .render(area, &mut buffer, &Theme::default(), &mut preview.scroll);
        assert!(preview.scroll > u16::MAX as usize);
        assert_eq!(suggestion.code, code);
        assert_eq!(target.result, "old();\n");
    }

    #[test]
    fn ai_merge_budget_counts_complete_decorated_preview() {
        let target = target();
        let suggestion = Suggestion {
            code: Some("x".repeat(100)),
            explanation: "建议".into(),
        };
        assert!(Preview::new_with_limit(1, &target, &suggestion, 100).is_err());
        assert!(Preview::new_with_limit(1, &target, &suggestion, usize::MAX).is_err());
    }
}

impl Preview {
    pub(crate) fn new(id: u64, target: &Target, suggestion: &Suggestion) -> Result<Self> {
        Self::new_with_limit(id, target, suggestion, 60_000)
    }

    pub(crate) fn new_with_limit(
        id: u64,
        target: &Target,
        suggestion: &Suggestion,
        max_bytes: usize,
    ) -> Result<Self> {
        ensure!(
            max_bytes > 0 && max_bytes <= 16 * 1024 * 1024,
            "AI 预览预算无效"
        );
        let status = if suggestion.code.is_some() {
            "候选待复核"
        } else {
            "需人工处理"
        };
        let too_large = "AI 候选过大，无法完整预览；请缩小冲突块或减少 max-tokens，草稿已保留";
        let mut lines = ReviewLines::default();
        lines.push(Role::Heading, status);
        let explanation = suggestion.explanation.trim();
        // Keep a short explanation beside the change. A long one follows the
        // candidate so it cannot push the first changed lines off the screen.
        let short = explanation.lines().count() <= 2 && explanation.chars().count() <= 100;
        if short || suggestion.code.is_none() {
            lines.push(Role::Text, explanation);
        }
        if let Some(code) = &suggestion.code {
            ensure!(
                target
                    .result
                    .len()
                    .saturating_add(code.len())
                    .saturating_add(explanation.len())
                    <= max_bytes,
                "{too_large}"
            );
            lines.push(Role::Note, "− 当前 Result   + 候选修改");
            if code.is_empty() {
                lines.push(Role::Removed, "删除此冲突块");
            }
            lines.diff(&target.result, code, true);
            if !short {
                lines.push(Role::Heading, "修改说明");
                lines.push(Role::Text, explanation);
            }
            lines.push(Role::Note, "接受到 Result 后仍需 Review / Apply。");
        }
        ensure!(lines.source.len() <= max_bytes, "{too_large}");
        Ok(Self {
            id,
            title: format!(
                " 冲突建议 · {} · 冲突块 {} ",
                display(&target.path),
                target.block + 1
            ),
            text: lines.finish(),
            can_accept: suggestion.code.is_some(),
            scroll: 0,
            page_rows: 1,
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
        if scroll_key_by(*key, &mut self.scroll, self.page_rows) {
            return EventResult::Consumed(None);
        }
        let action = match *key {
            key!(Enter) if self.can_accept => 0,
            key!(Esc) | ctrl!('c') => 1,
            key!('r') => 2,
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
        self.page_rows = usize::from(body.height.saturating_sub(1).max(1));
        self.text
            .render(body, surface, &cx.editor.theme, &mut self.scroll);
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
