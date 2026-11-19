use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use super::explain::{Decision, Matcher};
use super::{Effect, Sources};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn ignored(dir: &Path, sample: &Path) -> bool {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["check-ignore", "-q", "--"])
        .arg(sample)
        .output()
        .unwrap();
    match output.status.code() {
        Some(0) => true,
        Some(1) => false,
        _ => panic!("{}", String::from_utf8_lossy(&output.stderr)),
    }
}

#[test]
fn overlays_the_buffer_and_compares_effects_like_git() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.excludesFile", ""]);
    git(root, &["config", "core.ignorecase", "true"]);
    fs::create_dir(root.join("build"))?;
    for (before, rule, after, sample, expected) in [
        (
            "*.log\n!keep.log\n",
            "keep.log",
            "",
            "keep.log",
            Effect::Changed,
        ),
        (
            "",
            "keep.log",
            "!keep.log\n",
            "keep.log",
            Effect::Overridden,
        ),
        ("*.LOG\n", "keep.log", "", "keep.log", Effect::Unchanged),
        (
            "build/\n",
            "!build/keep.txt",
            "",
            "build/keep.txt",
            Effect::ParentExcluded,
        ),
        (
            "build/*\n",
            "!build/keep.txt",
            "",
            "build/keep.txt",
            Effect::Changed,
        ),
        ("", r"/trace\[1\].log", "", "trace[1].log", Effect::Changed),
    ] {
        let sample = root.join(sample);
        fs::write(&sample, "")?;
        let file = root.join(".gitignore");
        fs::write(&file, "*\n")?;
        let sources = Sources::load(root, sample.parent().unwrap(), true);
        let effect = sources.rules(before, after).effect(rule, &sample, false);
        assert_eq!(effect, expected, "{rule}");
        fs::write(&file, format!("{before}{after}"))?;
        let old = ignored(root, &sample);
        fs::write(&file, format!("{before}{rule}\n{after}"))?;
        assert_eq!(
            effect == Effect::Changed,
            old != ignored(root, &sample),
            "{rule}"
        );
    }
    Ok(())
}

#[test]
fn repository_layers_index_and_linked_worktree() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let worktree = temp.path().join("linked");
    fs::create_dir(&repo)?;
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "test",
            "--allow-empty",
        ],
    );
    git(
        &repo,
        &["worktree", "add", "--detach", worktree.to_str().unwrap()],
    );
    let excludes = temp.path().join("global-ignore");
    fs::write(&excludes, "*.global\n")?;
    git(
        &worktree,
        &["config", "core.excludesFile", excludes.to_str().unwrap()],
    );
    fs::write(repo.join(".git/info/exclude"), "*.info\n")?;
    let base = worktree.join("app");
    let dir = base.join("out");
    fs::create_dir_all(&dir)?;
    fs::write(worktree.join(".gitignore"), "*.ancestor\n")?;
    fs::write(dir.join(".gitignore"), "!local.tmp\n")?;
    fs::write(dir.join("tracked"), "")?;
    git(&worktree, &["add", "app/out/tracked"]);
    let sources = Sources::load(&base, &dir, true);
    let rules = sources.rules("", "");
    assert_eq!(rules.tracked(&dir.join("tracked")), Some(true));
    assert_eq!(rules.tracked(&dir.join("untracked")), Some(false));
    for ext in ["global", "info", "ancestor"] {
        let sample = dir.join(format!("file.{ext}"));
        fs::write(&sample, "")?;
        assert!(ignored(&worktree, &sample));
        assert_eq!(
            rules.effect(&format!("!out/file.{ext}"), &sample, false),
            Effect::Changed
        );
    }
    assert_eq!(
        rules.effect("out/*.tmp", &dir.join("local.tmp"), false),
        Effect::Overridden
    );
    let (matcher, parent) = Matcher::load(&base, "# linked\n!out/*.info\n", 2, true, &|| false)?;
    let child = matcher.enter(&parent, &dir)?;
    let sample = dir.join("file.info");
    let result = matcher.evaluate(&child, &sample)?;
    assert_eq!(
        result.before.unwrap().source,
        repo.join(".git/info/exclude")
    );
    let winner = result.after.unwrap();
    assert_eq!(winner.source, Path::new("app/.gitignore"));
    assert_eq!(
        (winner.line, winner.pattern.as_str(), winner.ignored),
        (2, "!out/*.info", false)
    );
    Ok(())
}

#[test]
fn unavailable_index_or_repository_reports_unknown() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let sample = root.join("example");
    for corrupt_index in [false, true] {
        if corrupt_index {
            git(root, &["init", "-q"]);
            fs::write(root.join(".git/index"), "truncated")?;
        }
        let sources = Sources::load(root, root, true);
        let rules = sources.rules("", "");
        assert_eq!(rules.tracked(&sample), None);
        assert_eq!(rules.effect("example", &sample, false), Effect::Unknown);
        assert!(Matcher::load(root, "example", 1, true, &|| false).is_err());
    }
    Ok(())
}

fn git_decision(root: &Path, path: &Path) -> Option<Decision> {
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["check-ignore", "--no-index", "-v", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(
            super::git_path(path.strip_prefix(root).unwrap())
                .unwrap()
                .as_ref(),
        )
        .unwrap();
    input.write_all(&[0]).unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    if output.status.code() == Some(1) && output.stdout.is_empty() {
        return None;
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fields: Vec<_> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .split('\0')
        .collect();
    let pattern = fields[2].to_owned();
    Some(Decision {
        source: fields[0].into(),
        line: fields[1].parse().unwrap(),
        ignored: !pattern.starts_with('!'),
        pattern,
    })
}

#[test]
fn preview_explains_parents_negations_and_shadowing_like_git() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.excludesFile", ""]);
    git(root, &["config", "core.ignorecase", "true"]);
    let child = root.join("out");
    let sample = child.join("keep.log");
    fs::create_dir(&child)?;
    fs::write(&sample, "")?;
    for (buffer, line, deeper) in [
        // Ancestor and leaf both match; Git reports the first excluded ancestor.
        ("out/\nout/keep.log\n", 1, ""),
        ("out/\nout/keep.log\n", 2, ""),
        ("out/\n!out/keep.log\n", 2, ""),
        ("out/\n!out/\n", 2, ""),
        ("out/*\n!out/keep.log\n", 2, ""),
        (
            "# CRLF and original line numbers\r\n*.LOG\r\n\r\n!/out/keep.log\r\n",
            2,
            "",
        ),
        ("# earlier\n*.log\n", 2, "# later\n!/keep.log\n"),
        ("*.log\n*.LOG\n", 2, ""),
        ("out/\n", 1, "!keep.log\n"),
    ] {
        let document = root.join(".gitignore");
        fs::write(child.join(".gitignore"), deeper)?;
        fs::write(&document, "# unsaved overlay differs from disk\n")?;
        let (matcher, parent) = Matcher::load(root, buffer, line, true, &|| false)?;
        let dir = matcher.enter(&parent, &child)?;
        let result = matcher.evaluate(&dir, &sample)?;
        assert!(result.selected, "{buffer:?}:{line}");
        // A blank placeholder keeps later provenance line numbers stable.
        let without: String = buffer
            .split_inclusive('\n')
            .enumerate()
            .map(|(i, s)| if i + 1 == line { "\n" } else { s })
            .collect();
        fs::write(&document, without)?;
        assert_eq!(
            result.before.as_deref(),
            git_decision(root, &sample).as_ref(),
            "{buffer:?}:{line}, before"
        );
        fs::write(&document, buffer)?;
        assert_eq!(
            result.after.as_deref(),
            git_decision(root, &sample).as_ref(),
            "{buffer:?}:{line}, after"
        );
    }
    Ok(())
}

#[test]
fn preview_attributes_global_info_ancestor_and_nested_sources() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    let base = root.join("app");
    let dir = base.join("out");
    fs::create_dir_all(&dir)?;
    git(&root, &["init", "-q"]);
    let global = temp.path().join("global");
    fs::write(&global, "*.global\n*.info\n")?;
    git(
        &root,
        &["config", "core.excludesFile", global.to_str().unwrap()],
    );
    fs::write(root.join(".git/info/exclude"), "# higher\n*.info\n")?;
    fs::write(root.join(".gitignore"), "/app/out/*.ancestor\n")?;
    fs::write(dir.join(".gitignore"), "# deeper\n!important.nested\n")?;
    let buffer = "# unescape only for matching\n/out/*\n!\\#escaped\n";
    let (matcher, parent) = Matcher::load(&base, buffer, 2, true, &|| false)?;
    let state = matcher.enter(&parent, &dir)?;
    for name in [
        "x.global",
        "x.info",
        "x.ancestor",
        "important.nested",
        "#escaped",
    ] {
        let path = dir.join(name);
        fs::write(&path, "")?;
        let result = matcher.evaluate(&state, &path)?;
        assert!(result.selected);
        fs::write(base.join(".gitignore"), "#\n\n!\\#escaped\n")?;
        assert_eq!(
            result.before.as_deref(),
            git_decision(&root, &path).as_ref(),
            "{name} before"
        );
        fs::write(base.join(".gitignore"), buffer)?;
        assert_eq!(
            result.after.as_deref(),
            git_decision(&root, &path).as_ref(),
            "{name} after"
        );
    }
    Ok(())
}

#[test]
fn preview_matches_untracked_children_of_a_replaced_index_file() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.excludesFile", ""]);
    let replaced = root.join("entry");
    fs::write(&replaced, "tracked")?;
    git(root, &["add", "entry"]);
    fs::remove_file(&replaced)?;
    fs::create_dir(&replaced)?;
    fs::write(replaced.join("child"), "untracked")?;
    let untracked = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .output()?;
    assert!(untracked.status.success());
    assert_eq!(untracked.stdout, b"entry/child\0");
    let report = super::preview::scan(root, "entry/", 0, true, || false)?;
    assert!(!report.partial);
    assert_eq!((report.changed, report.unchanged), (1, 0));
    assert_eq!(report.entries[0].path, Path::new("entry/child"));
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000,1111111111111111111111111111111111111111,submodule",
        ],
    );
    let inside = root.join("submodule/src");
    fs::create_dir_all(&inside)?;
    assert!(super::preview::scan(&inside, "*", 0, true, || false).is_err());
    Ok(())
}

#[test]
fn preview_negates_literal_dollar_names_like_git() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.excludesFile", ""]);
    let sample = root.join("$cache.tmp");
    fs::write(&sample, "")?;
    for (buffer, line) in [
        ("# comment\n\\$cache.tmp\n!$cache.tmp\n", 3),
        ("\u{feff}!$cache.tmp\r\n", 1),
    ] {
        let (matcher, parent) = Matcher::load(root, buffer, line, true, &|| false)?;
        let result = matcher.evaluate(&parent, &sample)?;
        assert!(result.selected);
        fs::write(root.join(".gitignore"), buffer)?;
        assert_eq!(
            result.after.as_deref(),
            git_decision(root, &sample).as_ref()
        );
    }
    Ok(())
}

#[test]
fn preview_quotes_non_utf8_rule_bytes() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "core.excludesFile", ""]);
    let child = root.join("out");
    fs::create_dir(&child)?;
    fs::write(child.join(".gitignore"), b"# local\n![x\xff].log\n")?;
    let (matcher, parent) = Matcher::load(root, "*.log", 1, true, &|| false)?;
    let parent = matcher.enter(&parent, &child)?;
    let decision = matcher
        .evaluate(&parent, &child.join("x.log"))?
        .after
        .unwrap();
    assert_eq!(decision.pattern, r#""![x\xff].log""#);
    assert_eq!(decision.line, 2);
    assert!(!decision.ignored);
    Ok(())
}
