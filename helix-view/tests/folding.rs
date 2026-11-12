use arc_swap::ArcSwap;
use helix_core::{smallvec, Range, Rope, Selection, Transaction};
use helix_view::{editor::Config, Document, ViewId};
use std::sync::Arc;

fn document(source: &str) -> (Document, ViewId, ViewId) {
    let loader = helix_core::config::default_lang_loader();
    let language = loader
        .language(loader.language_for_name("python").unwrap())
        .config()
        .clone();
    let mut doc = Document::from(
        Rope::from(source),
        None,
        Arc::new(ArcSwap::from_pointee(Config::default())),
        Arc::new(ArcSwap::from_pointee(loader)),
    );
    doc.set_language(Some(language), &helix_core::config::default_lang_loader());
    let mut ids = slotmap::SlotMap::<ViewId, ()>::with_key();
    let first = ids.insert(());
    let second = ids.insert(());
    doc.ensure_view_init(first);
    doc.ensure_view_init(second);
    (doc, first, second)
}

const SOURCE: &str = "# header\ndef outer():\n    def inner():\n        pass\n    inner()\n\ndef next():\n    pass\n";

#[test]
fn folding_is_view_only_and_nested_state_survives_parent() {
    let (mut doc, a, b) = document(SOURCE);
    let before = doc.text().clone();
    let version = doc.version();
    let candidates = doc.fold_candidates().to_vec();
    assert_eq!(candidates.len(), 3);
    assert!(doc.close_fold(a, candidates[1].clone(), true));
    assert!(doc.close_fold(a, candidates[0].clone(), true));
    assert_eq!(doc.folds(a), &[candidates[0].range.clone()]);
    assert!(doc.folds(b).is_empty());
    assert!(doc.unfold_at(a, candidates[0].declaration));
    assert_eq!(doc.folds(a), &[candidates[1].range.clone()]);
    assert_eq!(doc.text(), &before);
    assert_eq!(doc.version(), version);
    assert!(!doc.is_modified());
}

#[test]
fn explicit_folding_moves_only_primary_and_rejects_secondary_cursors() {
    let (mut doc, view, _) = document(SOURCE);
    let candidate = doc.fold_candidates()[0].clone();
    doc.set_selection(
        view,
        Selection::single(candidate.range.start + 5, candidate.range.start + 5),
    );
    assert!(doc.close_fold(view, candidate.clone(), true));
    assert_eq!(
        doc.selection(view).primary().cursor(doc.text().slice(..)),
        candidate.range.start
    );
    doc.clear_folds(view);
    doc.set_selection(
        view,
        Selection::new(
            smallvec![Range::point(0), Range::point(candidate.range.start + 5)],
            0,
        ),
    );
    assert!(!doc.close_fold(view, candidate, true));
    assert!(doc.folds(view).is_empty());
}

#[test]
fn fold_all_skips_cursors_and_selection_transactions_reveal_targets() {
    let (mut doc, view, _) = document(SOURCE);
    let candidates = doc.fold_candidates().to_vec();
    let target = candidates[1].range.start + 5;
    doc.set_selection(view, Selection::single(target, target));
    doc.fold_all(view);
    assert_eq!(doc.folds(view), &[candidates[2].range.clone()]);
    let target = candidates[2].range.start + 5;
    let transaction =
        Transaction::new(doc.text()).with_selection(Selection::single(target, target));
    doc.apply(&transaction, view);
    assert!(doc.folds(view).is_empty());
}

#[test]
fn edits_from_other_views_map_or_invalidate_folds() {
    let (mut doc, a, b) = document(SOURCE);
    let candidate = doc.fold_candidates()[0].clone();
    doc.close_fold(a, candidate.clone(), true);
    doc.apply(
        &Transaction::change(doc.text(), [(0, 0, Some("# new\n".into()))].into_iter()),
        b,
    );
    assert_eq!(
        doc.folds(a),
        &[candidate.range.start + 6..candidate.range.end + 6]
    );
    let at = doc.text().to_string().find("pass").unwrap();
    doc.apply(
        &Transaction::change(doc.text(), [(at, at, Some("x".into()))].into_iter()),
        b,
    );
    assert!(doc.folds(a).is_empty());
    assert_eq!(doc.fold_candidates().len(), 3);
}

#[test]
fn language_change_and_view_removal_clear_state() {
    let (mut doc, a, _) = document(SOURCE);
    doc.fold_all(a);
    assert!(!doc.folds(a).is_empty());
    doc.remove_view(a);
    doc.ensure_view_init(a);
    assert!(doc.folds(a).is_empty());
    doc.fold_all(a);
    doc.set_language(None, &helix_core::config::default_lang_loader());
    assert!(doc.fold_candidates().is_empty());
    assert!(doc.folds(a).is_empty());
}

#[test]
fn edits_outside_function_that_change_syntax_remove_stale_folds() {
    let (mut doc, view, other) = document(SOURCE);
    let candidate = doc.fold_candidates()[0].clone();
    doc.close_fold(view, candidate, true);
    let end = doc.text().len_chars();
    doc.apply(
        &Transaction::change(
            doc.text(),
            [
                (0, 0, Some("\"\"\"\n".into())),
                (end, end, Some("\"\"\"\n".into())),
            ]
            .into_iter(),
        ),
        other,
    );
    assert!(doc.fold_candidates().is_empty());
    assert!(doc.folds(view).is_empty());
}

#[test]
fn folds_clear_ai_previews_and_cancel_pending_requests() {
    let (mut doc, view, _) = document(SOURCE);
    doc.ai_completion = Some(helix_view::ai_completion::AiCompletion {
        view,
        version: doc.version(),
        annotation: helix_core::text_annotations::InlineAnnotation::new(0, "preview"),
    });
    let handle = doc.ai_completion_controller.restart();
    doc.fold_all(view);
    assert!(doc.ai_completion.is_none());
    assert!(handle.is_canceled());
    assert_eq!(doc.text().to_string(), SOURCE);
}

#[test]
fn folded_edit_undo_redo_never_restores_stale_hidden_ranges() {
    let (mut doc, view, _) = document(SOURCE);
    let mut v = helix_view::View::new(doc.id(), Default::default());
    v.id = view;
    let candidate = doc.fold_candidates()[0].clone();
    doc.close_fold(view, candidate.clone(), true);
    doc.apply(
        &Transaction::change(
            doc.text(),
            [(candidate.function.start, candidate.function.end, None)].into_iter(),
        ),
        view,
    );
    assert!(doc.folds(view).is_empty());
    assert!(doc.undo(&mut v));
    assert_eq!(doc.text().to_string(), SOURCE);
    assert!(doc.folds(view).is_empty());
    assert!(doc.redo(&mut v));
    assert!(!doc.text().to_string().contains("outer"));
    assert!(doc.folds(view).is_empty());
}
