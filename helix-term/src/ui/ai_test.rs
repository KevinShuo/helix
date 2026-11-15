//! Scrollable multi-file test/build preview and execution report.
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    handlers::ai_test,
    key,
};
use helix_core::Position;
use helix_view::{
    ai_test::Operation,
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
    text: Text<'static>,
    scroll: usize,
    applied: bool,
}

fn diff(before: &str, after: &str) -> String {
    let old = before.lines().collect::<Vec<_>>();
    let new = after.lines().collect::<Vec<_>>();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let start = prefix.saturating_sub(3);
    let mut out = format!(
        "@@ -{},{} +{},{} @@\n",
        start + 1,
        old.len() - suffix - start,
        start + 1,
        new.len() - suffix - start
    );
    for line in &old[start..prefix] {
        out.push_str(&format!(" {line}\n"));
    }
    for line in &old[prefix..old.len() - suffix] {
        out.push_str(&format!("-{line}\n"));
    }
    for line in &new[prefix..new.len() - suffix] {
        out.push_str(&format!("+{line}\n"));
    }
    for line in new[new.len() - suffix..].iter().take(3) {
        out.push_str(&format!(" {line}\n"));
    }
    out
}

impl Preview {
    pub(crate) fn new(op: &Operation) -> Self {
        let mut text = format!(
            "目标：{}\n测试：{}\n",
            op.target.name,
            op.project.test_file.display()
        );
        for note in op.project.notes.iter().chain(op.assumptions.iter()) {
            text.push_str(&format!("• {note}\n"));
        }
        if !op.applied {
            text.push_str("\n以下修改将应用到缓冲区，保存后才能运行。\n");
            if let Some(changes) = &op.changes {
                for change in changes {
                    let path = change
                        .before
                        .path
                        .strip_prefix(&op.project.root)
                        .unwrap_or(&change.before.path)
                        .display();
                    text.push_str(&format!(
                        "\n--- {path}\n+++ {path}\n{}",
                        diff(change.before.text.as_deref().unwrap_or(""), &change.after)
                    ));
                }
            }
        }
        if let Some(report) = &op.report {
            text.push_str(&format!(
                "\n运行阶段：{:?}；结果：{}\n{}\n",
                report.stage,
                if report.success {
                    "通过（不代表覆盖充分）"
                } else {
                    "失败"
                },
                report.log
            ));
        }
        let text = text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
            .collect::<String>();
        Self {
            id: op.id,
            text: Text::from(text),
            scroll: 0,
            applied: op.applied,
        }
    }
}

impl Component for Preview {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if !cx
            .editor
            .ai_test
            .as_ref()
            .is_some_and(|op| op.id == self.id && op.source_valid(cx.editor))
        {
            return EventResult::Consumed(Some(Box::new(|compositor, _| {
                compositor.remove("ai-test");
            })));
        }
        let Event::Key(key) = event else {
            return EventResult::Consumed(None);
        };
        let applied = self.applied;
        let action = match *key {
            key!(Enter) => 0,
            key!(Esc) | ctrl!('c') => 1,
            key!('r') if !applied => 2,
            key!('v') if applied => 3,
            key!(Down) | key!('j') => {
                self.scroll = self.scroll.saturating_add(1);
                return EventResult::Consumed(None);
            }
            key!(Up) | key!('k') => {
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
        EventResult::Consumed(Some(Box::new(move |compositor, cx| {
            compositor.remove("ai-test");
            let result = match action {
                0 if !applied => ai_test::accept(cx.editor),
                2 => ai_test::retry(cx.editor),
                3 => ai_test::run_last(cx.editor),
                1 if !applied => {
                    ai_test::cancel(cx.editor);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                cx.editor.set_error(error.to_string());
            }
        })))
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        let width = area.width.saturating_sub(4).clamp(1, 120).min(area.width);
        let height = area.height.saturating_sub(4).clamp(1, 40).min(area.height);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        surface.clear_with(popup, cx.editor.theme.get("ui.popup"));
        let rows =
            super::text::required_size(&self.text, width.saturating_sub(2).max(1)).1 as usize;
        self.scroll = self
            .scroll
            .min(rows.saturating_sub(height.saturating_sub(2) as usize));
        let title = if self.applied {
            " AI 测试结果 | Enter/Esc 关闭 · v 再次运行 "
        } else {
            " AI 单元测试 | Enter 接受全部 · Esc 取消 · r 重试 "
        };
        Paragraph::new(&self.text)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll.min(u16::MAX as usize) as u16, 0))
            .render(popup, surface);
    }
    fn cursor(&self, _area: Rect, _editor: &Editor) -> (Option<Position>, CursorKind) {
        (Some(Position::new(0, 0)), CursorKind::Hidden)
    }
    fn id(&self) -> Option<&'static str> {
        Some("ai-test")
    }
}
