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
    let Some(path) = helix_view::doc!(cx.editor).path().map(ToOwned::to_owned) else {
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
