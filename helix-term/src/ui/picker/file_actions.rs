use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, ensure, Context as _};
use helix_core::{fuzzy::MATCHER, Position};
use helix_view::{
    editor::CloseError,
    graphics::{CursorKind, Rect},
    Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Widget},
};

use super::{inject_nucleo_item, Focus};
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl, key,
    ui::{
        self,
        overlay::{overlaid, Overlay},
        FileExplorer, Prompt,
    },
};

mod move_to;

pub(super) struct ExplorerActions<T: 'static + Send + Sync, D: 'static> {
    pub(super) path: fn(&T) -> Option<&Path>,
    pub(super) open: fn(&super::Picker<T, D>, &BTreeSet<PathBuf>) -> EventResult,
    pub(super) marked: BTreeSet<PathBuf>,
}

impl FileExplorer {
    fn matches_entry(&self, entry: &(PathBuf, bool), matcher: &mut nucleo::Matcher) -> bool {
        let text = self.columns[0].format_text(entry, &self.editor_data);
        self.matcher
            .pattern
            .score(&[text.into_owned().into()], matcher)
            .is_some()
    }

    pub(in crate::ui) fn with_file_actions(mut self) -> Self {
        self.action_menu = Some(Box::new(ExplorerActions {
            path: |(path, _)| path.file_name().map(|_| path.as_path()),
            marked: BTreeSet::new(),
            open: |picker, marked| {
                let root = picker.editor_data.0.clone();
                // The synthetic parent entry has no file name.
                let selected = picker
                    .selection()
                    .filter(|(path, _)| path.file_name().is_some())
                    .cloned();
                let marked = marked.iter().cloned().collect();
                EventResult::Consumed(Some(Box::new(move |compositor, _| {
                    compositor.push(Box::new(overlaid(FileActions {
                        root,
                        selected,
                        marked,
                        state: State::Menu,
                    })));
                })))
            },
        }));
        self
    }

    fn refresh_entries(&mut self, target: Option<PathBuf>, editor: &Editor) -> io::Result<()> {
        let mut entries = ui::directory_content(&self.editor_data.0, editor)?;
        // A freshly created dotfile or a directory inside a flattened row
        // can be absent from the normal directory listing.
        if let Some(path) = &target {
            if !entries.iter().any(|(p, _)| p == path) {
                entries.push((path.clone(), path.is_dir()));
            }
        } else if !self.prompt.line().is_empty() {
            let mut matcher = MATCHER.lock();
            if !entries
                .iter()
                .any(|entry| self.matches_entry(entry, &mut matcher))
            {
                // Deleting the final match reveals its neighbour in directory order.
                let row = self.selection().map_or(0, |(path, dir)| {
                    entries.partition_point(|(p, d)| {
                        p.file_name().is_none() || (!*d, p) < (!*dir, path)
                    })
                });
                self.prompt.clear(editor);
                self.handle_prompt_change(false);
                self.cursor = row as u32;
            }
        }
        self.matcher.restart(true);
        self.preview_cache.clear();
        self.pending_focus = Some(Focus::Row(self.cursor));
        let injector = self.matcher.injector();
        for entry in entries {
            let focus = target.as_ref() == Some(&entry.0);
            if focus && !self.matches_entry(&entry, &mut MATCHER.lock()) {
                self.prompt.clear(editor);
                self.handle_prompt_change(false);
            }
            let index = inject_nucleo_item(&injector, &self.columns, entry, &self.editor_data);
            if focus {
                self.pending_focus = Some(Focus::Entry(index));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Operation {
    NewFile,
    NewDirectory,
    Rename,
}

impl Operation {
    fn label(self) -> &'static str {
        match self {
            Self::NewFile => "New file",
            Self::NewDirectory => "New directory",
            Self::Rename => "Rename",
        }
    }
}

enum State {
    Menu,
    Name(Operation, Box<Prompt>),
    Delete,
}

struct FileActions {
    root: PathBuf,
    selected: Option<(PathBuf, bool)>,
    marked: Vec<PathBuf>,
    state: State,
}

impl FileActions {
    fn selected_path(&self) -> anyhow::Result<&Path> {
        self.selected
            .as_ref()
            .map(|(p, _)| p.as_path())
            .context("Select a file or directory first")
    }

    fn start_name(&mut self, op: Operation, editor: &Editor) -> anyhow::Result<()> {
        let mut prompt = Prompt::new("".into(), None, ui::completers::none, |_, _, _| {});
        if op == Operation::Rename {
            let path = self.selected_path()?;
            let name = path
                .file_name()
                .and_then(OsStr::to_str)
                .context("The file name is not valid UTF-8")?;
            let stem = if path.is_dir() {
                name
            } else {
                path.file_stem().and_then(OsStr::to_str).unwrap_or(name)
            };
            prompt.set_line(name.to_owned(), editor);
            prompt.select_prefix(stem.len());
        }
        self.state = State::Name(op, Box::new(prompt));
        Ok(())
    }

    fn submit_name(&self, op: Operation, name: &str, cx: &mut Context) -> anyhow::Result<PathBuf> {
        ensure!(
            !name.trim().is_empty()
                && !name.chars().any(char::is_control)
                && Path::new(name).file_name() == Some(OsStr::new(name)),
            "Enter one file or directory name"
        );
        #[cfg(windows)]
        ensure!(
            !name.contains([':', '<', '>', '"', '|', '?', '*']) && !name.ends_with([' ', '.']),
            "Invalid Windows file name"
        );
        cx.block_try_flush_writes()?;
        let parent = if op == Operation::Rename {
            self.selected_path()?
                .parent()
                .context("No parent directory")?
        } else {
            &self.root
        };
        let target = parent.join(name);
        if op == Operation::Rename && self.selected_path()? == target {
            fs::symlink_metadata(&target)?;
            return Ok(target);
        }
        match fs::symlink_metadata(&target) {
            #[cfg(windows)]
            Ok(_)
                if op == Operation::Rename
                    && helix_stdx::path::eq_ignore_case(self.selected_path()?, &target) =>
            {
                ()
            }
            Ok(_) => bail!("Already exists: {}", target.display()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => (),
            Err(err) => return Err(err.into()),
        }
        // A buffer may already own this name even before its first save.
        let occupied = cx.editor.documents().filter_map(|doc| doc.path()).any(|p| {
            if op == Operation::Rename
                && self
                    .selected
                    .as_ref()
                    .is_some_and(|(src, _)| p.starts_with(src))
            {
                return false;
            }
            p.ancestors().any(|ancestor| {
                #[cfg(windows)]
                {
                    helix_stdx::path::eq_ignore_case(ancestor, &target)
                }
                #[cfg(not(windows))]
                {
                    ancestor == target
                }
            })
        });
        ensure!(!occupied, "An open buffer uses {}", target.display());
        if op == Operation::Rename {
            cx.editor
                .move_path_noreplace(self.selected_path()?, &target)?;
        } else {
            cx.editor
                .create_new_path(&target, op == Operation::NewDirectory)?;
        }
        Ok(target)
    }

    fn validate_delete(editor: &Editor, path: &Path) -> anyhow::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            ensure!(
                fs::read_dir(path)?.next().transpose()?.is_none(),
                "Directory is not empty: {}",
                path.display()
            );
        }
        for doc in editor.documents() {
            let Some(buffer) = doc.path() else { continue };
            if buffer == path {
                ensure!(
                    !doc.is_modified(),
                    "Save or discard unsaved changes before deleting {}",
                    path.display()
                );
                ensure!(
                    doc.merge().is_none() && editor.active_merge().is_none(),
                    "Cannot delete an open file during a merge"
                );
            } else if buffer.starts_with(path) {
                bail!("Directory contains an open buffer: {}", buffer.display());
            }
        }
        Ok(())
    }

    fn delete(&self, cx: &mut Context) -> anyhow::Result<()> {
        cx.block_try_flush_writes()?;
        let path = self.selected_path()?;
        cx.editor.delete_path_checked(path, false, |editor, path| {
            Self::validate_delete(editor, path).map_err(io::Error::other)
        })?;
        if let Some(id) = cx.editor.document_by_path(path).map(|doc| doc.id()) {
            cx.editor
                .close_document(id, false)
                .map_err(|err| match err {
                    CloseError::SaveError(err) => err,
                    CloseError::BufferModified(name) => anyhow!("Buffer modified: {name}"),
                    CloseError::DoesNotExist => anyhow!("Buffer already closed"),
                })?;
        }
        Ok(())
    }

    fn finish(target: Option<PathBuf>) -> EventResult {
        EventResult::Consumed(Some(Box::new(move |compositor, cx| {
            compositor.pop();
            if let Some(explorer) = compositor.find::<Overlay<FileExplorer>>() {
                if let Err(err) = explorer.content.refresh_entries(target, cx.editor) {
                    cx.editor
                        .set_error(format!("File operation succeeded, refresh failed: {err}"));
                }
            }
        })))
    }

    fn area(&self, area: Rect) -> Rect {
        let height = if matches!(self.state, State::Menu) {
            8 + u16::from(!self.marked.is_empty())
        } else {
            5
        };
        area.clip_top(area.height.saturating_sub(height))
            .with_width(area.width.min(80))
    }
}

impl Component for FileActions {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let close = || {
            EventResult::Consumed(Some(Box::new(|compositor, _| {
                compositor.pop();
            })))
        };
        if matches!(event, Event::Key(key!(Esc) | ctrl!('c'))) {
            return close();
        }
        let result = match (&mut self.state, event) {
            (State::Menu, Event::Key(key!('n'))) => self.start_name(Operation::NewFile, cx.editor),
            (State::Menu, Event::Key(key!('m'))) => {
                self.start_name(Operation::NewDirectory, cx.editor)
            }
            (State::Menu, Event::Key(key!('r')))
                if self.selected.is_some() && self.marked.is_empty() =>
            {
                self.start_name(Operation::Rename, cx.editor)
            }
            (State::Menu, Event::Key(key!('v'))) => {
                let sources = if self.marked.is_empty() {
                    let Some((source, _)) = &self.selected else {
                        return EventResult::Consumed(None);
                    };
                    vec![source.clone()]
                } else {
                    self.marked.clone()
                };
                match move_to::MoveTo::new(sources.into(), &self.root, cx.editor) {
                    Ok(picker) => {
                        return EventResult::Consumed(Some(Box::new(move |compositor, _| {
                            compositor.pop();
                            compositor.push(Box::new(overlaid(picker)));
                        })))
                    }
                    Err(err) => Err(err.into()),
                }
            }
            (State::Menu, Event::Key(key!('c'))) if !self.marked.is_empty() => {
                return EventResult::Consumed(Some(Box::new(|compositor, _| {
                    compositor.pop();
                    if let Some(explorer) = compositor.top_mut::<Overlay<FileExplorer>>() {
                        explorer
                            .content
                            .action_menu
                            .as_mut()
                            .unwrap()
                            .marked
                            .clear();
                    }
                })));
            }
            (State::Menu, Event::Key(key!('d')))
                if self.selected.is_some() && self.marked.is_empty() =>
            {
                self.state = State::Delete;
                Ok(())
            }
            (State::Name(op, prompt), Event::Key(key!(Enter))) => {
                let (op, name) = (*op, prompt.line().clone());
                match self.submit_name(op, &name, cx) {
                    Ok(target) => return Self::finish(Some(target)),
                    Err(err) => Err(err),
                }
            }
            (State::Name(_, prompt), _) => return prompt.handle_event(event, cx),
            (State::Delete, Event::Key(key!('y'))) => match self.delete(cx) {
                Ok(()) => return Self::finish(None),
                Err(err) => Err(err),
            },
            (State::Delete, Event::Key(key!(Enter) | key!('n'))) => return close(),
            _ => Ok(()),
        };
        if let Err(err) = result {
            cx.editor.set_error(err.to_string());
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        let area = self.area(area);
        let normal = cx.editor.theme.get("ui.menu");
        let inactive = cx.editor.theme.get("ui.text.inactive");
        surface.clear_with(area, normal);
        let block = Block::bordered();
        let inner = block.inner(area).clip_left(1).clip_right(1);
        block.render(area, surface);
        if inner.height == 0 || inner.width < 3 {
            return;
        }
        let mut line = |n: u16, text: &str, muted: bool| {
            if n < inner.height {
                surface.set_string_truncated(
                    inner.x,
                    inner.y + n,
                    text,
                    inner.width as usize,
                    |_| if muted { inactive } else { normal },
                    true,
                    n == 1,
                );
            }
        };
        match &mut self.state {
            State::Menu => {
                line(0, "File actions", false);
                line(1, "n  New file", false);
                line(2, "m  New directory", false);
                let single = self.selected.is_some() && self.marked.is_empty();
                line(3, "r  Rename", !single);
                line(4, "d  Delete", !single);
                if self.marked.is_empty() {
                    line(5, "v  Move to...", !single);
                } else {
                    line(
                        5,
                        &format!("v  Move {} marked to...", self.marked.len()),
                        false,
                    );
                    line(6, "c  Clear marks", false);
                }
            }
            State::Name(op, prompt) => {
                line(0, op.label(), false);
                let path = if *op == Operation::Rename {
                    self.selected
                        .as_ref()
                        .map(|(p, _)| p.as_path())
                        .unwrap_or(&self.root)
                } else {
                    &self.root
                };
                line(1, &path.display().to_string(), false);
                prompt.render(inner.clip_top(inner.height - 1), surface, cx);
            }
            State::Delete => {
                line(0, "Permanently delete?", false);
                if let Some((path, _)) = &self.selected {
                    line(
                        1,
                        &path
                            .strip_prefix(&self.root)
                            .unwrap_or(path)
                            .display()
                            .to_string(),
                        false,
                    );
                }
                line(2, "[Enter] Cancel    [y] Delete", false);
            }
        }
    }

    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        let inner = Block::bordered()
            .inner(self.area(area))
            .clip_left(1)
            .clip_right(1);
        if let State::Name(_, prompt) = &self.state {
            if inner.height > 0 && inner.width >= 3 {
                return prompt.cursor(inner.clip_top(inner.height - 1), editor);
            }
        }
        (
            Some(Position::new(inner.y as usize, inner.x as usize)),
            CursorKind::Hidden,
        )
    }
}
