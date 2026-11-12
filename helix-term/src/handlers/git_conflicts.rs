//! Bounded, cancellable discovery for the current editor context. Notifications
//! never open a component; an explicitly requested Git operation handles its
//! own guarded conflict-list presentation.

use std::time::Duration;

use helix_event::register_hook;
use helix_vcs::git_tools::GitRepository;
use helix_view::{
    events::{DocumentDidClose, DocumentFocusLost},
    Editor,
};

use crate::{job, ui::git::trusted};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConflictScanReason {
    Explicit,
    FocusGained,
}

fn context(editor: &Editor) -> Option<std::path::PathBuf> {
    editor
        .git_context_path()
        .map(|path| helix_stdx::path::normalize(std::fs::canonicalize(&path).unwrap_or(path)))
}

pub(crate) fn request(editor: &mut Editor, reason: ConflictScanReason) {
    // Focus is an ordinary terminal event even in a build without Git support.
    if !cfg!(feature = "git") && reason == ConflictScanReason::FocusGained {
        return;
    }
    let Some(path) = context(editor) else {
        return;
    };
    if !trusted(editor, &path) {
        editor.git_conflict_scan.cancel();
        if reason == ConflictScanReason::Explicit {
            editor.set_error("Git tools require a trusted workspace (:trust)");
        }
        return;
    }
    let document = editor.tree.try_get(editor.tree.focus).map(|view| view.doc);
    let session = editor.active_merge();
    let ticket = editor.git_conflict_scan.start(path.clone());
    tokio::spawn(async move {
        let query = async {
            if reason == ConflictScanReason::FocusGained {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let repository = GitRepository::open(path, true).await?;
            let snapshot = repository.conflicts().await?;
            anyhow::Ok((repository, snapshot))
        };
        let result = tokio::select! {
            biased;
            _ = ticket.canceled() => return,
            result = query => result,
        };
        job::dispatch(move |editor, _compositor| {
            let Some(path) = context(editor) else {
                return;
            };
            if !editor.git_conflict_scan.accepts(&ticket, &path)
                || editor.tree.try_get(editor.tree.focus).map(|view| view.doc) != document
                || editor.active_merge() != session
                || !trusted(editor, &path)
            {
                return;
            }
            match result {
                Ok((repository, snapshot)) => {
                    if !trusted(editor, repository.root()) {
                        return;
                    }
                    let changed = editor
                        .git_conflict_scan
                        .should_notify(repository.root(), &snapshot);
                    if !snapshot.files.is_empty()
                        && (changed || reason == ConflictScanReason::Explicit)
                    {
                        editor.set_status(format!(
                            "Git: {} conflicted files; Space v r to resolve",
                            snapshot.files.len()
                        ));
                    }
                }
                Err(error) if reason == ConflictScanReason::Explicit => {
                    editor.set_error(format!("{error:#}"))
                }
                Err(_) => (), // Ordinary focus outside a Git worktree stays quiet.
            }
        })
        .await;
    });
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentFocusLost<'_>| {
        event.editor.git_conflict_scan.cancel();
        Ok(())
    });
    register_hook!(move |event: &mut DocumentDidClose<'_>| {
        event.editor.git_conflict_scan.cancel();
        Ok(())
    });
}
