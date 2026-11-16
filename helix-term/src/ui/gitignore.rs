//! Offline template selection. A session owns its fixed target and discards late detection.
mod selection;
use selection::TemplateSelection;

use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl,
    gitignore::{self, Project},
    job, key,
    ui::Prompt,
};
use anyhow::{Context as _, Result};
use helix_core::{Position, Selection, Transaction};
use helix_view::{
    document::Mode,
    editor::Action,
    graphics::{CursorKind, Rect},
    Editor,
};
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::task::JoinHandle;
use tui::{
    buffer::Buffer,
    widgets::{Block, Widget},
};

const ID: &str = "gitignore-generator";
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

pub(crate) fn open(cx: &mut crate::commands::Context) {
    let Some(start) = cx.editor.git_context_path() else {
        return;
    };
    cx.callback.push(Box::new(move |compositor, cx| {
        compositor.remove(ID);
        compositor.push(Box::new(Generator::new(start)));
        cx.editor.clear_status();
    }));
}

pub(crate) fn open_typed(cx: &mut Context) {
    let Some(start) = cx.editor.git_context_path() else {
        return;
    };
    cx.jobs.callback(async move {
        Ok(job::Callback::EditorCompositor(Box::new(
            move |_, compositor| {
                compositor.remove(ID);
                compositor.push(Box::new(Generator::new(start)));
            },
        )))
    });
}

struct Generator {
    session: u64,
    project: Option<Project>,
    choices: TemplateSelection,
    prompt: Prompt,
    task: JoinHandle<()>,
    message: String,
    prompt_area: Rect,
}

impl Drop for Generator {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Generator {
    fn new(start: std::path::PathBuf) -> Self {
        let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let task = tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || gitignore::detect(&start)).await;
            job::dispatch(move |editor, compositor| {
                if !compositor
                    .find::<Generator>()
                    .is_some_and(|g| g.session == session)
                {
                    return;
                }
                let result = result.map_err(anyhow::Error::from).and_then(|r| r);
                let project = match result {
                    Ok(project) => project,
                    Err(error) => {
                        editor.set_error(error.to_string());
                        compositor.remove(ID);
                        return;
                    }
                };
                match open_existing(editor, &project.target) {
                    Ok(true) => {
                        compositor.remove(ID);
                    }
                    Ok(false) => {
                        let generator = compositor.find::<Generator>().unwrap();
                        let detected = project.detected.keys().copied().collect::<Vec<_>>();
                        generator.choices = TemplateSelection::new(&detected);
                        generator.project = Some(project);
                    }
                    Err(error) => {
                        editor.set_error(error.to_string());
                        compositor.remove(ID);
                    }
                }
            })
            .await;
        });
        Self {
            session,
            project: None,
            choices: TemplateSelection::new(&[]),
            prompt: Prompt::new("Search: ".into(), None, |_, _| Vec::new(), |_, _, _| {}),
            task,
            message: String::new(),
            prompt_area: Rect::default(),
        }
    }
}

/// Reuse the document before inspecting disk; never reload somebody's unsaved edits.
fn open_existing(editor: &mut Editor, target: &Path) -> Result<bool> {
    if let Some(id) = editor.document_id_by_path(target) {
        editor.switch(id, Action::Replace);
        editor.set_status("Existing .gitignore buffer; content preserved");
        return Ok(true);
    }
    match fs::symlink_metadata(target) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file(),
                ".gitignore target must be a regular file: {}",
                target.display()
            );
            editor.open(target, Action::Replace)?;
            editor.set_status("Existing .gitignore; content preserved");
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("Cannot inspect {}", target.display())),
    }
}

fn generate_draft(
    editor: &mut Editor,
    project: &Project,
    selected: &std::collections::BTreeSet<&str>,
) -> Result<()> {
    if open_existing(editor, &project.target)? {
        return Ok(());
    }
    let directory = project
        .target
        .parent()
        .context("Missing project directory")?;
    anyhow::ensure!(
        directory.is_dir(),
        "Project directory no longer exists: {}",
        directory.display()
    );
    // Verify readability as well, before creating any document.
    fs::read_dir(directory).with_context(|| format!("Cannot access {}", directory.display()))?;
    let text = gitignore::generate(selected)?;
    let previous = editor.documents().map(|doc| doc.id()).collect::<Vec<_>>();
    let id = editor.open(&project.target, Action::Replace)?;
    if previous.contains(&id) {
        return Ok(());
    }
    // Editor::open may have read a file created after the first check. Keep it intact.
    match fs::symlink_metadata(&project.target) {
        Ok(_) => {
            editor.set_status(".gitignore appeared during generation; no template inserted");
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let (view, doc) = helix_view::current!(editor);
    let transaction = Transaction::insert(doc.text(), &Selection::point(0), text.into())
        .with_selection(Selection::point(0));
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    editor.mode = Mode::Normal;
    editor.set_status(format!(
        "Generated .gitignore from {} templates; review and :w to save",
        selected.len()
    ));
    Ok(())
}

impl Component for Generator {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if matches!(event, Event::Key(key!(Esc) | ctrl!('c'))) {
            return EventResult::Consumed(Some(Box::new(|compositor, _| {
                compositor.remove(ID);
            })));
        }
        if self.project.is_none() {
            return EventResult::Consumed(None);
        }
        match event {
            Event::Key(key!(Tab)) => self.choices.toggle(),
            Event::Key(key!(Down) | ctrl!('n')) => self.choices.move_by(false),
            Event::Key(key!(Up) | ctrl!('p')) => self.choices.move_by(true),
            Event::Key(key!(Enter)) => {
                if self.choices.selected.is_empty() {
                    self.message = "Select at least one template".into();
                } else {
                    match generate_draft(
                        cx.editor,
                        self.project.as_ref().unwrap(),
                        &self.choices.selected,
                    ) {
                        Ok(()) => {
                            return EventResult::Consumed(Some(Box::new(|compositor, _| {
                                compositor.remove(ID);
                            })))
                        }
                        Err(error) => {
                            self.message = error.to_string();
                            cx.editor.set_error(self.message.clone());
                        }
                    }
                }
            }
            Event::Key(_) | Event::Paste(_) => {
                let previous = self.prompt.line().clone();
                self.prompt.handle_event(event, cx);
                if previous != *self.prompt.line() {
                    self.choices.filter(self.prompt.line());
                    self.message.clear();
                }
            }
            _ => (),
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        if area.area() == 0 {
            return;
        }
        let width = area.width.saturating_sub(4).min(110).max(1).min(area.width);
        let height = area
            .height
            .saturating_sub(2)
            .min(30)
            .max(1)
            .min(area.height);
        let panel = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let base = cx.editor.theme.get("ui.popup");
        let text = base.patch(cx.editor.theme.get("ui.text"));
        surface.clear_with(panel, base);
        let block = Block::bordered().title(" Gitignore templates ");
        let inner = block.inner(panel);
        block.render(panel, surface);
        self.prompt_area = Rect::default();
        if inner.width < 20 || inner.height < 8 {
            if inner.area() > 0 {
                surface.set_stringn(
                    inner.x,
                    inner.y,
                    "Resize window; Esc cancels",
                    inner.width as usize,
                    text,
                );
            }
            return;
        }
        let mut line = |row: u16, content: &str, style| {
            surface.set_stringn(
                inner.x,
                inner.y + row,
                &super::ai_preview_text::display(content),
                inner.width as usize,
                style,
            );
        };
        let Some(project) = &self.project else {
            line(0, "Detecting project... Esc cancels", text);
            return;
        };
        line(0, &project.target.display().to_string(), text);
        let summary = if !project.warnings.is_empty() {
            format!("Partial detection: {}", project.warnings.join("; "))
        } else if project.detected.is_empty() {
            "No project type detected; select templates manually".to_owned()
        } else {
            format!(
                "Detected: {}",
                project
                    .detected
                    .keys()
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let summary = if project.is_git {
            summary
        } else {
            format!("No Git repository detected | {summary}")
        };
        line(1, &summary, text);
        let list_height = inner.height.saturating_sub(7) as usize;
        let scroll = self
            .choices
            .cursor
            .saturating_sub(list_height.saturating_sub(1));
        for (row, &index) in self
            .choices
            .filtered
            .iter()
            .enumerate()
            .skip(scroll)
            .take(list_height)
        {
            let template = &gitignore::TEMPLATES[index];
            let label = format!(
                "[{}] {}  {}{}",
                if self.choices.selected.contains(template.id) {
                    'x'
                } else {
                    ' '
                },
                template.name,
                template.category,
                if project.detected.contains_key(template.id) {
                    "  detected"
                } else {
                    ""
                }
            );
            let style = if row == self.choices.cursor {
                base.patch(cx.editor.theme.get("ui.menu.selected"))
            } else {
                text
            };
            line(3 + (row - scroll) as u16, &label, style);
        }
        if self.choices.filtered.is_empty() {
            line(3, "No matching templates", text);
        }
        let detail = self
            .choices
            .current()
            .map(|t| {
                let evidence = project
                    .detected
                    .get(t.id)
                    .map(|v| format!("Detected from: {}", v.join(", ")))
                    .unwrap_or_else(|| "Manual selection".into());
                line(2, &format!("{} | {evidence}", t.name), text);
                t.description.to_owned()
            })
            .unwrap_or_default();
        line(inner.height - 4, &detail, text);
        line(
            inner.height - 3,
            &format!(
                "{} selected | {}",
                self.choices.selected.len(),
                self.message
            ),
            text,
        );
        line(
            inner.height - 2,
            "Tab toggle | Enter generate | Esc cancel",
            text,
        );
        self.prompt_area = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        self.prompt.render(self.prompt_area, surface, cx);
    }

    fn cursor(&self, _area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        if self.project.is_some() && self.prompt_area.area() > 0 {
            self.prompt.cursor(self.prompt_area, editor)
        } else {
            (None, CursorKind::Hidden)
        }
    }
    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
}

#[cfg(test)]
mod tests;
