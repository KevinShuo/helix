mod actions;
pub(crate) mod commit;
pub(crate) mod conflicts;
pub(crate) mod merge;
mod render;

use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use helix_core::Position;
use helix_vcs::git_tools::{
    GitDetails, GitFile, GitLogFilter, GitOperation, GitRefKind, GitReference, GitRepository,
    GitSnapshot, GitTarget,
};
use helix_view::{
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    Editor,
};
use tokio::task::{AbortHandle, JoinHandle};
use tui::buffer::Buffer;

use crate::{
    compositor::{Component, Context, Event, EventResult},
    job,
};

pub(super) const ID: &str = "git-tools";
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub(crate) enum OpenAction {
    Conflicts,
    Log,
    Branches,
    Status,
    FileHistory(PathBuf),
    NewBranch,
    Fetch,
    Pull,
    Push,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Branches,
    Commits,
    Files,
    Diff,
}

impl Pane {
    fn next(self, backwards: bool) -> Self {
        let panes = [Self::Branches, Self::Commits, Self::Files, Self::Diff];
        let index = panes.iter().position(|p| *p == self).unwrap();
        panes[(index + if backwards { 3 } else { 1 }) % 4]
    }
}

#[derive(Clone, Debug)]
struct BranchRow {
    label: String,
    reference: Option<usize>,
    group: Option<String>,
    head: bool,
}

#[derive(Clone, Copy)]
enum FileRow {
    Conflicts(usize),
    Changes(usize),
    File(usize),
}

#[derive(Clone, Copy, Default)]
struct PanelLayout {
    panel: Rect,
    branches: Rect,
    commits: Rect,
    files: Rect,
    diff: Rect,
}

pub(crate) struct GitPanel {
    instance: u64,
    path: PathBuf,
    repository: Option<GitRepository>,
    snapshot: GitSnapshot,
    filter: GitLogFilter,
    details: GitDetails,
    patch: String,
    target: GitTarget,
    focus: Pane,
    focused: bool,
    suspended: bool,
    fullscreen: bool,
    selected_branch: usize,
    branch_query: String,
    selected_commit: usize,
    selected_file: usize,
    branch_scroll: usize,
    commit_scroll: usize,
    file_scroll: usize,
    diff_scroll: usize,
    diff_horizontal: usize,
    collapsed: BTreeSet<String>,
    load_version: u64,
    detail_version: u64,
    patch_version: u64,
    snapshot_task: Option<JoinHandle<()>>,
    repository_task: Option<JoinHandle<()>>,
    resolving_repository: bool,
    plan_task: Option<JoinHandle<()>>,
    plan_version: u64,
    detail_task: Option<JoinHandle<()>>,
    patch_task: Option<JoinHandle<()>>,
    operation_task: Option<AbortHandle>,
    operation_version: u64,
    pending_action: Option<OpenAction>,
    message: String,
    busy: bool,
    action_menu: bool,
    confirmation: Option<GitOperation>,
    last_layout: PanelLayout,
}

impl Drop for GitPanel {
    fn drop(&mut self) {
        for task in [
            &self.repository_task,
            &self.plan_task,
            &self.snapshot_task,
            &self.detail_task,
            &self.patch_task,
        ]
        .into_iter()
        .flatten()
        {
            task.abort();
        }
        // An explicitly confirmed operation must finish even if the user closes
        // its window. Its result is still reported through the editor job queue.
    }
}

pub(crate) fn open(cx: &mut crate::commands::Context, action: OpenAction) {
    if matches!(action, OpenAction::Conflicts) {
        conflicts::open(cx);
        return;
    }
    let Some(path) = cx.editor.git_context_path() else {
        return;
    };
    if let Err(error) = cx
        .editor
        .leave_merge(helix_view::merge::MergeLeave::KeepDraft)
    {
        cx.editor.set_error(error.to_string());
        return;
    }
    if !trusted(cx.editor, &path) {
        cx.editor
            .set_error("Git tools require a trusted workspace (:trust)");
        return;
    }
    cx.callback.push(Box::new(move |compositor, _cx| {
        if let Some(panel) = compositor.find::<GitPanel>() {
            panel.resolve_open(path, action);
        } else {
            let mut panel = GitPanel::new(path.clone(), action.clone());
            panel.resolve_open(path, action);
            compositor.push(Box::new(panel));
        }
    }));
}

pub(crate) fn trusted(editor: &Editor, path: &std::path::Path) -> bool {
    editor
        .workspace_trust
        .query(
            &helix_loader::find_workspace_in(path).0,
            helix_loader::workspace_trust::TrustQuery::Git,
        )
        .is_trusted()
}

impl GitPanel {
    fn is_conflict(&self, file: &GitFile) -> bool {
        matches!(self.target, GitTarget::WorkingTree)
            && matches!(
                file.status.as_str(),
                "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
            )
    }

    fn file_rows(&self) -> Vec<FileRow> {
        let conflicts = self
            .details
            .files
            .iter()
            .filter(|file| self.is_conflict(file))
            .count();
        if conflicts == 0 {
            return (0..self.details.files.len()).map(FileRow::File).collect();
        }
        let mut rows = vec![FileRow::Conflicts(conflicts)];
        rows.extend(
            self.details
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| self.is_conflict(file))
                .map(|(i, _)| FileRow::File(i)),
        );
        let changes = self.details.files.len() - conflicts;
        if changes > 0 {
            rows.push(FileRow::Changes(changes));
            rows.extend(
                self.details
                    .files
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| !self.is_conflict(file))
                    .map(|(i, _)| FileRow::File(i)),
            );
        }
        rows
    }

    fn new(path: PathBuf, action: OpenAction) -> Self {
        let mut filter = GitLogFilter::default();
        if let OpenAction::FileHistory(path) = &action {
            filter.path = Some(path.clone());
        }
        Self {
            instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            path,
            repository: None,
            snapshot: GitSnapshot::default(),
            filter,
            details: GitDetails::default(),
            patch: String::new(),
            target: if matches!(action, OpenAction::Status) {
                GitTarget::WorkingTree
            } else {
                GitTarget::Commit(String::new())
            },
            focus: if matches!(action, OpenAction::Branches) {
                Pane::Branches
            } else {
                Pane::Commits
            },
            focused: true,
            suspended: false,
            fullscreen: false,
            selected_branch: 0,
            selected_commit: 0,
            selected_file: 0,
            branch_query: String::new(),
            branch_scroll: 0,
            commit_scroll: 0,
            file_scroll: 0,
            diff_scroll: 0,
            diff_horizontal: 0,
            collapsed: BTreeSet::new(),
            load_version: 0,
            detail_version: 0,
            patch_version: 0,
            snapshot_task: None,
            repository_task: None,
            resolving_repository: false,
            plan_task: None,
            plan_version: 0,
            detail_task: None,
            patch_task: None,
            operation_task: None,
            operation_version: 0,
            pending_action: Some(action),
            message: "Loading Git repository...".into(),
            busy: true,
            action_menu: false,
            confirmation: None,
            last_layout: PanelLayout::default(),
        }
    }

    fn layout(&self, area: Rect) -> PanelLayout {
        let available = area.height.saturating_sub(1);
        let height = if self.fullscreen || area.height < 12 {
            available
        } else {
            (area.height * 3 / 5)
                .max(8)
                .min(area.height.saturating_sub(4))
        };
        let panel = Rect::new(
            area.x,
            area.bottom().saturating_sub(1 + height).max(area.y),
            area.width,
            height,
        );
        let body = panel
            .clip_top(2.min(height))
            .clip_bottom(1.min(height.saturating_sub(2)));
        let empty = Rect::new(body.x, body.y, 0, 0);
        if area.width < 85 || body.height < 6 {
            return PanelLayout {
                panel,
                branches: if self.focus == Pane::Branches {
                    body
                } else {
                    empty
                },
                commits: if self.focus == Pane::Commits {
                    body
                } else {
                    empty
                },
                files: if self.focus == Pane::Files {
                    body
                } else {
                    empty
                },
                diff: if self.focus == Pane::Diff {
                    body
                } else {
                    empty
                },
            };
        }
        let branch_width = (body.width / 5).clamp(18, 32);
        let file_width = (body.width / 4).max(24);
        let right = body.clip_left(branch_width);
        let top_height = (body.height * 2 / 3)
            .max(3)
            .min(body.height.saturating_sub(3));
        PanelLayout {
            panel,
            branches: body.with_width(branch_width),
            commits: right.with_height(top_height).clip_right(file_width),
            files: right
                .with_height(top_height)
                .clip_left(right.width.saturating_sub(file_width)),
            diff: right.clip_top(top_height),
        }
    }

    fn branch_rows(&self) -> Vec<BranchRow> {
        let mut rows = vec![
            BranchRow {
                label: "All branches".into(),
                reference: None,
                group: None,
                head: false,
            },
            BranchRow {
                label: format!("HEAD ({})", self.snapshot.head_name),
                reference: None,
                group: None,
                head: true,
            },
        ];
        for (kind, name, key) in [
            (GitRefKind::Local, "Local", "local"),
            (GitRefKind::Remote, "Remote", "remote"),
            (GitRefKind::Tag, "Tags", "tags"),
        ] {
            let collapsed = self.collapsed.contains(key);
            rows.push(BranchRow {
                label: format!("{} {name}", if collapsed { "▸" } else { "▾" }),
                reference: None,
                group: Some(key.into()),
                head: false,
            });
            if collapsed {
                continue;
            }
            let mut last_remote = String::new();
            for (index, reference) in
                self.snapshot
                    .references
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| {
                        r.kind == kind
                            && r.name
                                .to_lowercase()
                                .contains(&self.branch_query.to_lowercase())
                    })
            {
                if kind == GitRefKind::Remote {
                    let remote = reference.name.split('/').next().unwrap_or("");
                    let remote_key = format!("remote/{remote}");
                    if remote != last_remote {
                        rows.push(BranchRow {
                            label: format!(
                                "  {} {remote}",
                                if self.collapsed.contains(&remote_key) {
                                    "▸"
                                } else {
                                    "▾"
                                }
                            ),
                            reference: None,
                            group: Some(remote_key.clone()),
                            head: false,
                        });
                        last_remote = remote.to_owned();
                    }
                    if self.collapsed.contains(&remote_key) {
                        continue;
                    }
                }
                let label = if kind == GitRefKind::Remote {
                    format!(
                        "    {}",
                        reference
                            .name
                            .split_once('/')
                            .map_or(reference.name.as_str(), |(_, r)| r)
                    )
                } else {
                    format!(
                        "  {}{}",
                        if reference.current { "* " } else { "" },
                        reference.name
                    )
                };
                rows.push(BranchRow {
                    label,
                    reference: Some(index),
                    group: None,
                    head: false,
                });
            }
        }
        rows
    }

    fn filter_selected_branch(&mut self) -> bool {
        let rows = self.branch_rows();
        let Some(row) = rows.get(self.selected_branch) else {
            return false;
        };
        if let Some(group) = &row.group {
            if !self.collapsed.remove(group) {
                self.collapsed.insert(group.clone());
            }
            self.selected_branch = self
                .selected_branch
                .min(self.branch_rows().len().saturating_sub(1));
            return false;
        }
        self.filter.revision = row
            .reference
            .map(|i| self.snapshot.references[i].full_name.clone())
            .or_else(|| row.head.then(|| "HEAD".into()));
        self.selected_commit = 0;
        self.commit_scroll = 0;
        self.target = GitTarget::Commit(String::new());
        true
    }

    fn commit_rows(&self) -> Vec<(usize, bool)> {
        self.snapshot
            .commits
            .iter()
            .enumerate()
            .flat_map(|(i, c)| {
                std::iter::once((i, false)).chain(c.graph_after.iter().map(move |_| (i, true)))
            })
            .collect()
    }

    fn apply_snapshot(&mut self, version: u64, snapshot: GitSnapshot) -> bool {
        if version != self.load_version {
            return false;
        }
        let previous = self
            .snapshot
            .commits
            .get(self.selected_commit)
            .map(|c| c.id.clone());
        self.snapshot = snapshot;
        self.selected_commit = previous
            .and_then(|id| self.snapshot.commits.iter().position(|c| c.id == id))
            .unwrap_or(
                self.selected_commit
                    .min(self.snapshot.commits.len().saturating_sub(1)),
            );
        self.selected_branch = self
            .selected_branch
            .min(self.branch_rows().len().saturating_sub(1));
        self.busy = false;
        true
    }

    fn refresh(&mut self) {
        if self.resolving_repository {
            return;
        }
        if self
            .operation_task
            .as_ref()
            .is_some_and(|t| !t.is_finished())
        {
            return;
        }
        if let Some(task) = self.snapshot_task.take() {
            task.abort();
        }
        self.load_version += 1;
        let (instance, version) = (self.instance, self.load_version);
        let repository = self.repository.clone();
        let path = self.path.clone();
        let filter = self.filter.clone();
        self.busy = true;
        self.message = "Loading Git history...".into();
        self.snapshot_task = Some(tokio::spawn(async move {
            let result = async {
                let repository = match repository {
                    Some(r) => r,
                    None => GitRepository::open(path, true).await?,
                };
                let snapshot = repository.snapshot(&filter).await?;
                Ok::<_, anyhow::Error>((repository, snapshot))
            }
            .await;
            job::dispatch(move |editor, compositor| {
                let Some(panel) = compositor
                    .find::<GitPanel>()
                    .filter(|p| p.instance == instance && p.load_version == version)
                else {
                    return;
                };
                panel.snapshot_task = None;
                match result {
                    Ok((repository, snapshot)) => {
                        panel.repository = Some(repository);
                        panel.apply_snapshot(version, snapshot);
                        panel.message = if panel.snapshot.has_more {
                            "m: load more commits".into()
                        } else {
                            String::new()
                        };
                        panel.request_details();
                        let action = panel.pending_action.take();
                        if let Some(action) = action {
                            actions::perform_open_action(compositor, editor, action);
                        }
                        crate::handlers::git_conflicts::request(
                            editor,
                            crate::handlers::git_conflicts::ConflictScanReason::Explicit,
                        );
                    }
                    Err(error) => {
                        panel.busy = false;
                        panel.message = format!("{error:#}");
                        editor.set_error(panel.message.clone());
                    }
                }
            })
            .await;
        }));
    }

    fn resolve_open(&mut self, path: PathBuf, action: OpenAction) {
        self.load_version += 1;
        self.detail_version += 1;
        self.patch_version += 1;
        self.plan_version += 1;
        for task in [
            &self.repository_task,
            &self.snapshot_task,
            &self.detail_task,
            &self.patch_task,
            &self.plan_task,
        ]
        .into_iter()
        .flatten()
        {
            task.abort();
        }
        self.repository_task = None;
        self.snapshot_task = None;
        self.detail_task = None;
        self.patch_task = None;
        self.plan_task = None;
        self.confirmation = None;
        self.action_menu = false;
        self.resolving_repository = true;
        self.busy = true;
        self.focused = true;
        self.message = "Discovering current Git worktree...".into();
        let (instance, version) = (self.instance, self.load_version);
        self.repository_task = Some(tokio::spawn(async move {
            let result = GitRepository::open(path.clone(), true).await;
            job::dispatch(move |editor, compositor| {
                let Some(panel) = compositor
                    .find::<GitPanel>()
                    .filter(|p| p.instance == instance && p.load_version == version)
                else {
                    return;
                };
                panel.repository_task = None;
                panel.resolving_repository = false;
                match result {
                    Ok(repository) => {
                        if panel
                            .repository
                            .as_ref()
                            .is_some_and(|old| !old.same_worktree(&repository))
                        {
                            let fullscreen = panel.fullscreen;
                            *panel = GitPanel::new(path.clone(), action.clone());
                            panel.fullscreen = fullscreen;
                        }
                        panel.path = path;
                        panel.repository = Some(repository);
                        panel.pending_action = Some(action.clone());
                        if panel
                            .operation_task
                            .as_ref()
                            .is_some_and(|t| !t.is_finished())
                        {
                            actions::perform_open_action(compositor, editor, action);
                        } else {
                            panel.refresh();
                        }
                    }
                    Err(error) => {
                        panel.busy = false;
                        panel.message = format!("{error:#}");
                        editor.set_error(panel.message.clone());
                    }
                }
            })
            .await;
        }));
    }

    fn release_editor(&mut self) {
        self.focused = false;
        self.fullscreen = false;
        self.confirmation = None;
        self.action_menu = false;
        self.plan_version += 1;
        if let Some(task) = self.plan_task.take() {
            task.abort();
        }
    }

    fn request_details(&mut self) {
        if let Some(task) = self.detail_task.take() {
            task.abort();
        }
        if let Some(task) = self.patch_task.take() {
            task.abort();
        }
        self.detail_version += 1;
        self.patch_version += 1;
        self.selected_file = 0;
        self.file_scroll = 0;
        self.diff_scroll = 0;
        self.diff_horizontal = 0;
        self.details = GitDetails::default();
        self.patch.clear();
        if matches!(self.target, GitTarget::Commit(_)) {
            let Some(commit) = self.snapshot.commits.get(self.selected_commit) else {
                return;
            };
            self.target = GitTarget::Commit(commit.id.clone());
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let (instance, version, target) = (self.instance, self.detail_version, self.target.clone());
        self.detail_task = Some(tokio::spawn(async move {
            let result = repository.details(&target).await;
            job::dispatch(move |editor, compositor| {
                let Some(panel) = compositor
                    .find::<GitPanel>()
                    .filter(|p| p.instance == instance && p.detail_version == version)
                else {
                    return;
                };
                panel.detail_task = None;
                match result {
                    Ok(mut details) => {
                        if matches!(panel.target, GitTarget::WorkingTree) {
                            details.files.sort_by_key(|file| !panel.is_conflict(file));
                        }
                        panel.details = details;
                        panel.request_patch();
                    }
                    Err(error) => {
                        panel.message = format!("{error:#}");
                        editor.set_error(panel.message.clone());
                    }
                }
            })
            .await;
        }));
    }

    fn request_patch(&mut self) {
        if let Some(task) = self.patch_task.take() {
            task.abort();
        }
        self.patch_version += 1;
        self.diff_scroll = 0;
        self.diff_horizontal = 0;
        self.patch.clear();
        let (Some(repository), Some(file)) = (
            self.repository.clone(),
            self.details.files.get(self.selected_file).cloned(),
        ) else {
            return;
        };
        if self.is_conflict(&file) {
            self.patch = "Unmerged file · Enter or Space v r to resolve".into();
            return;
        }
        let (instance, version, target) = (self.instance, self.patch_version, self.target.clone());
        self.patch_task = Some(tokio::spawn(async move {
            let result = repository.patch(&target, &file).await;
            job::dispatch(move |editor, compositor| {
                let Some(panel) = compositor
                    .find::<GitPanel>()
                    .filter(|p| p.instance == instance && p.patch_version == version)
                else {
                    return;
                };
                panel.patch_task = None;
                match result {
                    Ok(patch) => {
                        // In a docked window, show the actual changed lines
                        // before the potentially lengthy Git file headers.
                        panel.diff_scroll = patch
                            .lines()
                            .position(|line| line.starts_with("@@"))
                            .unwrap_or(0);
                        panel.patch = patch;
                    }
                    Err(error) => {
                        panel.message = format!("{error:#}");
                        editor.set_error(panel.message.clone());
                    }
                }
            })
            .await;
        }));
    }

    fn move_selection(&mut self, down: bool, count: usize) {
        let move_index = |index: usize, len: usize| {
            if down {
                index.saturating_add(count).min(len.saturating_sub(1))
            } else {
                index.saturating_sub(count)
            }
        };
        match self.focus {
            Pane::Branches => {
                self.selected_branch = move_index(self.selected_branch, self.branch_rows().len())
            }
            Pane::Commits => {
                let next = move_index(self.selected_commit, self.snapshot.commits.len());
                if next != self.selected_commit {
                    self.selected_commit = next;
                    self.target = GitTarget::Commit(String::new());
                    self.request_details();
                }
            }
            Pane::Files => {
                let next = move_index(self.selected_file, self.details.files.len());
                if next != self.selected_file {
                    self.selected_file = next;
                    self.request_patch();
                }
            }
            Pane::Diff => {
                self.diff_scroll = move_index(self.diff_scroll, self.patch.lines().count())
            }
        }
    }
}

impl Component for GitPanel {
    fn takes_keyboard_input(&self) -> bool {
        self.focused && !self.suspended
    }

    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }

    fn editor_area(&self, area: Rect) -> Rect {
        if self.suspended || self.fullscreen || area.height < 12 {
            area
        } else {
            area.clip_bottom(self.layout(area).panel.height)
        }
    }

    fn editor_area_for(&self, area: Rect, editor: &Editor) -> Rect {
        if editor.active_merge().is_some() {
            area
        } else {
            self.editor_area(area)
        }
    }

    fn render(&mut self, area: Rect, frame: &mut Buffer, cx: &mut Context) {
        if self.suspended {
            return;
        }
        if cx.editor.active_merge().is_some() {
            return;
        }
        self.last_layout = self.layout(area);
        self.draw(frame, &cx.editor.theme);
        // The editor's which-key menu must remain above a docked window.
        if !self.focused {
            if let Some(mut info) = cx.editor.autoinfo.take() {
                info.render(area, frame, cx);
                cx.editor.autoinfo = Some(info);
            }
        }
    }

    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        if self.focused && editor.active_merge().is_none() {
            (
                Some(Position::new(area.y as usize, area.x as usize)),
                CursorKind::Hidden,
            )
        } else {
            (None, CursorKind::Hidden)
        }
    }

    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if self.suspended {
            return EventResult::Ignored(None);
        }
        if cx.editor.active_merge().is_some() {
            return EventResult::Ignored(None);
        }
        if let Event::Mouse(mouse) = event {
            let layout = self.last_layout;
            let inside = |r: Rect| {
                mouse.column >= r.x
                    && mouse.column < r.right()
                    && mouse.row >= r.y
                    && mouse.row < r.bottom()
            };
            if !inside(layout.panel) {
                return EventResult::Ignored(None);
            }
            if matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Up(_)) {
                return EventResult::Consumed(None);
            }
            self.focused = true;
            for (pane, rect) in [
                (Pane::Branches, layout.branches),
                (Pane::Commits, layout.commits),
                (Pane::Files, layout.files),
                (Pane::Diff, layout.diff),
            ] {
                if !inside(rect) {
                    continue;
                }
                self.focus = pane;
                match mouse.kind {
                    MouseEventKind::ScrollDown => self.move_selection(true, 3),
                    MouseEventKind::ScrollUp => self.move_selection(false, 3),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let row = mouse.row.saturating_sub(rect.y + 1) as usize;
                        match pane {
                            Pane::Branches => {
                                self.selected_branch = (self.branch_scroll + row)
                                    .min(self.branch_rows().len().saturating_sub(1));
                                if self.filter_selected_branch() {
                                    self.refresh();
                                }
                            }
                            Pane::Commits => {
                                if let Some((index, _)) =
                                    self.commit_rows().get(self.commit_scroll + row).copied()
                                {
                                    self.selected_commit = index;
                                    self.target = GitTarget::Commit(String::new());
                                    self.request_details();
                                }
                            }
                            Pane::Files => {
                                let offset = render::file_header_rows(&self.details, rect);
                                if row >= offset {
                                    if let Some(FileRow::File(index)) = self
                                        .file_rows()
                                        .get(self.file_scroll + row - offset)
                                        .copied()
                                    {
                                        self.selected_file = index;
                                        self.request_patch();
                                    }
                                }
                            }
                            Pane::Diff => {}
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => self.action_menu = true,
                    _ => {}
                }
            }
            return EventResult::Consumed(None);
        }
        if matches!(event, Event::FocusGained) {
            if self.snapshot_task.is_none() && self.repository.is_some() {
                self.refresh();
            }
            return EventResult::Ignored(None);
        }
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        if !self.focused {
            return EventResult::Ignored(None);
        }
        if key.code == KeyCode::Char('w') && key.modifiers == KeyModifiers::CONTROL {
            self.release_editor();
            return EventResult::Ignored(None);
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            if key.modifiers == KeyModifiers::CONTROL {
                match key.code {
                    KeyCode::Char('c') => {
                        self.confirmation = None;
                        self.action_menu = false;
                    }
                    KeyCode::Char('d') if self.confirmation.is_none() => {
                        self.move_selection(true, 10)
                    }
                    KeyCode::Char('u') if self.confirmation.is_none() => {
                        self.move_selection(false, 10)
                    }
                    _ => {}
                }
            }
            return EventResult::Consumed(None);
        }
        if self.confirmation.is_some() {
            if key.code == KeyCode::Char('y') {
                self.start_operation(cx.editor, cx.jobs);
            } else if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q')
            ) {
                self.confirmation = None;
            }
            return EventResult::Consumed(None);
        }
        if self.action_menu {
            return self.handle_action_menu(key.code, cx);
        }
        if key.code == KeyCode::Char(' ')
            || (key.code == KeyCode::Char('w') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.release_editor();
            return EventResult::Ignored(None);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                return EventResult::Consumed(Some(Box::new(|compositor, _| {
                    compositor.remove(ID);
                })))
            }
            KeyCode::F(11) | KeyCode::Char('z') => self.fullscreen = !self.fullscreen,
            KeyCode::Tab => {
                self.focus = self.focus.next(key.modifiers.contains(KeyModifiers::SHIFT))
            }
            KeyCode::Char('1') => self.focus = Pane::Branches,
            KeyCode::Char('2') => self.focus = Pane::Commits,
            KeyCode::Char('3') => self.focus = Pane::Files,
            KeyCode::Char('4') => self.focus = Pane::Diff,
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(true, 1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(false, 1),
            KeyCode::PageDown => self.move_selection(true, 10),
            KeyCode::PageUp => self.move_selection(false, 10),
            KeyCode::Home => self.move_selection(false, usize::MAX),
            KeyCode::End => self.move_selection(true, usize::MAX),
            KeyCode::Left | KeyCode::Char('h') if self.focus == Pane::Diff => {
                self.diff_horizontal = self.diff_horizontal.saturating_sub(8)
            }
            KeyCode::Right | KeyCode::Char('l') if self.focus == Pane::Diff => {
                self.diff_horizontal = self.diff_horizontal.saturating_add(8)
            }
            KeyCode::Left | KeyCode::Char('h') => self.focus = self.focus.next(true),
            KeyCode::Right | KeyCode::Char('l') => self.focus = self.focus.next(false),
            KeyCode::Enter => match self.focus {
                Pane::Branches => {
                    if self.filter_selected_branch() {
                        self.refresh();
                        self.focus = Pane::Commits;
                    }
                }
                Pane::Commits => {
                    self.focus = Pane::Files;
                }
                Pane::Files => {
                    if self
                        .details
                        .files
                        .get(self.selected_file)
                        .is_some_and(|file| self.is_conflict(file))
                    {
                        return self.open_selected_file(false, cx.editor);
                    }
                    self.request_patch();
                    self.focus = Pane::Diff;
                }
                Pane::Diff => {}
            },
            KeyCode::F(5) | KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('m') => {
                if self.snapshot.has_more && self.filter.limit < 5000 {
                    self.filter.limit = (self.filter.limit + 200).min(5000);
                    self.refresh();
                }
            }
            KeyCode::Char('/') => {
                return self.filter_prompt(
                    if self.focus == Pane::Branches {
                        actions::FilterKind::Branch
                    } else {
                        actions::FilterKind::Text
                    },
                    cx.editor,
                )
            }
            KeyCode::Char('A') => {
                return self.filter_prompt(actions::FilterKind::Author, cx.editor)
            }
            KeyCode::Char('D') => return self.filter_prompt(actions::FilterKind::Date, cx.editor),
            KeyCode::Char('p') => return self.filter_prompt(actions::FilterKind::Path, cx.editor),
            KeyCode::Char('R') => {
                self.filter = GitLogFilter::default();
                self.branch_query.clear();
                self.target = GitTarget::Commit(String::new());
                self.refresh();
            }
            KeyCode::Char('s') => {
                self.target = GitTarget::WorkingTree;
                self.request_details();
                self.focus = Pane::Files;
            }
            KeyCode::Char('a') => self.action_menu = true,
            KeyCode::Char('d') => return self.show_details(),
            KeyCode::Char('y') => {
                if let GitTarget::Commit(id) = &self.target {
                    match cx.editor.registers.write('+', vec![id.clone()]) {
                        Ok(_) => {
                            self.message = "Copied commit hash".into();
                        }
                        Err(error) => cx.editor.set_error(format!("{error}")),
                    }
                }
            }
            KeyCode::Char('o') => return self.open_selected_file(false, cx.editor),
            KeyCode::Char('O') => return self.open_selected_file(true, cx.editor),
            _ => {}
        }
        EventResult::Consumed(None)
    }
}

#[cfg(test)]
mod test;
