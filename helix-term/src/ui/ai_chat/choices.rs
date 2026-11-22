use super::display;
use crate::{compositor::Context, handlers::ai_optimize};
use anyhow::{ensure, Context as _, Result};
use helix_core::Selection;
use helix_view::{ai_chat::Session, editor::Action, graphics::Rect, Editor};
use std::path::Path;
use tui::buffer::Buffer;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Location,
    Suggestion,
}

impl Kind {
    pub(super) fn len(self, session: &Session) -> usize {
        match self {
            Self::Location => session.locations.len(),
            Self::Suggestion => session.suggestions.len(),
        }
    }

    fn label(self, session: &Session, i: usize) -> String {
        match self {
            Self::Location => {
                let (path, line) = &session.locations[i];
                let path = helix_stdx::path::normalize(path);
                let root = session
                    .explore_root
                    .as_deref()
                    .or(session.project_root.as_deref())
                    .map(helix_stdx::path::normalize);
                let path = root
                    .as_deref()
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(&path);
                let path = helix_stdx::path::get_relative_path(path);
                format!("{}:{line}", display(&path.to_string_lossy()))
            }
            Self::Suggestion => {
                let suggestion = &session.suggestions[i];
                let action = if suggestion.before.is_some() && suggestion.after.is_some() {
                    "可预览"
                } else {
                    "仅建议"
                };
                format!("{}. {action} · {}", i + 1, display(&suggestion.title))
            }
        }
    }
}

pub(super) struct Choices {
    pub kind: Kind,
    pub index: usize,
    pub error: String,
}

impl Choices {
    pub(super) fn new(kind: Kind) -> Self {
        Self {
            kind,
            index: 0,
            error: String::new(),
        }
    }

    pub(super) fn activate(&self, cx: &mut Context) -> Result<()> {
        let session = cx.editor.ai_chat.as_ref().context("代码对话已关闭")?;
        match self.kind {
            Kind::Location => {
                let (path, line) = session
                    .locations
                    .get(self.index)
                    .context("没有此源码位置")?
                    .clone();
                open_location(cx.editor, &path, line)
            }
            Kind::Suggestion => ai_optimize::preview(cx, self.index + 1),
        }
    }

    pub(super) fn render(
        &self,
        area: Rect,
        surface: &mut Buffer,
        editor: &Editor,
        session: &Session,
    ) {
        if area.area() == 0 {
            return;
        }
        let title = match self.kind {
            Kind::Location => "源码位置",
            Kind::Suggestion => "优化建议",
        };
        let help = if self.error.is_empty() {
            match self.kind {
                Kind::Location if area.width < 36 => "↑↓ 选 · Enter 跳转 · Esc",
                Kind::Location => "↑↓ 选择 · Enter 跳转 · Esc 返回",
                Kind::Suggestion
                    if session
                        .suggestions
                        .get(self.index)
                        .is_some_and(|s| s.before.is_none()) =>
                {
                    "仅供讨论 · ↑↓ 选择 · Esc 返回"
                }
                Kind::Suggestion if area.width < 36 => "↑↓ 选 · Enter 预览 · Esc",
                Kind::Suggestion => "↑↓ 选择 · Enter 预览 · Esc 返回",
            }
        } else {
            &self.error
        };
        let info = editor.theme.get("ui.text.info");
        surface.set_stringn(area.x, area.y, title, area.width as usize, info);
        let rows = area.clip_top(1).clip_bottom(1);
        let first = (self.index + 1).saturating_sub(rows.height as usize);
        for (y, i) in (first..self.kind.len(session))
            .take(rows.height as usize)
            .enumerate()
        {
            let selected = self.index == i;
            let marker = if selected { "›" } else { " " };
            let text = self.kind.label(session, i);
            let style = editor.theme.get(if selected {
                "ui.menu.selected"
            } else {
                "ui.text"
            });
            let y = rows.y + y as u16;
            let (x, _) = surface.set_stringn(rows.x, y, marker, rows.width as usize, style);
            let x = x.saturating_add(1);
            surface.set_string_truncated(
                x,
                y,
                &text,
                rows.right().saturating_sub(x) as usize,
                |_| style,
                true,
                matches!(self.kind, Kind::Location),
            );
        }
        if area.height > 1 {
            surface.set_stringn(area.x, area.bottom() - 1, help, area.width as usize, info);
        }
    }
}

fn open_location(editor: &mut Editor, path: &Path, line: usize) -> Result<()> {
    ensure!(line > 0, "行号无效");
    let path = helix_stdx::path::canonicalize(path);
    let id = if let Some(doc) = editor.document_by_path(&path) {
        doc.id()
    } else {
        ensure!(path.is_file(), "引用的源文件已移走");
        editor.open(&path, Action::Load)?
    };
    let doc = editor.document(id).context("无法打开源文件")?;
    ensure!(
        line <= doc.text().len_lines(),
        "源文件已更改，引用行已不存在"
    );
    let offset = doc.text().line_to_char(line - 1);
    editor.switch(id, Action::Replace);
    let (view, doc) = current!(editor);
    ensure!(view.doc == id, "无法切换到引用的源文件");
    let view_id = view.id;
    doc.set_selection(view_id, Selection::point(offset));
    editor.ensure_cursor_in_view(view_id);
    Ok(())
}
