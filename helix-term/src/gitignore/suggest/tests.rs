use super::*;
use std::process::Command;

fn options(base: &Path, input: &str, before: &str, after: &str) -> Vec<Candidate> {
    let cursor = input.chars().position(|c| c == '|').unwrap();
    let context = Context::parse(&input.replace('|', ""), cursor).unwrap();
    suggest(base, &context, before, after, true, true, true, || false)
}

fn git(base: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .current_dir(base)
        .args(args)
        .status()
        .unwrap()
        .success());
}

#[test]
fn edit_segment_uses_character_offsets_and_preserves_suffix() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("中文/build/cache")).unwrap();
    let context = Context::parse("!/中文/build/cache/", 7).unwrap();
    assert_eq!(
        (context.start, context.end, context.filter.as_str()),
        (5, 10, "bu")
    );
    let items = options(temp.path(), "!/中文/bu|ild/cache/", "", "");
    assert!(items
        .iter()
        .any(|c| c.replacement == "build" && c.label == "!/中文/build/cache/"));
}

#[test]
fn paths_and_templates_merge_without_changing_scope() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("Cargo.toml"), "").unwrap();
    let get = || options(root, "/tar|", "", "");
    let template = get();
    assert!(template[0].detail.contains("Cargo.toml"));
    fs::create_dir(root.join("target")).unwrap();
    let items = get();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].rank, template[0].rank);
    assert!(items[0].detail.contains("Local directory") && items[0].detail.contains("Rust"));
    let items = options(root, "tar|", "", "");
    assert!(items.iter().any(|c| c.label == "target/"));
    assert!(items.iter().any(|c| c.label == "/target/"));
}

#[test]
fn escaping_and_directory_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    for name in [
        "!keep",
        "#note",
        "trace[1].log",
        " leading space",
        ".env.example",
    ] {
        fs::write(temp.path().join(name), "").unwrap();
    }
    for (input, label) in [
        (r"\!k|", r"\!keep"),
        (r"\#n|", r"\#note"),
        ("tra|", r"trace\[1\].log"),
        (" l|", " leading space"),
        ("!.en|", "!.env.example"),
    ] {
        assert!(
            options(temp.path(), input, "", "")
                .iter()
                .any(|c| c.label == label),
            "{input}"
        );
    }
    assert!(Context::parse("#thing", 4).is_none());
    for input in ["../|", ".git/|", "**/|", "//|"] {
        assert!(options(temp.path(), input, "", "").is_empty(), "{input}");
    }
}

#[cfg(feature = "git")]
#[test]
fn extensions_count_untracked_files_and_show_an_example() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "--quiet"]);
    for name in ["a.log", "b.log", "tracked.log"] {
        fs::write(root.join(name), "").unwrap();
    }
    git(root, &["add", "tracked.log"]);
    let items = options(root, "*.|", "", "");
    let item = items.iter().find(|c| c.label == "*.log").unwrap();
    assert!(
        item.detail.contains("a.log") && item.detail.contains("2 samples"),
        "{item:?}"
    );
    assert!(!options(root, "tra|", "", "")
        .iter()
        .any(|c| c.label == "tracked.log"));
}

#[test]
fn project_fallback_and_manual_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "--quiet"]);
    fs::write(root.join("Cargo.toml"), "").unwrap();
    let nested = root.join("frontend");
    fs::create_dir(&nested).unwrap();
    let cx = Context::parse("", 0).unwrap();
    let templates = |manual| suggest(&nested, &cx, "", "", false, manual, true, || false);
    assert!(templates(true).iter().any(|c| c.label == "/target/"));
    fs::write(nested.join("package.json"), "").unwrap();
    let new = templates(true);
    assert!(new.iter().any(|c| c.label == "node_modules/"));
    assert!(!new.iter().any(|c| c.label == "/target/"));
    fs::write(nested.join("extra.log"), "").unwrap();
    assert!(!templates(true).iter().any(|c| c.label == "extra.log"));
}

#[cfg(feature = "git")]
#[test]
fn semantic_ranking_uses_unsaved_rules() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "--quiet"]);
    for name in [".env.example", ".env.other"] {
        fs::write(temp.path().join(name), "").unwrap();
    }
    let items = options(temp.path(), "!.env|", ".env.other\n", "");
    assert_eq!(items[0].label, "!.env.other");
    assert!(items[0].rank < items[1].rank);
    let items = options(temp.path(), "|", ".env.other\n", "");
    assert!(!items.iter().any(|c| c.label == ".env.other"));
}

#[test]
fn keep_start_syntax_when_cursor_is_before_it() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("other")).unwrap();
    for (input, start, end, label) in [
        ("|!name", 1, 5, "!other/"),
        ("|/dir/", 1, 4, "/other/"),
        ("!|/dir/", 2, 5, "!/other/"),
    ] {
        let cursor = input.chars().position(|c| c == '|').unwrap();
        let ctx = Context::parse(&input.replace('|', ""), cursor).unwrap();
        assert_eq!((ctx.start, ctx.end), (start, end), "{input}");
        assert!(
            options(temp.path(), input, "", "")
                .iter()
                .any(|c| c.label == label),
            "{input}"
        );
    }
}

#[test]
fn repeat_adapts_directory_and_anchor_to_the_destination() {
    let source = Context::parse("tar", 3).unwrap();
    let destination = Context::parse("/ta/keep", 3).unwrap();
    let pattern = source.repeat_pattern("/target/");
    assert_eq!(destination.replay_segment(&pattern), "target");
    let pattern = destination.repeat_pattern("build");
    assert_eq!(source.replay_segment(&pattern), "build/");
    let pattern = destination.repeat_pattern("!cache");
    assert_eq!(source.replay_segment(&pattern), r"\!cache/");
}

#[test]
fn template_without_sample_respects_known_rule_order() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "").unwrap();
    let cx = Context::parse("", 0).unwrap();
    for paths in [false, true] {
        let available = |before, after| {
            suggest(temp.path(), &cx, before, after, paths, true, true, || false)
                .iter()
                .any(|c| c.label == "/target/")
        };
        assert!(!available("/target/\r\n*.log\r\n# comment\r\n\r\n", ""));
        assert!(available("/target/\n!/*/\n", ""));
        assert!(!available("", "!/target/\n"));
        assert!(!available("", "/target/\n"));
        assert!(available("", "# /target/\n"));
    }
}

#[cfg(not(feature = "git"))]
#[test]
fn no_git_keeps_paths_with_unknown_effect() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("local.txt"), "").unwrap();
    let items = options(temp.path(), "loc|", "*.txt\n", "");
    assert_eq!(items[0].label, "local.txt");
    assert!(items[0].detail.contains("Effect unknown"));
}

#[test]
fn cancelled_requests_yield_no_partial_candidates_and_manual_refresh_reads_changes() {
    let temp = tempfile::tempdir().unwrap();
    let cx = Context::parse("item", 4).unwrap();
    let run = |manual, stop| suggest(temp.path(), &cx, "", "", true, manual, true, || stop);
    fs::write(temp.path().join("item-one"), "").unwrap();
    assert_eq!(run(true, false).len(), 1);
    fs::write(temp.path().join("item-two"), "").unwrap();
    assert_eq!(run(true, false).len(), 2);
    assert!(run(true, true).is_empty());
    let blank = Context::parse("", 0).unwrap();
    assert!(suggest(temp.path(), &blank, "", "", true, false, true, || false).is_empty());
}

#[test]
fn caps_samples_and_result_counts() {
    let temp = tempfile::tempdir().unwrap();
    // Keep project marker discovery small; sample the explicit subdirectory.
    let directory = temp.path().join("logs");
    fs::create_dir(&directory).unwrap();
    for index in 0..4098 {
        fs::write(directory.join(format!("entry{index:04}")), "").unwrap();
    }
    for index in 0..11 {
        fs::write(temp.path().join(format!("root{index}")), "").unwrap();
    }
    let items = options(temp.path(), "logs/entry|", "", "");
    assert_eq!(items.len(), 50);
    assert!(items
        .iter()
        .all(|c| c.detail.contains("Partial directory sample")));
    assert_eq!(options(temp.path(), "|", "", "").len(), 8);
}

#[cfg(unix)]
#[test]
fn skips_directory_symlinks_at_every_level() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::create_dir(external.path().join("inner")).unwrap();
    fs::write(external.path().join("inner/private.txt"), "").unwrap();
    symlink(external.path(), temp.path().join("linked")).unwrap();
    assert!(options(temp.path(), "linked/inner/pr|", "", "").is_empty());
}
