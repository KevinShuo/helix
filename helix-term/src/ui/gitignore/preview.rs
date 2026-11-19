use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

use anyhow::{ensure, Context as _, Result};
use helix_event::TaskController;
use helix_vcs::ignore_completion::preview::{scan, Report};
use helix_view::{graphics::Rect, DocumentId, Editor, ViewId};
use tui::{
    buffer::Buffer,
    widgets::{Block, Widget},
};

use super::display_text;
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl, job, key,
    ui::ai_preview_text::{scroll_key, PreviewText},
};

const ID: &str = "gitignore-rule-preview";

struct Target {
    document: DocumentId,
    view: ViewId,
    version: i32,
    cursor: usize,
    path: PathBuf,
}

impl Target {
    fn current(&self, editor: &Editor) -> bool {
        let (view, doc) = helix_view::current_ref!(editor);
        self.view == view.id
            && self.document == doc.id()
            && self.version == doc.version()
            && doc.path() == Some(self.path.as_path())
            && self.cursor
                == doc
                    .selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..))
    }
}

pub(crate) fn open(cx: &mut Context) -> Result<()> {
    let (view, doc) = helix_view::current_ref!(cx.editor);
    let path = doc
        .path()
        .filter(|p| p.file_name().is_some_and(|n| n == ".gitignore"))
        .context("Open a .gitignore file to preview a rule")?;
    let base = path
        .parent()
        .context("Missing .gitignore directory")?
        .to_owned();
    let text = doc.text().clone();
    let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
    let byte = text.char_to_byte(cursor);
    let target = Target {
        document: doc.id(),
        view: view.id,
        version: doc.version(),
        cursor,
        path: path.to_owned(),
    };
    let trust = cx.editor.workspace_trust.clone();
    let mut task = TaskController::new();
    let handle = task.restart();
    let path = target.path.clone();
    cx.jobs
        .after_event
        .push(Box::new(move |editor, compositor| {
            if !target.current(editor) {
                return;
            }
            compositor.replace_or_push(
                ID,
                Preview {
                    target,
                    _task: task,
                    scroll: 0,
                    text: PreviewText::new("Scanning untracked files…".into()),
                },
            );
        }));
    tokio::spawn(async move {
        let worker = handle.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<PreviewText> {
            let text = text.to_string();
            // Git uses LF-delimited patterns, including when the editor recognizes
            // other Unicode line separators. Work from the captured byte offset.
            let line = text[..byte].bytes().filter(|&b| b == b'\n').count();
            let rule = text.lines().nth(line).unwrap_or_default();
            ensure!(
                !matches!(rule.trim_end_matches(' '), "" | "!") && !rule.starts_with('#'),
                "Place the cursor on a .gitignore rule"
            );
            let workspace = helix_loader::find_workspace_in(&base).0;
            let trusted = trust
                .query(&workspace, helix_loader::workspace_trust::TrustQuery::Git)
                .is_trusted();
            let report = scan(&base, &text, line, trusted, || worker.is_canceled())?;
            Ok(format_report(&path, line, rule, &report))
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|r| r);
        job::dispatch(move |editor, compositor| {
            if handle.is_canceled() {
                return;
            }
            let Some(preview) = compositor.find_id::<Preview>(ID) else {
                return;
            };
            if !preview.target.current(editor) {
                compositor.remove(ID);
                return;
            }
            match result {
                Ok(text) => preview.text = text,
                Err(error) => {
                    compositor.remove(ID);
                    editor.set_error(format!("{error:#}"));
                }
            }
        })
        .await;
    });
    Ok(())
}

fn display_path(path: &Path) -> String {
    let Some(path_text) = path.to_str() else {
        return format!("{:?}", path.as_os_str());
    };
    #[cfg(windows)]
    let path_text = path_text.replace('\\', "/");
    // Quote unusual names so that a newline or tab cannot masquerade as
    // another row or as a filename containing spaces.
    if path_text
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
    {
        format!("{path_text:?}")
    } else {
        path_text.into()
    }
}

fn display_rule(rule: &str) -> String {
    // One very long glob may win for every example in the panel.
    match rule.char_indices().nth(1024) {
        Some((end, _)) => format!("{}…", display_text(&rule[..end])),
        None => display_text(rule),
    }
}

fn format_report(path: &Path, line: usize, rule: &str, report: &Report) -> PreviewText {
    let origin = path.strip_prefix(&report.root).unwrap_or(path);
    let mut text = format!(
        "Rule {}:{}  {}\n{} changed, {} unchanged\n",
        display_path(origin),
        line + 1,
        display_rule(rule),
        report.changed,
        report.unchanged
    );
    if report.partial {
        text.push_str("Partial scan. Counts cover the paths examined.\n");
    }
    let total = report.changed + report.unchanged;
    if report.entries.len() < total {
        writeln!(
            text,
            "Showing {} of {total} matching files.",
            report.entries.len()
        )
        .unwrap();
    }
    if report.entries.is_empty() {
        text.push_str("\nNo matching untracked files found.");
    }
    for entry in &report.entries {
        let state = match (entry.before_ignored, entry.after_ignored) {
            (false, true) => "Now ignored",
            (true, false) => "Now included",
            (true, true) => "Still ignored",
            (false, false) => "Still included",
        };
        writeln!(text, "\n{state}  {}", display_path(&entry.path)).unwrap();
        if let Some(winner) = &entry.winner {
            writeln!(
                text,
                "  {}:{}  {}",
                display_path(&winner.source),
                winner.line,
                display_rule(&winner.pattern)
            )
            .unwrap();
        } else {
            text.push_str("  Included by default\n");
        }
    }
    PreviewText::new(text)
}

struct Preview {
    target: Target,
    // Dropping the panel cancels its walk and invalidates queued callbacks.
    _task: TaskController,
    text: PreviewText,
    scroll: usize,
}

impl Component for Preview {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if !self.target.current(cx.editor) || matches!(event, Event::Key(key!(Esc) | ctrl!('c'))) {
            return EventResult::Consumed(Some(Box::new(|compositor, _| {
                compositor.remove(ID);
            })));
        }
        if let Event::Key(key) = event {
            scroll_key(*key, &mut self.scroll);
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        if area.area() == 0 {
            return;
        }
        let width = area.width.saturating_sub(2).clamp(1, 110);
        let height = area.height.saturating_sub(2).clamp(1, 32);
        let area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let style = cx.editor.theme.get("ui.popup");
        surface.clear_with(area, style);
        let block = Block::bordered().title(" Gitignore rule preview | ↑↓ scroll · Esc close ");
        let body = block.inner(area);
        block.render(area, surface);
        if !self.target.current(cx.editor) {
            if body.area() > 0 {
                surface.set_stringn(
                    body.x,
                    body.y,
                    "Source changed. Reopen the preview.",
                    body.width as usize,
                    style,
                );
            }
            return;
        }
        self.text.render(body, surface, style, &mut self.scroll);
    }

    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignore_preview_escapes_unusual_names_and_bounds_long_patterns() {
        assert_eq!(display_path(Path::new("正常 file.log")), "正常 file.log");
        assert_eq!(display_path(Path::new("two\nrows")), "\"two\\nrows\"");
        assert_eq!(
            display_rule(&"界".repeat(1200)),
            format!("{}…", "界".repeat(1024))
        );
    }
}
