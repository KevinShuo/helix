use super::*;
use crate::{
    compositor::Compositor,
    job::{Callback, Job, Jobs},
    ui::{Prompt, PromptEvent},
};
use helix_core::{Selection, Transaction};
use helix_view::editor::Action;

#[derive(Clone, Copy)]
pub(super) enum FilterKind {
    Text,
    Author,
    Date,
    Path,
    Branch,
}

pub(super) fn perform_open_action(
    compositor: &mut Compositor,
    editor: &mut Editor,
    action: OpenAction,
) {
    let Some(panel) = compositor.find::<GitPanel>() else {
        return;
    };
    panel.pending_action = None;
    panel.focused = true;
    if matches!(
        action,
        OpenAction::NewBranch | OpenAction::Fetch | OpenAction::Pull | OpenAction::Push
    ) && panel
        .repository
        .as_ref()
        .is_some_and(GitRepository::operation_in_progress)
    {
        editor.set_error("A Git operation is already running for this repository");
        return;
    }
    match action {
        OpenAction::Conflicts => {}
        OpenAction::Log => {
            panel.target = GitTarget::Commit(String::new());
            panel.request_details();
            panel.focus = Pane::Commits;
        }
        OpenAction::Branches => panel.focus = Pane::Branches,
        OpenAction::Status => {
            panel.target = GitTarget::WorkingTree;
            panel.request_details();
            panel.focus = Pane::Files;
        }
        OpenAction::FileHistory(path) => {
            panel.filter.path = Some(path);
            panel.target = GitTarget::Commit(String::new());
            panel.refresh();
        }
        OpenAction::NewBranch => {
            let instance = panel.instance;
            let prompt = new_branch_prompt(instance, editor);
            compositor.push(Box::new(prompt));
        }
        OpenAction::Fetch => panel.confirm(GitOperation::Fetch, editor),
        OpenAction::Pull => panel.confirm(GitOperation::Pull, editor),
        OpenAction::Push => panel.confirm(GitOperation::Push, editor),
    }
}

fn new_branch_prompt(instance: u64, editor: &Editor) -> Prompt {
    Prompt::new(
        "New Git branch: ".into(),
        None,
        |_, _| Vec::new(),
        move |_, input, event| {
            if event != PromptEvent::Validate || input.trim().is_empty() {
                return;
            }
            let name = input.trim().to_owned();
            job::dispatch_blocking(move |editor, compositor| {
                let Some(panel) = compositor
                    .find::<GitPanel>()
                    .filter(|p| p.instance == instance)
                else {
                    return;
                };
                let start = match &panel.target {
                    GitTarget::Commit(id) if !id.is_empty() => id.clone(),
                    _ => "HEAD".into(),
                };
                panel.confirm(GitOperation::NewBranch { name, start }, editor);
            });
        },
    )
    .with_line(String::new(), editor)
}

pub(super) fn operation_description(operation: &GitOperation, current: &str) -> Vec<String> {
    match operation {
        GitOperation::Checkout { reference } => vec![
            format!("Switch from: {current}"),
            format!("Switch to: {}", reference.name),
            if reference.kind == GitRefKind::Tag {
                "Checkout tag in detached HEAD state".into()
            } else if reference.kind == GitRefKind::Remote {
                "Create a local tracking branch (no overwrite)".into()
            } else {
                "Update working files; Git preserves local changes or refuses".into()
            },
        ],
        GitOperation::NewBranch { name, start } => vec![
            format!("Create and switch to: {name}"),
            format!("Starting commit: {start}"),
        ],
        GitOperation::Merge { reference } => vec![
            format!("Merge into: {current}"),
            format!("Source: {reference}"),
            "Git may create a merge commit or report conflicts".into(),
        ],
        GitOperation::CherryPick { commit } => vec![
            format!("Apply to: {current}"),
            format!("Commit: {commit}"),
            "Git may report conflicts that require resolution".into(),
        ],
        GitOperation::Fetch => vec![
            "Fetch all configured remotes".into(),
            "Update remote references without changing working files".into(),
        ],
        GitOperation::Pull => vec![
            format!("Pull into: {current}"),
            "Only fast-forward updates; diverged branches are refused".into(),
        ],
        GitOperation::Push => vec![
            format!("Push only branch: {current}"),
            "Configured push remote; no force and no unrelated refs".into(),
        ],
        GitOperation::PushPlanned(plan) => vec![
            format!(
                "Push from: {} @ {}",
                plan.local_branch,
                &plan.source_id[..8.min(plan.source_id.len())]
            ),
            format!("Remote: {}", plan.remote),
            format!("Destination: {}", plan.destination),
            "No force; no unrelated branches or tags".into(),
        ],
    }
}

fn dirty_buffers(editor: &Editor, repository: &GitRepository) -> bool {
    editor.documents().any(|doc| {
        doc.is_modified()
            && doc
                .path()
                .is_some_and(|p| repository.contains_path(p).unwrap_or(true))
    })
}

impl GitPanel {
    pub(super) fn show_details(&self) -> EventResult {
        let metadata = self
            .snapshot
            .commits
            .get(self.selected_commit)
            .filter(|_| matches!(self.target, GitTarget::Commit(_)));
        let mut lines = vec!["Git commit details · PgUp/PgDn scroll · Esc close".to_owned()];
        if let Some(commit) = metadata {
            lines.extend([
                format!("Commit: {}", commit.id),
                format!("Author: {}", commit.author),
                format!("Date: {}", commit.date),
                format!("Refs: {}", commit.decoration),
                format!("Parents: {}", commit.parents.join(" ")),
                String::new(),
            ]);
        }
        lines.extend(self.details.message.lines().map(render::safe_text));
        let contents = CommitText {
            text: lines
                .iter()
                .map(|l| render::safe_text(l))
                .collect::<Vec<_>>()
                .join("\n"),
        };
        let popup = crate::ui::Popup::new("git-commit-details", contents);
        EventResult::Consumed(Some(Box::new(move |compositor, _| {
            compositor.push(Box::new(popup))
        })))
    }

    pub(super) fn selected_reference(&self) -> Option<&GitReference> {
        self.branch_rows()
            .get(self.selected_branch)
            .and_then(|r| r.reference)
            .and_then(|i| self.snapshot.references.get(i))
    }

    fn confirm(&mut self, operation: GitOperation, editor: &mut Editor) {
        if self.resolving_repository {
            editor.set_error("Git repository is still loading");
            return;
        }
        let Some(repo) = &self.repository else {
            editor.set_error("Git repository is still loading");
            return;
        };
        if !trusted(editor, repo.root()) {
            editor.set_error("Git tools require a trusted workspace (:trust)");
            return;
        }
        if repo.operation_in_progress()
            || self.plan_task.as_ref().is_some_and(|t| !t.is_finished())
            || self
                .operation_task
                .as_ref()
                .is_some_and(|t| !t.is_finished())
        {
            editor.set_error("A Git operation is already running");
            return;
        }
        if operation.changes_worktree() && dirty_buffers(editor, repo) {
            self.message =
                "Save modified buffers in this repository before changing branches or pulling"
                    .into();
            editor.set_error(self.message.clone());
            return;
        }
        if matches!(operation, GitOperation::Push) {
            let repository = repo.clone();
            self.plan_version += 1;
            let (instance, version) = (self.instance, self.plan_version);
            self.message = "Resolving push remote and destination...".into();
            self.plan_task = Some(tokio::spawn(async move {
                let result = repository.push_plan().await;
                job::dispatch(move |editor, compositor| {
                    let Some(panel) = compositor
                        .find::<GitPanel>()
                        .filter(|p| p.instance == instance && p.plan_version == version)
                    else {
                        return;
                    };
                    panel.plan_task = None;
                    match result {
                        Ok(plan) => {
                            panel.message.clear();
                            panel.confirm(GitOperation::PushPlanned(plan), editor);
                        }
                        Err(error) => {
                            panel.message = format!("{error:#}");
                            editor.set_error(panel.message.clone());
                        }
                    }
                })
                .await;
            }));
            return;
        }
        editor.set_status(operation_description(&operation, &self.snapshot.head_name).join(" · "));
        self.action_menu = false;
        self.confirmation = Some(operation);
    }

    pub(super) fn start_operation(&mut self, editor: &mut Editor, jobs: &Jobs) {
        let (Some(operation), Some(repository)) =
            (self.confirmation.take(), self.repository.clone())
        else {
            return;
        };
        if !trusted(editor, repository.root()) {
            editor.set_error("Git tools require a trusted workspace (:trust)");
            return;
        }
        if operation.changes_worktree() && dirty_buffers(editor, &repository) {
            editor.set_error("Save modified buffers before this Git operation");
            return;
        }
        self.load_version += 1;
        self.detail_version += 1;
        self.patch_version += 1;
        for task in [&self.snapshot_task, &self.detail_task, &self.patch_task]
            .into_iter()
            .flatten()
        {
            task.abort();
        }
        self.snapshot_task = None;
        self.detail_task = None;
        self.patch_task = None;
        self.busy = true;
        editor.git_conflict_scan.cancel();
        self.message = "Running Git operation...".into();
        self.operation_version = self.operation_version.wrapping_add(1);
        let (instance, operation_version, load_version) =
            (self.instance, self.operation_version, self.load_version);
        let context = editor.git_context_path();
        let document = editor.tree.try_get(editor.tree.focus).map(|view| view.doc);
        let changes_worktree = operation.changes_worktree();
        let task = tokio::spawn(async move {
            let result = repository.operate(&operation).await;
            // The exit status/message alone cannot tell whether an operation
            // left unmerged index stages (including localized Git output).
            let snapshot = repository.conflicts().await;
            (repository, result, snapshot)
        });
        self.operation_task = Some(task.abort_handle());
        jobs.add(
            Job::with_callback(async move {
                let (repository, result, snapshot) = task.await?;
                Ok(Callback::Editor(Box::new(move |editor| {
                    if changes_worktree {
                        merge::reload_clean(editor, repository.root(), None);
                    }
                    let mut message = match &result {
                        Ok(message) => {
                            if message.is_empty() {
                                "Git operation completed".into()
                            } else {
                                message.clone()
                            }
                        }
                        Err(error) => format!("{error:#}"),
                    };
                    let success = result.is_ok();
                    let snapshot = match snapshot {
                        Ok(snapshot) => Some(snapshot),
                        Err(error) => {
                            message.push_str(&format!("; conflict scan failed: {error:#}"));
                            None
                        }
                    };
                    let in_context = editor.git_context_path() == context
                        && editor.tree.try_get(editor.tree.focus).map(|view| view.doc) == document
                        && trusted(editor, repository.root());
                    if in_context {
                        if let Some(snapshot) = snapshot.as_ref().filter(|s| !s.files.is_empty()) {
                            editor
                                .git_conflict_scan
                                .should_notify(repository.root(), snapshot);
                            editor.set_status(format!(
                                "Git: {} conflicted files; Space v r to resolve",
                                snapshot.files.len()
                            ));
                        } else if success {
                            editor.set_status(render::safe_text(&message));
                        } else {
                            editor.set_error(render::safe_text(&message));
                        }
                    }
                    // An Editor callback also runs during exit, when there may be
                    // no compositor. UI updates are separately guarded on delivery.
                    job::dispatch_blocking(move |editor, compositor| {
                        let in_context = editor.git_context_path() == context
                            && editor.tree.try_get(editor.tree.focus).map(|view| view.doc)
                                == document
                            && trusted(editor, repository.root());
                        let Some(panel) = compositor.find::<GitPanel>().filter(|p| {
                            (p.instance != instance || p.operation_version == operation_version)
                                && p.repository
                                    .as_ref()
                                    .is_some_and(|r| r.same_worktree(&repository))
                        }) else {
                            return;
                        };
                        let originating_panel = panel.instance == instance;
                        let show_conflicts = originating_panel
                            && in_context
                            && panel.focused
                            && panel.load_version == load_version;
                        if originating_panel {
                            panel.operation_task = None;
                            panel.busy = false;
                            panel.message = render::safe_text(&message);
                        }
                        // A reopened panel still needs fresh data, while only
                        // the originating panel may automatically open a list.
                        panel.refresh();
                        if let Some(snapshot) =
                            snapshot.filter(|s| show_conflicts && !s.files.is_empty())
                        {
                            conflicts::present_snapshot(compositor, editor, repository, snapshot);
                        }
                    });
                })))
            })
            .wait_before_exiting(),
        );
    }

    pub(super) fn handle_action_menu(&mut self, key: KeyCode, cx: &mut Context) -> EventResult {
        self.action_menu = false;
        match key {
            KeyCode::Char('c') => {
                if let Some(reference) = self.selected_reference().cloned() {
                    self.confirm(GitOperation::Checkout { reference }, cx.editor);
                } else {
                    cx.editor
                        .set_error("Select a local branch, remote branch or tag first");
                }
            }
            KeyCode::Char('n') => {
                let instance = self.instance;
                return EventResult::Consumed(Some(Box::new(move |compositor, cx| {
                    compositor.push(Box::new(new_branch_prompt(instance, cx.editor)));
                })));
            }
            KeyCode::Char('m') => {
                if let Some(reference) = self.selected_reference() {
                    self.confirm(
                        GitOperation::Merge {
                            reference: reference.full_name.clone(),
                        },
                        cx.editor,
                    );
                } else {
                    cx.editor
                        .set_error("Select the branch to merge in the branch tree");
                }
            }
            KeyCode::Char('p') => {
                if let GitTarget::Commit(commit) = &self.target {
                    if !commit.is_empty() {
                        self.confirm(
                            GitOperation::CherryPick {
                                commit: commit.clone(),
                            },
                            cx.editor,
                        );
                    }
                }
            }
            KeyCode::Char('f') => self.confirm(GitOperation::Fetch, cx.editor),
            KeyCode::Char('u') => self.confirm(GitOperation::Pull, cx.editor),
            KeyCode::Char('P') => self.confirm(GitOperation::Push, cx.editor),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    pub(super) fn filter_prompt(&self, kind: FilterKind, editor: &Editor) -> EventResult {
        let (label, input) = match kind {
            FilterKind::Text => (
                "Git search (message or full hash): ",
                self.filter.text.clone(),
            ),
            FilterKind::Author => ("Git author: ", self.filter.author.clone()),
            FilterKind::Date => (
                "Git date (since..until, empty clears): ",
                format!("{}..{}", self.filter.since, self.filter.until),
            ),
            FilterKind::Path => (
                "Git path (relative to repository): ",
                self.filter
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            FilterKind::Branch => ("Find branch or tag: ", self.branch_query.clone()),
        };
        let instance = self.instance;
        let prompt = Prompt::new(
            label.into(),
            None,
            |_, _| Vec::new(),
            move |_, input, event| {
                if event != PromptEvent::Validate {
                    return;
                }
                let input = input.to_owned();
                job::dispatch_blocking(move |_, compositor| {
                    let Some(panel) = compositor
                        .find::<GitPanel>()
                        .filter(|p| p.instance == instance)
                    else {
                        return;
                    };
                    match kind {
                        FilterKind::Text => {
                            if matches!(input.len(), 40 | 64)
                                && input.bytes().all(|c| c.is_ascii_hexdigit())
                            {
                                panel.filter.revision = Some(input);
                                panel.filter.text.clear();
                            } else {
                                panel.filter.text = input;
                            }
                        }
                        FilterKind::Author => panel.filter.author = input,
                        FilterKind::Date => {
                            let (since, until) = input.split_once("..").unwrap_or((&input, ""));
                            panel.filter.since = since.to_owned();
                            panel.filter.until = until.to_owned();
                        }
                        FilterKind::Path => {
                            panel.filter.path = (!input.is_empty()).then(|| PathBuf::from(input))
                        }
                        FilterKind::Branch => {
                            panel.branch_query = input;
                            panel.selected_branch = 0;
                            panel.branch_scroll = 0;
                            return;
                        }
                    }
                    panel.selected_commit = 0;
                    panel.commit_scroll = 0;
                    panel.filter.limit = 200;
                    panel.target = GitTarget::Commit(String::new());
                    panel.refresh();
                });
            },
        )
        .with_line(input, editor);
        EventResult::Consumed(Some(Box::new(move |compositor, _| {
            compositor.push(Box::new(prompt))
        })))
    }

    pub(super) fn open_selected_file(
        &mut self,
        historical: bool,
        editor: &mut Editor,
    ) -> EventResult {
        let (Some(repository), Some(file)) = (
            self.repository.clone(),
            self.details.files.get(self.selected_file).cloned(),
        ) else {
            return EventResult::Consumed(None);
        };
        if !historical && self.is_conflict(&file) {
            let root = repository.root().to_owned();
            return EventResult::Consumed(Some(Box::new(move |compositor, cx| {
                conflicts::present(compositor, cx.editor, root);
                if let Some(list) = compositor.find::<conflicts::ConflictList>() {
                    list.request_file(file.path, false, None);
                }
            })));
        }
        if !historical {
            let path = repository.root().join(&file.path);
            if !path.is_file() {
                editor.set_error(
                    "File does not exist in the working tree; use O to open its revision",
                );
                return EventResult::Consumed(None);
            }
            match editor.open(&path, Action::Replace) {
                Ok(_) => self.release_editor(),
                Err(error) => editor.set_error(format!("{error:#}")),
            }
        } else {
            let GitTarget::Commit(id) = &self.target else {
                editor.set_error("Select a commit to open its file revision");
                return EventResult::Consumed(None);
            };
            let id = if file.status.starts_with('D') {
                self.snapshot
                    .commits
                    .get(self.selected_commit)
                    .and_then(|c| c.parents.first())
                    .cloned()
                    .unwrap_or_else(|| id.clone())
            } else {
                id.clone()
            };
            let path = file
                .old_path
                .filter(|_| file.status.starts_with('D'))
                .unwrap_or(file.path);
            tokio::spawn(async move {
                let result = repository.file_at(&id, &path).await;
                job::dispatch(move |editor, _| match result {
                    Ok(bytes) => {
                        if bytes.contains(&0) {
                            editor.set_error("Cannot open binary revision as text");
                            return;
                        }
                        let contents = String::from_utf8_lossy(&bytes);
                        editor.new_file(Action::VerticalSplit);
                        let (view, doc) = helix_view::current!(editor);
                        let change = Transaction::change(
                            doc.text(),
                            std::iter::once((
                                0,
                                doc.text().len_chars(),
                                Some(contents.as_ref().into()),
                            )),
                        )
                        .with_selection(Selection::point(0));
                        doc.apply(&change, view.id);
                        doc.append_changes_to_history(view);
                        editor.set_status(format!(
                            "Revision {} · {} (scratch buffer)",
                            &id[..8],
                            path.display()
                        ));
                    }
                    Err(error) => editor.set_error(format!("{error:#}")),
                })
                .await;
            });
            self.release_editor();
        }
        EventResult::Consumed(None)
    }
}

struct CommitText {
    text: String,
}

impl Component for CommitText {
    fn required_size(&mut self, viewport: (u16, u16)) -> Option<(u16, u16)> {
        use helix_core::unicode::width::UnicodeWidthStr;
        let width = viewport.0.max(1) as usize;
        let height = self
            .text
            .lines()
            .map(|l| l.width().max(1).div_ceil(width))
            .sum::<usize>();
        Some((viewport.0, height.min(u16::MAX as usize - 2) as u16))
    }
    fn render(&mut self, area: Rect, frame: &mut Buffer, cx: &mut Context) {
        use tui::widgets::{Paragraph, Widget, Wrap};
        let text = tui::text::Text::from(self.text.as_str());
        Paragraph::new(&text)
            .wrap(Wrap { trim: false })
            .scroll((cx.scroll.unwrap_or(0).min(u16::MAX as usize) as u16, 0))
            .render(area, frame);
    }
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        if let Event::Key(key) = event {
            if key.code == KeyCode::Char('q') {
                return EventResult::Consumed(Some(Box::new(|compositor, _| {
                    compositor.remove("git-commit-details");
                })));
            }
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
