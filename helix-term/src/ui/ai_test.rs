//! Scrollable multi-file test/build preview and execution report.
mod report;

use super::ai_preview_text::{
    display,
    review::{ReviewLines, ReviewText, Role},
    scroll_key_by, PreviewText,
};
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
    input::KeyEvent,
    Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(crate) struct Preview {
    id: u64,
    file: String,
    text: ReviewText,
    log: Option<PreviewText>,
    show_log: bool,
    log_scroll: usize,
    scroll: usize,
    page_rows: usize,
    applied: bool,
}

impl Preview {
    pub(crate) fn new(op: &Operation) -> Self {
        let path = op
            .project
            .test_file
            .strip_prefix(&op.project.root)
            .unwrap_or(&op.project.test_file);
        let file = display(&path.to_string_lossy());
        let mut text = ReviewLines::default();
        if let Some(report) = &op.report {
            if !op.applied {
                text.push(Role::Note, "上次运行");
            }
            report::summary(report, &mut text);
        }
        text.push(
            Role::Heading,
            &format!("目标   {}", display(&op.target.name)),
        );
        if !op.applied {
            text.push(
                Role::Note,
                if op.repairs > 0 {
                    "修复待确认 · 接受后请保存并重新运行"
                } else {
                    "待确认 · 接受后请保存并运行"
                },
            );
            if let Some(changes) = &op.changes {
                for change in changes {
                    let path = change
                        .before
                        .path
                        .strip_prefix(&op.project.root)
                        .unwrap_or(&change.before.path)
                        .display();
                    let kind = if change.before.text.is_none() {
                        "新建"
                    } else {
                        "修改"
                    };
                    text.push(
                        Role::Heading,
                        &format!("\n{kind}   {}", display(&path.to_string())),
                    );
                    text.diff(
                        change.before.text.as_deref().unwrap_or(""),
                        &change.after,
                        false,
                    );
                }
            }
        } else {
            text.push(Role::Note, &file);
            if op.report.is_none() {
                text.push(Role::Text, "修改已接受 · 保存后可运行测试");
            }
        }
        for note in op.project.notes.iter().chain(op.assumptions.iter()) {
            text.push(Role::Note, &format!("• {note}"));
        }
        Self {
            id: op.id,
            file,
            text: text.finish(),
            log: op.report.as_ref().map(|r| PreviewText::new(r.log.clone())),
            show_log: false,
            log_scroll: 0,
            scroll: 0,
            page_rows: 1,
            applied: op.applied,
        }
    }

    fn read_key(&mut self, key: KeyEvent) -> bool {
        if key == key!('l') && self.log.is_some() {
            self.show_log = !self.show_log;
            return true;
        }
        if self.show_log && !self.applied && key == key!(Enter) {
            self.show_log = false;
            return true;
        }
        let scroll = if self.show_log {
            &mut self.log_scroll
        } else {
            &mut self.scroll
        };
        scroll_key_by(key, scroll, self.page_rows)
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
        if self.read_key(*key) {
            return EventResult::Consumed(None);
        }
        let applied = self.applied;
        let action = match *key {
            key!(Enter) => 0,
            key!(Esc) | ctrl!('c') => 1,
            key!('r') if !applied => 2,
            key!('v') if applied => 3,
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
            .clamp(1, 100)
            .min(area.width);
        let height = area
            .height
            .saturating_sub(margin)
            .clamp(1, 40)
            .min(area.height);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        surface.clear_with(popup, cx.editor.theme.get("ui.popup"));
        let scope = if self.show_log {
            "测试 · 原始输出"
        } else if self.applied {
            "测试结果"
        } else {
            "测试修改"
        };
        let title = format!(" {scope} · {} ", self.file);
        let style = cx.editor.theme.get("ui.popup");
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title.as_str())
            .style(style);
        let inner = block.inner(popup);
        block.render(popup, surface);
        let mut hints = Vec::new();
        let short = inner.width < 28;
        if self.applied {
            hints.extend([
                if short {
                    "Esc 关闭"
                } else {
                    "Enter/Esc 关闭"
                },
                if short { "v 重跑" } else { "v 再次运行" },
            ]);
        } else {
            hints.extend([
                if self.show_log {
                    if short {
                        "Enter 返回"
                    } else {
                        "Enter 返回修改"
                    }
                } else if short {
                    "Enter 接受"
                } else {
                    "Enter 接受全部"
                },
                "Esc 取消",
            ]);
        }
        if self.log.is_some() {
            hints.push(if self.show_log {
                "l 返回"
            } else if short {
                "l 日志"
            } else {
                "l 原始输出"
            });
        }
        if !self.applied {
            hints.push("r 重试");
        }
        let footer = footer_lines(&hints, inner.width);
        let visible = inner.height.saturating_sub(1).min(footer.len() as u16);
        let body = inner.clip_bottom(visible);
        self.page_rows = usize::from(body.height.saturating_sub(1).max(1));
        if let Some(log) = self.log.as_mut().filter(|_| self.show_log) {
            log.render(body, surface, style, &mut self.log_scroll);
        } else {
            self.text
                .render(body, surface, &cx.editor.theme, &mut self.scroll);
        }
        for (i, line) in footer.iter().take(visible as usize).enumerate() {
            surface.set_stringn(
                inner.x,
                inner.bottom() - visible + i as u16,
                line,
                inner.width as usize,
                style.patch(cx.editor.theme.get("ui.text.info")),
            );
        }
    }
    fn cursor(&self, _area: Rect, _editor: &Editor) -> (Option<Position>, CursorKind) {
        (Some(Position::new(0, 0)), CursorKind::Hidden)
    }
    fn id(&self) -> Option<&'static str> {
        Some("ai-test")
    }
}

fn footer_lines(hints: &[&str], width: u16) -> Vec<String> {
    use helix_core::unicode::width::UnicodeWidthStr;
    let mut rows = vec![String::new()];
    for hint in hints {
        let row = rows.last_mut().unwrap();
        if !row.is_empty() && row.width() + 2 + hint.width() > width as usize {
            rows.push(hint.to_string());
        } else {
            if !row.is_empty() {
                row.push_str("  ");
            }
            row.push_str(hint);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_log_returns_to_review_before_enter_can_accept() {
        let mut preview = Preview {
            id: 1,
            file: "test.py".into(),
            text: ReviewLines::default().finish(),
            log: Some(PreviewText::new("compiler output".into())),
            show_log: false,
            log_scroll: 0,
            scroll: 5,
            page_rows: 3,
            applied: false,
        };
        assert!(preview.read_key(key!('l')) && preview.show_log);
        assert!(preview.read_key(key!(PageDown)));
        assert_eq!((preview.log_scroll, preview.scroll), (3, 5));
        assert!(preview.read_key(key!(Enter)) && !preview.show_log);
        assert!(!preview.read_key(key!(Enter)));
        assert!(preview.read_key(key!('l')) && preview.show_log);
        assert_eq!(preview.log_scroll, 3);
        assert!(preview.read_key(key!('l')) && !preview.show_log);
        assert_eq!(preview.scroll, 5);
    }
}
