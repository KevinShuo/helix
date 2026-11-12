//! Repository conflict list. Reads belong to this instance; confirmed writes belong to Jobs.
use super::{merge, trusted, NEXT_INSTANCE};
use crate::{
    commands,
    compositor::{Component, Compositor, Context, Event, EventResult},
    job::{self, Callback, Job},
};
use helix_core::Position;
use helix_vcs::git_tools::{GitConflictSnapshot, GitRepository, MergeAction, MergeActionPlan};
use helix_view::{
    graphics::{CursorKind, Rect},
    input::{KeyCode, MouseButton, MouseEventKind},
    merge::{MergeLeave, MergeSessionId, MergeWriteStatus},
    Editor,
};
use std::{path::PathBuf, sync::atomic::Ordering};
use tokio::task::JoinHandle;
use tui::buffer::Buffer;

pub(crate) const ID: &str = "git-conflicts";

struct FileRequest {
    path: PathBuf,
    discard: bool,
    expected: Option<(MergeSessionId, i32)>,
}

pub(crate) struct ConflictList {
    instance: u64,
    version: u64,
    root: PathBuf,
    repository: Option<GitRepository>,
    snapshot: GitConflictSnapshot,
    rows: Vec<(PathBuf, String)>,
    selected: usize,
    scroll: usize,
    visible: bool,
    task: Option<JoinHandle<()>>,
    pending_file: Option<FileRequest>,
    message: String,
    confirmation: Option<MergeActionPlan>,
    confirm_scroll: usize,
    area: Rect,
}
impl Drop for ConflictList {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub(crate) fn root(editor: &Editor) -> PathBuf {
    editor
        .git_context_path()
        .unwrap_or_else(helix_stdx::env::current_working_dir)
}
pub(crate) fn open(cx: &mut commands::Context) {
    let path = root(cx.editor);
    if !trusted(cx.editor, &path) {
        cx.editor
            .set_error("Git tools require a trusted workspace (:trust)");
        return;
    }
    if let Err(error) = cx.editor.leave_merge(MergeLeave::KeepDraft) {
        cx.editor.set_error(error.to_string());
        return;
    }
    cx.callback.push(Box::new(move |compositor, cx| {
        present(compositor, cx.editor, path)
    }));
}
/// Caller must validate its own operation/panel identity before presenting an automatic result.
pub(crate) fn present(compositor: &mut Compositor, editor: &mut Editor, path: PathBuf) {
    if !trusted(editor, &path) {
        editor.set_error("Git tools require a trusted workspace (:trust)");
        return;
    }
    if let Some(list) = compositor
        .find::<ConflictList>()
        .filter(|list| list.root == path)
    {
        list.visible = true;
        list.refresh();
    } else {
        compositor.remove(ID);
        let mut list = ConflictList {
            instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            version: 0,
            root: path,
            repository: None,
            snapshot: Default::default(),
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            visible: true,
            task: None,
            pending_file: None,
            message: String::new(),
            confirmation: None,
            confirm_scroll: 0,
            area: Rect::default(),
        };
        list.refresh();
        compositor.push(Box::new(list));
    }
}
/// Bind a confirmed refresh to its result document's worktree, even after
/// native buffer switching has resumed a draft from a different repository.
pub(super) fn refresh_draft(
    compositor: &mut Compositor,
    editor: &mut Editor,
    root: PathBuf,
    path: PathBuf,
    expected: (MergeSessionId, i32),
) {
    if !trusted(editor, &root) {
        return;
    }
    present(compositor, editor, root.clone());
    if let Some(list) = compositor
        .find::<ConflictList>()
        .filter(|list| list.root == root)
    {
        list.request_file(path, true, Some(expected));
    }
}

impl ConflictList {
    pub(super) fn refresh_for(&mut self, root: &std::path::Path) {
        if self.root == root {
            self.refresh();
        }
    }
    pub(super) fn refresh(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.version += 1;
        self.confirmation = None;
        self.message = "Loading conflicts...".into();
        let (instance, version, path) = (self.instance, self.version, self.root.clone());
        self.task = Some(tokio::spawn(async move {
            let result = async {
                let repository = GitRepository::open(path.clone(), true).await?;
                let snapshot = repository.conflicts().await?;
                let mut rows = Vec::new();
                let mut versions = Vec::new();
                for file in &snapshot.files {
                    let state = match repository.conflict_file(&file.path).await {
                        Ok(data) => {
                            versions.push((file.path.clone(), data.version));
                            data.support
                                .reason()
                                .map(|reason| format!("External: {reason}"))
                                .unwrap_or_else(|| "Unresolved".into())
                        }
                        Err(error) => format!("External: {error}"),
                    };
                    rows.push((file.path.clone(), format!("{}  {state}", file.status)));
                }
                Ok::<_, anyhow::Error>((repository, snapshot, rows, versions))
            }
            .await;
            job::dispatch(move |editor, compositor| {
                let Some(list) = compositor.find::<ConflictList>().filter(|list| {
                    list.instance == instance && list.version == version && list.root == path
                }) else {
                    return;
                };
                list.task = None;
                match result {
                    Ok((repository, snapshot, mut rows, versions)) => {
                        list.root = repository.root().to_owned();
                        for (_, id) in editor.merge_sessions.iter() {
                            let Some(state) = editor.documents[&id]
                                .merge()
                                .filter(|s| s.data.version.root() == repository.root())
                            else {
                                continue;
                            };
                            let status = match &state.write_status {
                                MergeWriteStatus::Staged
                                    if state.data.operation == snapshot.operation
                                        && !snapshot
                                            .files
                                            .iter()
                                            .any(|file| file.path == state.data.file.path) =>
                                {
                                    "Staged"
                                }
                                MergeWriteStatus::Staged => "Stale draft",
                                MergeWriteStatus::NeedsRefresh(_) => "Stale draft",
                                MergeWriteStatus::Retry { .. }
                                    if state.data.operation == snapshot.operation
                                        && snapshot
                                            .files
                                            .iter()
                                            .any(|file| *file == state.data.file) =>
                                {
                                    "Written, not staged"
                                }
                                MergeWriteStatus::Retry { .. } => "Stale draft",
                                _ if versions
                                    .iter()
                                    .find(|(p, _)| *p == state.data.file.path)
                                    .is_none_or(|(_, version)| *version != state.data.version) =>
                                {
                                    "Stale draft"
                                }
                                _ => "Editing draft",
                            };
                            if let Some(row) = rows
                                .iter_mut()
                                .find(|(path, _)| *path == state.data.file.path)
                            {
                                row.1 = status.into();
                            } else {
                                rows.push((
                                    state.data.file.path.clone(),
                                    if matches!(state.write_status, MergeWriteStatus::Staged) {
                                        status.into()
                                    } else {
                                        "Stale draft (resolved externally)".into()
                                    },
                                ));
                            }
                        }
                        rows.sort_by(|a, b| a.0.cmp(&b.0));
                        list.repository = Some(repository);
                        list.snapshot = snapshot;
                        list.rows = rows;
                        list.selected = list.selected.min(list.rows.len().saturating_sub(1));
                        list.message = if list.snapshot.operation.is_merge() {
                            "Enter resolve · r refresh · f Finish Merge · a Abort Merge · q close"
                        } else {
                            "Enter inspect · r refresh · external resolution required · q close"
                        }
                        .into();
                        if let Some(request) = list.pending_file.take() {
                            list.request_file(request.path, request.discard, request.expected);
                        }
                    }
                    Err(error) => {
                        list.message = format!("{error:#}");
                        editor.set_error(list.message.clone());
                    }
                }
            })
            .await;
        }));
    }
    pub(super) fn request_file(
        &mut self,
        path: PathBuf,
        discard: bool,
        expected: Option<(helix_view::merge::MergeSessionId, i32)>,
    ) {
        let Some(repository) = self.repository.clone() else {
            self.pending_file = Some(FileRequest {
                path,
                discard,
                expected,
            });
            return;
        };
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.version += 1;
        let (instance, version, root) = (self.instance, self.version, self.root.clone());
        self.message = "Loading merge sources...".into();
        self.task = Some(tokio::spawn(async move {
            let result = async {
                let data = repository.conflict_file(&path).await?;
                if let Some(reason) = data.support.reason() {
                    anyhow::bail!("{reason}");
                }
                let candidate = repository.merge_candidate(&data).await?;
                Ok::<_, anyhow::Error>((data, candidate))
            }
            .await;
            job::dispatch(move |editor, compositor| {
                let Some(list) = compositor.find::<ConflictList>().filter(|l| l.instance == instance && l.version == version && l.root == root) else { return; };
                list.task = None;
                if let Some((session, document_version)) = expected {
                    if editor.active_merge() != Some(session) || editor.merge_sessions.document(session).is_none_or(|id| {
                        editor.documents.get(&id).is_none_or(|doc| doc.version() != document_version
                            || doc.merge().is_none_or(|state| state.data.version.root() != root || state.data.file.path != path))
                    }) { return; }
                } else if !list.visible || editor.active_merge().is_some() { return; }
                let existing = editor.merge_sessions.find(&root, &path);
                if discard && result.is_ok() {
                    if let Some(id) = existing { if let Err(error) = editor.discard_merge(id) { editor.set_error(error.to_string()); return; } }
                } else if !discard {
                    if let Some(id) = existing {
                    let document = editor.merge_sessions.document(id).unwrap();
                    let state = editor.documents.get_mut(&document).unwrap().merge_mut().unwrap();
                    let stale = match &state.write_status {
                        MergeWriteStatus::Staged => result.is_ok() || state.data.operation != list.snapshot.operation,
                        MergeWriteStatus::Retry { .. } => result.as_ref().map_or(true, |(data, _)| data.operation != state.data.operation || data.file != state.data.file),
                        _ => result.as_ref().map_or(true, |(data, _)| data.version != state.data.version),
                    };
                    if stale {
                        state.write_status = MergeWriteStatus::NeedsRefresh("Sources or working file changed; draft preserved. Refresh requires confirmation.".into());
                    }
                    match editor.resume_merge(id) {
                        Ok(()) => { list.visible = false; merge::install(compositor, id); },
                        Err(error) => editor.set_error(error.to_string()),
                    }
                    return;
                    }
                }
                match result {
                    Ok((data, candidate)) => match editor.enter_merge(data, candidate) {
                        Ok(id) => { list.visible = false; merge::install(compositor, id); },
                        Err(error) => editor.set_error(error.to_string()),
                    },
                    Err(error) => { list.message = format!("{error:#}"); editor.set_error(list.message.clone()); }
                }
            }).await;
        }));
    }
    fn prepare(&mut self, action: MergeAction, editor: &mut Editor) {
        if !self.snapshot.operation.is_merge() {
            editor.set_error(
                "This operation requires an external resolver; Finish/Abort Merge are unavailable",
            );
            return;
        }
        if !trusted(editor, &self.root) {
            editor.set_error("Git tools require a trusted workspace (:trust)");
            return;
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        if action == MergeAction::Abort
            && editor.documents().any(|doc| {
                doc.is_modified()
                    || doc.merge().is_some_and(|m| {
                        m.data.version.root() == repository.root()
                            && !matches!(m.write_status, MergeWriteStatus::Staged)
                    })
            })
        {
            editor.set_error(
                "Save ordinary buffers and explicitly discard merge drafts before Abort Merge",
            );
            return;
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.version += 1;
        let (instance, version, root) = (self.instance, self.version, self.root.clone());
        self.task = Some(tokio::spawn(async move {
            let result = repository.prepare_merge_action(action).await;
            job::dispatch(move |editor, compositor| {
                let Some(list) = compositor.find::<ConflictList>().filter(|l| {
                    l.instance == instance && l.version == version && l.root == root && l.visible
                }) else {
                    return;
                };
                list.task = None;
                match result {
                    Ok(plan) => {
                        list.confirmation = Some(plan);
                        list.confirm_scroll = 0;
                    }
                    Err(error) => editor.set_error(format!("{error:#}")),
                }
            })
            .await;
        }));
    }
    fn execute(&mut self, cx: &mut Context) {
        let Some(plan) = self.confirmation.take() else {
            return;
        };
        let Some(repository) = self.repository.clone() else {
            return;
        };
        if !trusted(cx.editor, repository.root()) {
            cx.editor
                .set_error("Git tools require a trusted workspace (:trust)");
            return;
        }
        if plan.action() == MergeAction::Abort
            && cx.editor.documents().any(|doc| {
                doc.is_modified()
                    || doc.merge().is_some_and(|m| {
                        m.data.version.root() == repository.root()
                            && !matches!(m.write_status, MergeWriteStatus::Staged)
                    })
            })
        {
            cx.editor
                .set_error("Drafts or unsaved buffers prevent Abort Merge");
            return;
        }
        let instance = self.instance;
        self.message = "Running confirmed merge operation...".into();
        cx.jobs.add(
            Job::with_callback(async move {
                let result = repository.execute_merge_action(&plan).await;
                Ok(Callback::Editor(Box::new(move |editor| {
                    match result {
                        Ok(_) => {
                            editor.set_status("Merge operation completed");
                            merge::reload_clean(editor, repository.root(), None);
                        }
                        Err(error) => editor.set_error(format!("{error:#}")),
                    }
                    job::dispatch_blocking(move |_, compositor| {
                        if let Some(list) = compositor
                            .find::<ConflictList>()
                            .filter(|l| l.instance == instance)
                        {
                            list.refresh();
                        }
                    });
                })))
            })
            .wait_before_exiting(),
        );
    }
}
impl Component for ConflictList {
    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
    fn render(&mut self, area: Rect, frame: &mut Buffer, cx: &mut Context) {
        if !self.visible || cx.editor.active_merge().is_some() {
            return;
        }
        self.area = area.clip_bottom(1);
        let area = self.area;
        frame.clear_with(area, cx.editor.theme.get("ui.background"));
        let style = cx.editor.theme.get("ui.text");
        merge::render::line(
            frame,
            area,
            0,
            &format!(
                "Conflicts ({}) · {:?}",
                self.snapshot.files.len(),
                self.snapshot.operation
            ),
            style,
        );
        if let Some(plan) = &self.confirmation {
            let mut lines = vec![
                format!(
                    "Confirm {:?}: y confirm · n cancel · j/k scroll",
                    plan.action()
                ),
                format!("HEAD: {}", plan.head()),
                format!("Parents: {}", plan.parents().join(", ")),
                "All staged paths (including non-conflict files):".into(),
            ];
            lines.extend(plan.staged_paths().iter().map(|p| p.display().to_string()));
            lines.push("Default merge message:".into());
            lines.extend(plan.default_message().lines().map(ToOwned::to_owned));
            if plan.action() == MergeAction::Abort {
                lines.push(
                    "Abort may not restore pre-merge uncommitted changes. No hard reset fallback."
                        .into(),
                );
            }
            self.confirm_scroll = self.confirm_scroll.min(lines.len().saturating_sub(1));
            for (row, text) in lines.iter().skip(self.confirm_scroll).enumerate() {
                merge::render::line(frame, area, row + 1, text, style);
            }
            return;
        }
        let height = area.height.saturating_sub(3) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected >= self.scroll + height {
            self.scroll = self.selected.saturating_sub(height.saturating_sub(1));
        }
        for (row, (path, state)) in self.rows.iter().skip(self.scroll).take(height).enumerate() {
            let selected = self.scroll + row == self.selected;
            merge::render::line(
                frame,
                area,
                row + 2,
                &format!(
                    "{} {}  [{state}]",
                    if selected { ">" } else { " " },
                    path.display()
                ),
                if selected {
                    cx.editor.theme.get("ui.selection")
                } else {
                    style
                },
            );
        }
        merge::render::line(
            frame,
            area,
            area.height.saturating_sub(1) as usize,
            &self.message,
            style,
        );
    }
    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        (
            (self.visible && editor.active_merge().is_none())
                .then_some(Position::new(area.y as usize, area.x as usize)),
            CursorKind::Hidden,
        )
    }
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if !self.visible || cx.editor.active_merge().is_some() {
            return EventResult::Ignored(None);
        }
        if matches!(event, Event::FocusGained | Event::FocusLost) {
            return EventResult::Ignored(None);
        }
        let code = match event {
            Event::Key(key) => key.code,
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.selected = (self.scroll + mouse.row.saturating_sub(self.area.y + 2) as usize)
                    .min(self.rows.len().saturating_sub(1));
                KeyCode::Enter
            }
            _ => return EventResult::Consumed(None),
        };
        if self.confirmation.is_some() {
            match code {
                KeyCode::Char('y') => self.execute(cx),
                KeyCode::Char('j') | KeyCode::Down => self.confirm_scroll += 1,
                KeyCode::Char('k') | KeyCode::Up => {
                    self.confirm_scroll = self.confirm_scroll.saturating_sub(1)
                }
                KeyCode::Char('n') | KeyCode::Esc => self.confirmation = None,
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                return EventResult::Consumed(Some(Box::new(|compositor, _| {
                    compositor.remove(ID);
                })))
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1))
            }
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('r') | KeyCode::F(5) => {
                if trusted(cx.editor, &self.root) {
                    self.refresh();
                } else {
                    cx.editor
                        .set_error("Git tools require a trusted workspace (:trust)");
                }
            }
            KeyCode::Char('f') => self.prepare(MergeAction::Finish, cx.editor),
            KeyCode::Char('a') => self.prepare(MergeAction::Abort, cx.editor),
            KeyCode::Enter => {
                if !trusted(cx.editor, &self.root) {
                    cx.editor
                        .set_error("Git tools require a trusted workspace (:trust)");
                    return EventResult::Consumed(None);
                }
                if let Some((path, _)) = self.rows.get(self.selected) {
                    self.request_file(path.clone(), false, None);
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }
}

/// Present a structured, already-read snapshot after the caller validates its
/// triggering operation. Does not issue another query or disturb GitPanel state.
pub(crate) fn present_snapshot(
    compositor: &mut Compositor,
    editor: &mut Editor,
    repository: GitRepository,
    snapshot: GitConflictSnapshot,
) {
    if !trusted(editor, repository.root()) {
        return;
    }
    compositor.remove(ID);
    let rows = snapshot
        .files
        .iter()
        .map(|file| (file.path.clone(), format!("{}  Unresolved", file.status)))
        .collect();
    let list = ConflictList {
        instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
        version: 0,
        root: repository.root().to_owned(),
        repository: Some(repository),
        snapshot,
        rows,
        selected: 0,
        scroll: 0,
        visible: true,
        task: None,
        pending_file: None,
        message: "Enter resolve · r refresh · f Finish Merge · a Abort Merge · q close".into(),
        confirmation: None,
        confirm_scroll: 0,
        area: Rect::default(),
    };
    compositor.push(Box::new(list));
}
