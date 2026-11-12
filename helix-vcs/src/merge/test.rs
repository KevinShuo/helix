use super::*;
use crate::git_tools::MergeTextFormat;
use helix_core::{Rope, Transaction};

const BASE: &str = "before\nvalue=1\nafter\n";
const CURRENT: &str = "before\nvalue=2\nafter\n";
const INCOMING: &str = "before\nvalue=3\nafter\n";

fn fixture(base: &str, current: &str, incoming: &str, generated: &str) -> MergeCandidate {
    candidate_from_output(
        MergeSources::new(Rope::from(base), Rope::from(current), Rope::from(incoming)),
        generated,
        8,
        Some(generated),
    )
    .unwrap()
}

fn basic() -> MergeCandidate {
    fixture(BASE, CURRENT, INCOMING, "before\n<<<<<<<< helix-current\nvalue=2\n|||||||| helix-base\nvalue=1\n========\nvalue=3\n>>>>>>>> helix-incoming\nafter\n")
}

fn choose(model: &mut MergeModel, text: &mut Rope, block: usize, choice: ConflictChoice) {
    let tx = model.choice(block, choice, text.slice(..)).unwrap();
    assert!(tx.apply(text));
    model.on_change(tx.changes());
    model.mark_reviewed(Some(block));
}

#[test]
fn merge_choices_require_review_and_preserve_context() {
    for (choice, expected) in [
        (ConflictChoice::Current, CURRENT),
        (ConflictChoice::Incoming, INCOMING),
        (ConflictChoice::Both, "before\nvalue=2\nvalue=3\nafter\n"),
    ] {
        let mut model = MergeModel::from_candidate(basic());
        assert!(!model.ready_to_apply());
        let mut text = model.initial_result().clone();
        assert_eq!(text, BASE);
        choose(&mut model, &mut text, 0, choice);
        assert_eq!(text, expected);
        assert!(model.ready_to_apply());
    }
}

#[test]
fn merge_nonconflicting_changes_retained() {
    let generated = "left-new\nkeep\n<<<<<<<< helix-current\ncurrent\n|||||||| helix-base\nbase\n========\nincoming\n>>>>>>>> helix-incoming\nkeep\nright-new\n";
    let candidate = fixture(
        "left\nkeep\nbase\nkeep\nright\n",
        "left-new\nkeep\ncurrent\nkeep\nright\n",
        "left\nkeep\nincoming\nkeep\nright-new\n",
        generated,
    );
    let mut model = MergeModel::from_candidate(candidate);
    let mut text = model.initial_result().clone();
    choose(&mut model, &mut text, 0, ConflictChoice::Incoming);
    assert_eq!(text, "left-new\nkeep\nincoming\nkeep\nright-new\n");
    assert_eq!(
        model.sources().current(),
        "left-new\nkeep\ncurrent\nkeep\nright\n"
    );
    let maps = model.line_maps(text.slice(..));
    assert_eq!(maps.current[2], 2);
    assert_eq!(maps.incoming[4], 4);
}

#[test]
fn merge_aa_empty_base() {
    let candidate = fixture("", "current\n", "incoming\n", "<<<<<<<< helix-current\ncurrent\n|||||||| helix-base\n========\nincoming\n>>>>>>>> helix-incoming\n");
    let mut model = MergeModel::from_candidate(candidate);
    assert_eq!(model.initial_result(), "");
    let mut text = model.initial_result().clone();
    choose(&mut model, &mut text, 0, ConflictChoice::Both);
    assert_eq!(text, "current\nincoming\n");
}

#[test]
fn merge_literal_markers_not_conflicts() {
    let text = "<<<<<<< literal\n||||||| literal\n=======\n>>>>>>> literal\n";
    let model = MergeModel::from_candidate(fixture(text, text, text, text));
    assert_eq!(model.initial_result(), text);
    assert!(model.blocks().is_empty());
    assert!(model.ready_to_apply());
}

#[test]
fn merge_external_edits_preserved() {
    let mut candidate = basic();
    candidate.matches_working = false;
    let sources = candidate.sources.clone();
    let mut model = MergeModel::preserve_working(Rope::from("manual, no markers\n"), sources);
    let mut text = model.initial_result().clone();
    assert_eq!(model.mode(), MergeMode::WholeFileReview);
    assert_eq!(text, "manual, no markers\n");
    assert!(!model.ready_to_apply());
    assert!(model
        .choice(0, ConflictChoice::Current, text.slice(..))
        .is_err());
    model.mark_reviewed(None);
    assert!(model.ready_to_apply());
    let tx = Transaction::change(&text, [(0, 0, Some("edit".into()))].into_iter());
    let undo = tx.changes().invert(&text);
    tx.apply(&mut text);
    model.on_change(tx.changes());
    assert!(!model.ready_to_apply());
    model.mark_reviewed(None);
    undo.apply(&mut text);
    model.on_change(&undo);
    assert!(!model.ready_to_apply());
}

#[test]
fn merge_ranges_shift_after_insert() {
    let mut model = MergeModel::from_candidate(basic());
    let mut text = model.initial_result().clone();
    choose(&mut model, &mut text, 0, ConflictChoice::Current);
    let tx = Transaction::change(&text, [(0, 0, Some("π\n".into()))].into_iter());
    tx.apply(&mut text);
    model.on_change(tx.changes());
    assert_eq!(model.blocks()[0].result, 9..17);
    assert!(model.ready_to_apply());
    choose(&mut model, &mut text, 0, ConflictChoice::Incoming);
    assert_eq!(text, "π\nbefore\nvalue=3\nafter\n");
}

#[test]
fn merge_outside_replacements_do_not_swallow_context() {
    for (from, to, inserted, expected) in [
        (0, 7, "new\n", "new\nvalue=3\nafter\n"),
        (15, 21, "new\n", "before\nvalue=3\nnew\n"),
    ] {
        let mut model = MergeModel::from_candidate(basic());
        let mut text = model.initial_result().clone();
        choose(&mut model, &mut text, 0, ConflictChoice::Current);
        let tx = Transaction::change(&text, [(from, to, Some(inserted.into()))].into_iter());
        tx.apply(&mut text);
        model.on_change(tx.changes());
        choose(&mut model, &mut text, 0, ConflictChoice::Incoming);
        assert_eq!(text, expected);
    }
}

#[test]
fn merge_only_equivalent_standard_structures_match() {
    let sources = basic().sources;
    let generated = "before\n<<<<<<<< helix-current\nvalue=2\n|||||||| helix-base\nvalue=1\n========\nvalue=3\n>>>>>>>> helix-incoming\nafter\n";
    for (working, matches) in [
        ("before\n<<<<<<< ours\nvalue=2\n=======\nvalue=3\n>>>>>>> theirs\nafter\n", true),
        ("before\n<<<<<<< ours\nvalue=2\n||||||| ancestor\nvalue=1\n=======\nvalue=3\n>>>>>>> theirs\nafter\n", true),
        ("before\n<<<<<<< ours\nmanual edit\n=======\nvalue=3\n>>>>>>> theirs\nafter\n", false),
        ("manual before\n<<<<<<< ours\nvalue=2\n=======\nvalue=3\n>>>>>>> theirs\nafter\n", false),
    ] {
        let candidate = candidate_from_output(sources.clone(), generated, 8, Some(working)).unwrap();
        assert_eq!(candidate.matches_working, matches);
        if !matches {
            let model = MergeModel::from_candidate(candidate);
            assert_eq!(model.initial_result(), working);
            assert!(!model.ready_to_apply());
        }
    }
}

#[test]
fn merge_ambiguous_source_mapping_preserves_working() {
    let generated = "<<<<<<<< helix-current\nnot-current\n|||||||| helix-base\nbase\n========\nincoming\n>>>>>>>> helix-incoming\n";
    let candidate = fixture("base\n", "current\n", "incoming\n", generated);
    assert!(!candidate.matches_working);
    let model = MergeModel::from_candidate(candidate);
    assert_eq!(model.mode(), MergeMode::WholeFileReview);
    assert_eq!(model.initial_result(), generated);
}

#[test]
fn merge_unicode_line_separators_preserve_source_ranges() {
    let mut model = MergeModel::from_candidate(fixture(
        "a\u{2028}b\nbase\n", "a\u{2028}b\ncurrent\n", "a\u{2028}b\nincoming\n",
        "a\u{2028}b\n<<<<<<<< helix-current\ncurrent\n|||||||| helix-base\nbase\n========\nincoming\n>>>>>>>> helix-incoming\n",
    ));
    assert_eq!(model.mode(), MergeMode::Blockwise);
    let mut text = model.initial_result().clone();
    choose(&mut model, &mut text, 0, ConflictChoice::Incoming);
    assert_eq!(text, "a\u{2028}b\nincoming\n");
    let maps = model.line_maps(text.slice(..));
    assert!(maps.incoming.len() >= text.len_lines());
}

#[test]
fn merge_scroll_maps_clamp_deleted_lines_to_source() {
    let sources = MergeSources::new(Rope::from("one"), Rope::from("one"), Rope::from("one"));
    let result = Rope::from("different\nextra\nmore\n");
    let maps = sources.line_maps(result.slice(..));
    for map in [&maps.base, &maps.current, &maps.incoming] {
        assert!(map[..result.len_lines()].iter().all(|line| *line == 0));
    }
}

#[test]
fn merge_undo_redo_invalidates_review() {
    let mut model = MergeModel::from_candidate(basic());
    let mut text = model.initial_result().clone();
    let tx = model
        .choice(0, ConflictChoice::Current, text.slice(..))
        .unwrap();
    let undo = tx.changes().invert(&text);
    tx.apply(&mut text);
    model.on_change(tx.changes());
    model.mark_reviewed(Some(0));
    assert!(model.ready_to_apply());
    undo.apply(&mut text);
    model.on_change(&undo);
    assert!(!model.ready_to_apply());
    model.mark_reviewed(Some(0));
    tx.apply(&mut text);
    model.on_change(tx.changes());
    assert!(!model.ready_to_apply());
}

#[test]
fn merge_cross_block_edit_needs_full_review() {
    let generated = "<<<<<<<< helix-current\na\n|||||||| helix-base\nb\n========\nc\n>>>>>>>> helix-incoming\nmiddle\n<<<<<<<< helix-current\nx\n|||||||| helix-base\ny\n========\nz\n>>>>>>>> helix-incoming\n";
    let mut model = MergeModel::from_candidate(fixture(
        "b\nmiddle\ny\n",
        "a\nmiddle\nx\n",
        "c\nmiddle\nz\n",
        generated,
    ));
    let text = model.initial_result().clone();
    let tx = Transaction::change(&text, [(1, text.len_chars() - 1, None)].into_iter());
    model.on_change(tx.changes());
    assert_eq!(model.mode(), MergeMode::WholeFileReview);
    assert!(!model.ready_to_apply());
    model.mark_reviewed(Some(0));
    assert!(!model.ready_to_apply());
    model.mark_reviewed(None);
    assert!(model.ready_to_apply());
}

#[test]
fn merge_utf8_bom_crlf_roundtrip() {
    let bytes = b"\xef\xbb\xbfhello\r\nworld";
    let format = MergeTextFormat::detect(bytes).unwrap();
    let text = format.decode(bytes).unwrap();
    assert_eq!(text, "hello\nworld");
    assert_eq!(format.encode(text.slice(..)).unwrap(), bytes);
    assert!(format.decode(b"bad\xff").is_err());
    assert!(format
        .encode(Rope::from("mixed\r\nline\n").slice(..))
        .is_err());
}

#[test]
fn merge_result_limits_reject_expansion() {
    let format = MergeTextFormat {
        bom: false,
        crlf: true,
    };
    assert!(format
        .encode(Rope::from("a\n".repeat(50_001)).slice(..))
        .is_err());
    assert!(format
        .encode(Rope::from("a".repeat(2 * 1024 * 1024 + 1)).slice(..))
        .is_err());
    assert!(format.encode(Rope::from("\0").slice(..)).is_err());
    // LF text fits in 2 MiB; expanding its long lines to CRLF does not.
    let near_limit = format!("{}\n", "a".repeat(49)).repeat(41_943);
    assert!(near_limit.len() < 2 * 1024 * 1024);
    assert!(format.encode(Rope::from(near_limit).slice(..)).is_err());
}

#[test]
fn merge_rebuild_preserves_manual_work_until_explicit_and_refuses_ambiguous_ranges() {
    let mut candidate = basic();
    candidate.matches_working = false;
    candidate.working = Some(Rope::from("manual work"));
    assert_eq!(
        MergeModel::from_candidate(candidate.clone())
            .initial_result()
            .to_string(),
        "manual work"
    );
    let rebuilt = candidate.clone().rebuild().unwrap();
    assert_eq!(rebuilt.mode(), MergeMode::Blockwise);
    assert_eq!(rebuilt.initial_result().to_string(), BASE);
    assert!(!rebuilt.ready_to_apply());
    candidate.ranges_verified = false;
    assert!(candidate.rebuild().is_err());
}
