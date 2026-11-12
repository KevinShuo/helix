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
        cx.editor
            .set_status("No foldable function or class at cursor");
        return;
    };
    if !doc.close_fold(view_id, candidate, true) {
        cx.editor.set_status("Fold contains another cursor");
    }
}

pub(crate) fn toggle_fold_at_line(doc: &mut Document, view: ViewId, line: usize) -> Option<bool> {
    let candidate = doc
        .fold_candidates()
        .iter()
        .find(|fold| fold.header_line(doc.text().slice(..)) == line)?
        .clone();
    Some(if doc.folds(view).contains(&candidate.range) {
        doc.unfold_at(view, candidate.range.start)
    } else {
        doc.close_fold(view, candidate, true)
    })
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
