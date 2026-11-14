use super::*;
use helix_view::theme::Style;
use tui::{
    text::Text,
    widgets::{Block, Borders, Paragraph, Widget},
};

impl CommitWorkspace {
    pub(super) fn dock(&self, area: Rect) -> Rect {
        if self.fullscreen || area.height < 16 {
            area
        } else {
            let h = (area.height * 2 / 3).max(14).min(area.height);
            Rect::new(area.x, area.bottom() - h, area.width, h)
        }
    }
    pub(super) fn draw(&mut self, area: Rect, frame: &mut Buffer, editor: &Editor) {
        let area = self.dock(area);
        self.area = area;
        let normal = editor.theme.get("ui.text");
        frame.clear_with(area, editor.theme.get("ui.background"));
        let language = match self.language {
            CommitLanguage::Chinese => "中文",
            CommitLanguage::English => "English",
        };
        let branch = self
            .snapshot
            .as_ref()
            .map_or("读取中", |s| s.head_name.as_str());
        line(
            frame,
            area,
            0,
            &format!(
                "AI 提交 | {branch}{} | {language} | {}",
                if self.snapshot.as_ref().is_some_and(|s| s.is_detached()) {
                    " (detached HEAD)"
                } else {
                    ""
                },
                self.intent
            ),
            editor.theme.get("ui.text.focus"),
        );
        let message_height = 7.min(area.height.saturating_sub(7));
        let upper_height = area.height.saturating_sub(message_height + 4);
        self.files_area = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width / 2,
            upper_height,
        );
        self.diff_area = Rect::new(
            area.x + area.width / 2,
            area.y.saturating_add(1),
            area.width - area.width / 2,
            upper_height,
        );
        self.message_area = Rect::new(
            area.x,
            area.y.saturating_add(1 + upper_height),
            area.width,
            message_height,
        );
        let rows = self.rows();
        let count = self
            .snapshot
            .as_ref()
            .map_or(0, |s| s.files.iter().filter(|f| f.staged).count());
        let offset = self
            .row
            .saturating_sub(self.files_area.height.saturating_sub(2) as usize / 2);
        let mut lines = vec![];
        if let Some(snapshot) = &self.snapshot {
            for (i, row) in rows
                .iter()
                .enumerate()
                .skip(offset)
                .take(self.files_area.height.saturating_sub(2) as usize)
            {
                if let Some(file) = snapshot.files.iter().find(|f| f.id == row.id) {
                    let mark = if row.staged {
                        "[已暂存]"
                    } else if self.selected.contains(&row.id) {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    let note = if file.partial && !row.staged {
                        " 整文件替换部分暂存!"
                    } else if !file.ai_eligible {
                        " 需手查"
                    } else {
                        ""
                    };
                    lines.push(format!(
                        "{} {mark} {}{note}",
                        if i == self.row { ">" } else { " " },
                        file.path.to_string_lossy()
                    ));
                }
            }
        }
        pane(
            frame,
            self.files_area,
            &format!("文件 | 已有暂存 {count} 全部包含"),
            &lines.join("\n"),
            0,
        );
        let (diff, title) = self
            .active()
            .and_then(|row| {
                self.snapshot.as_ref().and_then(|s| {
                    s.files.iter().find(|f| f.id == row.id).map(|f| {
                        let reason = self
                            .reasons
                            .get(&f.id)
                            .or(f.exclusion.as_ref())
                            .cloned()
                            .unwrap_or_default();
                        (
                            format!(
                                "{reason}\n{}",
                                if row.staged {
                                    &f.staged_diff
                                } else {
                                    &f.worktree_diff
                                }
                            ),
                            if row.staged {
                                "暂存 diff (HEAD → index)"
                            } else {
                                "候选 diff (index → 磁盘)"
                            },
                        )
                    })
                })
            })
            .unwrap_or(("没有文件改动".into(), "Diff"));
        let diff_lines = diff.lines().count();
        self.diff_scroll = self
            .diff_scroll
            .min(diff_lines.saturating_sub(self.diff_area.height.saturating_sub(2) as usize));
        pane(frame, self.diff_area, title, &diff, self.diff_scroll);
        let mut text = self.draft.text.clone();
        if self.editing {
            text.insert(self.draft.cursor, '▏');
        }
        let cursor_line = self.draft.text[..self.draft.cursor]
            .lines()
            .count()
            .saturating_sub(1);
        let scroll = if self.editing {
            cursor_line.saturating_sub(message_height.saturating_sub(3) as usize)
        } else {
            0
        };
        use helix_core::unicode::width::UnicodeWidthStr;
        let long_title = self
            .draft
            .text
            .lines()
            .next()
            .is_some_and(|line| line.width() > 72);
        let title = if self.editing {
            "编辑提交信息 · Esc / Ctrl-s 结束编辑 · Enter 换行"
        } else if long_title {
            "提交信息 · 标题超过 72 显示列，建议缩短"
        } else if self.draft.reviewed {
            "提交信息 · 已检查范围"
        } else {
            "提交信息 · 需生成或手动检查"
        };
        pane(frame, self.message_area, title, &text, scroll);
        let y = (1 + upper_height + message_height) as usize;
        line(frame, area, y, &self.message, normal);
        line(
            frame,
            area,
            y + 1,
            "空格 勾选 | s 暂存所选 | u 取消当前暂存 | a 推荐 | g 生成 | l 中/英",
            normal,
        );
        line(
            frame,
            area,
            y + 2,
            "e 编辑 | i 意图 | v 保留手动文本 | C 提交 | d 完整diff | r 刷新 | F11 全屏 | q 关闭",
            normal,
        );
    }
}

pub(super) struct PatchView {
    pub text: String,
}
impl Component for PatchView {
    fn required_size(&mut self, viewport: (u16, u16)) -> Option<(u16, u16)> {
        use helix_core::unicode::width::UnicodeWidthStr;
        let width = viewport.0.max(1) as usize;
        let rows = self
            .text
            .lines()
            .map(|l| l.width().max(1).div_ceil(width))
            .sum::<usize>();
        Some((viewport.0, rows.min(u16::MAX as usize - 2) as u16))
    }
    fn render(&mut self, area: Rect, frame: &mut Buffer, cx: &mut Context) {
        use tui::widgets::Wrap;
        Paragraph::new(&Text::from(self.text.as_str()))
            .wrap(Wrap { trim: false })
            .scroll((cx.scroll.unwrap_or(0).min(u16::MAX as usize) as u16, 0))
            .render(area, frame);
    }
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        if let Event::Key(key) = event {
            if matches!(key.code, KeyCode::Esc | KeyCode::PageUp | KeyCode::PageDown)
                || key.modifiers == KeyModifiers::CONTROL
                    && matches!(
                        key.code,
                        KeyCode::Char('u') | KeyCode::Char('d') | KeyCode::Char('c')
                    )
            {
                return EventResult::Ignored(None);
            }
            return EventResult::Consumed(None);
        }
        EventResult::Ignored(None)
    }
}
fn line(frame: &mut Buffer, area: Rect, row: usize, text: &str, style: Style) {
    if row < area.height as usize {
        frame.set_stringn(
            area.x,
            area.y + row as u16,
            &super::super::render::safe_text(text),
            area.width as usize,
            style,
        );
    }
}
fn pane(frame: &mut Buffer, area: Rect, title: &str, text: &str, scroll: usize) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let safe = text
        .lines()
        .map(super::super::render::safe_text)
        .collect::<Vec<_>>()
        .join("\n");
    if area.height < 3 || area.width < 3 {
        Paragraph::new(&Text::from(safe))
            .scroll((scroll.min(u16::MAX as usize) as u16, 0))
            .render(area, frame);
        return;
    }
    Paragraph::new(&Text::from(safe))
        .block(Block::default().borders(Borders::ALL).title(title))
        .scroll((scroll.min(u16::MAX as usize) as u16, 0))
        .render(area, frame);
}
