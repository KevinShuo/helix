use std::{fs, path::Path, process::Command};

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
    }
    Ok(())
}
