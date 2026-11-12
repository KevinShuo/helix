use super::*;
use crate::job::{Callback, Job};
use helix_vcs::git_tools::{ApplyOutcome, GitRepository, ResolutionRequest};
use helix_view::merge::MergeWriteStatus;
use std::path::Path;

pub(crate) fn reload_clean(editor: &mut Editor, root: &Path, target: Option<&Path>) {
    if editor.tree.is_empty() {
        return;
    }
    let ids: Vec<_> = editor
        .documents()
        .filter(|doc| {
            !doc.is_modified()
                && doc.merge().is_none()
                && doc.path().is_some_and(|p| {
                    target.map_or_else(|| p.starts_with(root), |target| p == target)
                })
        })
        .map(|doc| doc.id())
        .collect();
    let providers = editor.diff_providers.clone();
    for id in ids {
        let view_id = editor.get_synced_view_id(id);
        let doc = editor.documents.get_mut(&id).unwrap();
        if let Err(error) = doc.reload(editor.tree.get_mut(view_id), &providers, true) {
            editor.set_error(format!("Git changed files; buffer reload failed: {error}"));
        }
    }
}
pub(super) fn apply(cx: &mut Context, retry_only: bool) {
    let (view, doc) = helix_view::current!(cx.editor);
    doc.append_changes_to_history(view);
    let id = doc.id();
    let version = doc.version();
    let state = doc.merge().unwrap();
    if state.busy {
        cx.editor.set_error("Merge result is being applied");
        return;
    }
    if !state.model.ready_to_apply() {
        cx.editor.set_error("Review every conflict before Apply");
        return;
    }
    if matches!(
        state.write_status,
        MergeWriteStatus::Staged | MergeWriteStatus::NeedsRefresh(_)
    ) {
        cx.editor
            .set_error("Draft is staged or stale; refresh explicitly before applying");
        return;
    }
    let root = state.data.version.root().to_owned();
    let path = root.join(&state.data.file.path);
    let retry = if retry_only {
        match &state.write_status {
            MergeWriteStatus::Retry {
                token,
                version: written_version,
                ..
            } if *written_version == version => Some(token.clone()),
            _ => {
                cx.editor
                    .set_error("Stage retry no longer matches this draft; refresh first");
                return;
            }
        }
    } else {
        if matches!(state.write_status, MergeWriteStatus::Retry { .. }) {
            cx.editor.set_error(
                "Result was written but not staged. Retry stage or refresh; do not overwrite it",
            );
            return;
        }
        None
    };
    let request = match state.data.format.unwrap().encode(doc.text().slice(..)) {
        Ok(contents) => ResolutionRequest {
            version: state.data.version.clone(),
            contents,
        },
        Err(error) => {
            cx.editor.set_error(error.to_string());
            return;
        }
    };
    if !super::super::trusted(cx.editor, &root) {
        cx.editor
            .set_error("Git tools require a trusted workspace (:trust)");
        return;
    }
    if cx
        .editor
        .documents()
        .any(|doc| doc.path() == Some(path.as_path()) && doc.is_modified())
    {
        cx.editor
            .set_error("An unsaved buffer for this file prevents Apply");
        return;
    }
    cx.editor
        .documents
        .get_mut(&id)
        .unwrap()
        .merge_mut()
        .unwrap()
        .busy = true;
    cx.jobs.add(
        Job::with_callback(async move {
            let result = async {
                let repository = GitRepository::open(root.clone(), true).await?;
                if let Some(retry) = retry {
                    repository.retry_stage(&retry).await
                } else {
                    repository.apply_resolution(&request).await
                }
            }
            .await;
            Ok(Callback::Editor(Box::new(move |editor| {
                let Some(doc) = editor.documents.get_mut(&id) else {
                    return;
                };
                let Some(state) = doc.merge_mut() else {
                    return;
                };
                state.busy = false;
                let staged = matches!(result, Ok(ApplyOutcome::Staged));
                let written = staged || matches!(result, Ok(ApplyOutcome::WrittenNotStaged { .. }));
                let message = match result {
                    Ok(ApplyOutcome::Staged) => {
                        state.write_status = MergeWriteStatus::Staged;
                        doc.reset_modified();
                        "Merge result applied and staged".to_owned()
                    }
                    Ok(ApplyOutcome::WrittenNotStaged { retry, message }) => {
                        state.write_status = MergeWriteStatus::Retry {
                            token: retry,
                            version,
                            message: message.clone(),
                        };
                        message
                    }
                    Ok(ApplyOutcome::NeedsRefresh { message }) => {
                        state.write_status = MergeWriteStatus::NeedsRefresh(message.clone());
                        message
                    }
                    Err(error) => {
                        let message = format!("Not applied: {error:#}");
                        state.write_status = MergeWriteStatus::NeedsRefresh(message.clone());
                        message
                    }
                };
                if staged {
                    editor.set_status(message);
                } else {
                    editor.set_error(message);
                }
                if written {
                    reload_clean(editor, &root, Some(&path));
                }
                crate::job::dispatch_blocking(move |_, compositor| {
                    if let Some(list) = compositor.find::<super::super::conflicts::ConflictList>() {
                        list.refresh_for(&root);
                    }
                });
            })))
        })
        .wait_before_exiting(),
    );
}
