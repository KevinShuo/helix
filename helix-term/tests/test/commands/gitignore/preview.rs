use super::*;
use std::{path::Path, process::Command};

const PREVIEW: &str = ":gitignore-preview<ret>";
const TITLE: &str = "Gitignore rule preview";

fn git(root: &Path, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    anyhow::ensure!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn repository() -> anyhow::Result<tempfile::TempDir> {
    let repo = tempfile::tempdir()?;
    git(repo.path(), &["init", "-q"])?;
    git(repo.path(), &["config", "core.excludesFile", ""])?;
    Ok(repo)
}

async fn preview(app: &mut Application, ready_text: &str) -> anyhow::Result<String> {
    keys(app, PREVIEW).await?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = screen(app);
        if output.contains(ready_text) {
            return Ok(output);
        }
        anyhow::ensure!(
            !app.editor.is_err() && Instant::now() < deadline,
            "Expected preview containing {ready_text:?}:\n{output}"
        );
        keys(app, "").await?;
    }
}

fn assert_result(output: &str, path: &str, state: &str, source: &str, pattern: &str) {
    let mut rows = output.lines();
    let row = rows.find(|line| line.contains(path)).unwrap_or_else(|| {
        panic!("Missing path {path:?} in preview:\n{output}");
    });
    assert!(row.contains(state), "{path} should be {state}:\n{output}");
    let rule = rows.next().unwrap();
    assert!(
        rule.contains(source) && rule.contains(pattern),
        "Expected {source} and {pattern} for {path}:\n{output}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsaved_nested_exception_reports_changed_and_overridden_files() -> anyhow::Result<()> {
    let repo = repository()?;
    let root = repo.path();
    let rules = root.join("sub/.gitignore");
    fs::create_dir_all(root.join("sub/deep"))?;
    fs::write(root.join(".gitignore"), "*.log\n")?;
    fs::write(&rules, "*.bak\n")?;
    fs::write(
        root.join("sub/deep/.gitignore"),
        "# override\nblocked.log\n",
    )?;
    for name in [
        "sub/changed.log",
        "sub/tracked.log",
        "sub/deep/blocked.log",
        "outside.log",
    ] {
        fs::write(root.join(name), "")?;
    }
    git(root, &["add", "-f", "--", "sub/tracked.log"])?;
    let index = fs::read(root.join(".git/index"))?;

    let mut app = AppBuilder::new().with_file(&rules, None).build()?;
    app.test_resize(120, 32).await?;
    keys(&mut app, "o!*.log<esc>").await?;
    let doc = doc!(app.editor);
    let id = doc.id();
    let view_id = helix_view::view!(app.editor).id;
    let selection = doc.selection(view_id).clone();
    assert_eq!(doc.text().to_string(), "*.bak\n!*.log\n");
    let output = preview(&mut app, "1 changed, 1 unchanged").await?;
    assert!(output.contains(TITLE), "{output}");
    assert!(
        output.lines().any(|row| row.contains("Rule")
            && row.contains("sub/.gitignore:2")
            && row.contains("!*.log")),
        "{output}"
    );
    assert_result(
        &output,
        "sub/changed.log",
        "Now included",
        "sub/.gitignore:2",
        "!*.log",
    );
    assert_result(
        &output,
        "sub/deep/blocked.log",
        "Still ignored",
        "sub/deep/.gitignore:2",
        "blocked.log",
    );
    assert!(
        !output.contains("tracked.log") && !output.contains("outside.log"),
        "{output}"
    );

    keys(&mut app, "<esc>").await?;
    assert!(!screen(&app).contains(TITLE));
    assert_eq!(app.editor.documents().count(), 1);
    let doc = doc!(app.editor);
    assert_eq!(doc.id(), id);
    assert_eq!(doc.selection(view_id), &selection);
    assert!(doc.is_modified());
    assert_eq!(doc.text().to_string(), "*.bak\n!*.log\n");
    assert_eq!(fs::read_to_string(&rules)?, "*.bak\n");
    assert_eq!(fs::read(root.join(".git/index"))?, index);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn positive_rule_results_scroll_by_line_and_page() -> anyhow::Result<()> {
    let repo = repository()?;
    let rules = repo.path().join(".gitignore");
    fs::write(&rules, "*.hxpreview\n")?;
    for n in 0..20 {
        fs::write(repo.path().join(format!("item-{n:02}.hxpreview")), "")?;
    }
    let mut app = AppBuilder::new().with_file(rules, None).build()?;
    app.test_resize(80, 12).await?;
    let first = preview(&mut app, "20 changed, 0 unchanged").await?;
    assert_result(
        &first,
        "item-00.hxpreview",
        "Now ignored",
        ".gitignore:1",
        "*.hxpreview",
    );
    assert!(!first.contains("item-19.hxpreview"));

    for (down, up) in [("j", "k"), ("<down>", "<up>"), ("<pagedown>", "<pageup>")] {
        keys(&mut app, down).await?;
        assert!(
            !screen(&app).contains("Rule .gitignore:1"),
            "{down} should scroll the report"
        );
        keys(&mut app, up).await?;
        assert!(
            screen(&app).contains("Rule .gitignore:1"),
            "{up} should return to the rule"
        );
    }
    for _ in 0..20 {
        keys(&mut app, "<pagedown>").await?;
        if screen(&app).contains("item-19.hxpreview") {
            break;
        }
    }
    let last = screen(&app);
    assert!(
        last.contains("item-19.hxpreview") && !last.contains("item-00.hxpreview"),
        "{last}"
    );
    keys(&mut app, "<esc>").await?;
    assert!(!doc!(app.editor).is_modified());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsaved_new_file_with_no_matches_can_be_dismissed() -> anyhow::Result<()> {
    let repo = repository()?;
    let rules = repo.path().join(".gitignore");
    let mut config = test_config();
    config.keys = toml::from_str("[normal]\nF3 = ':gitignore-preview'")?;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(&rules, None)
        .build()?;
    keys(&mut app, "i/missing<esc>").await?;
    preview(&mut app, "No matching untracked files").await?;
    keys(&mut app, "<C-c>").await?;
    assert!(!screen(&app).contains(TITLE));
    // Cancel at command submission, before pumping background scan callbacks.
    keys(&mut app, ":gitignore-preview<ret><esc>").await?;
    assert!(!screen(&app).contains(TITLE), "{}", screen(&app));
    keys(&mut app, "<F3>iX<esc>").await?;
    assert!(!screen(&app).contains(TITLE));
    assert_eq!(doc!(app.editor).text().to_string().trim_end(), "/missing");
    assert!(doc!(app.editor).is_modified());
    assert!(!rules.exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_comments_blank_lines_and_other_document_types() -> anyhow::Result<()> {
    let repo = repository()?;
    let rules = repo.path().join(".gitignore");
    fs::write(&rules, "# comment\n   \n!\n")?;
    let mut app = AppBuilder::new().with_file(&rules, None).build()?;
    app.test_resize(80, 12).await?;
    for movement in ["gg", "j", "j"] {
        keys(&mut app, movement).await?;
        let output = preview(&mut app, "Place the cursor on a .gitignore rule").await?;
        assert!(app.editor.is_err(), "{output}");
        assert!(!output.contains(TITLE));
    }
    assert!(!doc!(app.editor).is_modified());
    let other = repo.path().join("CODEOWNERS");
    fs::write(&other, "*.hxpreview\n")?;
    app.editor
        .open(&other, helix_view::editor::Action::Replace)?;
    let output = preview(&mut app, "Open a .gitignore file").await?;
    assert!(app.editor.is_err() && !output.contains(TITLE), "{output}");
    Ok(())
}
