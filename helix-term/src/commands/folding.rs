use super::*;

pub(super) fn fold(cx: &mut Context) {
    change_fold(cx, false);
}

pub(super) fn toggle_fold(cx: &mut Context) {
    change_fold(cx, true);
}

fn change_fold(cx: &mut Context, toggle: bool) {
    let (view, doc) = current!(cx.editor);
    let view_id = view.id;
    let pos = doc
        .selection(view_id)
        .primary()
        .cursor(doc.text().slice(..));
    if toggle && doc.unfold_at(view_id, pos) {
        return;
    }
    let Some(candidate) = doc.fold_candidate_at(pos).cloned() else {
        cx.editor.set_status("No foldable function at cursor");
        return;
    };
    if !doc.close_fold(view_id, candidate, true) {
        cx.editor.set_status("Function contains another cursor");
    }
}

pub(crate) fn toggle_fold_at_line(doc: &mut Document, view: ViewId, line: usize) -> bool {
    let pos = doc.text().line_to_char(line);
    if doc.unfold_at(view, pos) {
        return true;
    }
    let candidate = doc
        .fold_candidates()
        .iter()
        .find(|fold| doc.text().char_to_line(fold.range.start) == line)
        .cloned();
    candidate.is_some_and(|candidate| doc.close_fold(view, candidate, true))
}

pub(super) fn unfold(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let pos = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    doc.unfold_at(view.id, pos);
}

pub(super) fn fold_all(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    doc.fold_all(view.id);
}

pub(super) fn unfold_all(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    doc.clear_folds(view.id);
}
