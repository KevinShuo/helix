use super::Context;
use crate::ui::git::{open, OpenAction};

pub(super) fn git_log(cx: &mut Context) {
    open(cx, OpenAction::Log);
}

pub(super) fn git_branches(cx: &mut Context) {
    open(cx, OpenAction::Branches);
}

pub(super) fn git_status(cx: &mut Context) {
    open(cx, OpenAction::Status);
}

pub(super) fn git_file_history(cx: &mut Context) {
    let doc = helix_view::doc!(cx.editor);
    let Some(path) = doc
        .merge()
        .map(|m| m.data.version.root().join(&m.data.file.path))
        .or_else(|| doc.path().map(ToOwned::to_owned))
    else {
        cx.editor
            .set_error("The current buffer has no file path for Git history");
        return;
    };
    open(cx, OpenAction::FileHistory(path));
}

pub(super) fn git_new_branch(cx: &mut Context) {
    open(cx, OpenAction::NewBranch);
}

pub(super) fn git_fetch(cx: &mut Context) {
    open(cx, OpenAction::Fetch);
}

pub(super) fn git_pull(cx: &mut Context) {
    open(cx, OpenAction::Pull);
}

pub(super) fn git_push(cx: &mut Context) {
    open(cx, OpenAction::Push);
}

pub(super) fn git_resolve_conflicts(cx: &mut Context) {
    open(cx, OpenAction::Conflicts);
}
pub(super) fn git_merge_menu(cx: &mut Context) {
    use crate::ui::git::merge::{merge_action, MergeUiAction as A};
    if cx.editor.active_merge().is_none() {
        cx.editor
            .set_error("Open a conflict file first (Space v r)");
        return;
    }
    cx.editor.set_status("Merge: n/p next/previous  l/r/b current/incoming/both  v base  m review  a apply  f files  L/R whole file  B rebuild  t retry  F refresh");
    cx.on_next_key(|cx, key| {
        use helix_view::input::KeyCode::Char;
        let action = match key.code {
            Char('n') => A::Next,
            Char('p') => A::Previous,
            Char('l') => A::UseCurrent,
            Char('r') => A::UseIncoming,
            Char('b') => A::KeepBoth,
            Char('v') => A::ShowBase,
            Char('m') => A::MarkReviewed,
            Char('a') => A::Apply,
            Char('f') => A::Files,
            Char('L') => A::AcceptCurrentFile,
            Char('R') => A::AcceptIncomingFile,
            Char('B') => A::Rebuild,
            Char('t') => A::RetryStage,
            Char('F') => A::Refresh,
            _ => return,
        };
        merge_action(cx, action);
    });
}
