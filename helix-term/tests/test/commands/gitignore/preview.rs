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
    wait_for_preview(app, ready_text).await
}

async fn wait_for_preview(app: &mut Application, ready_text: &str) -> anyhow::Result<String> {
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
async fn completed_exception_can_be_filtered_and_opened_before_saving() -> anyhow::Result<()> {
    let repo = repository()?;
    let root = repo.path();
    let rules = root.join(".gitignore");
    fs::write(&rules, "# saved\n")?;
    fs::write(root.join("$a.tmp"), "")?;
    fs::write(root.join("$b.tmp"), "")?;
    let mut app = AppBuilder::new()
        .with_file(&rules, None)
        .with_input_text("*.tmp\n![$]a.tmp\n#[|\n]#")
        .build()?;
    app.test_resize(110, 24).await?;
    keys(&mut app, "i!$<C-x>").await?;
    super::completion::wait_for_menu(&mut app).await?;
    keys(&mut app, "<tab><ret><esc>").await?;
    assert_eq!(
        doc!(app.editor).text().to_string(),
        "*.tmp\n![$]a.tmp\n!$b.tmp\n"
    );
    keys(&mut app, &format!("{PREVIEW}<tab>/$b")).await?;
    let output = wait_for_preview(&mut app, "Path filter matches 1 of 1 changed files").await?;
    assert_result(
        &output,
        "> Now included  $b.tmp",
        "Now included",
        ".gitignore:3",
        "!$b.tmp",
    );
    keys(&mut app, "<ret>").await?;
    let (view, doc) = helix_view::current_ref!(app.editor);
    assert_eq!(doc.path().unwrap(), rules);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    assert_eq!(doc.text().slice(cursor..).to_string(), "!$b.tmp\n");
    assert!(doc.is_modified());
    assert_eq!(fs::read_to_string(&rules)?, "# saved\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_only_combines_with_path_and_jumps_to_the_reincluded_files_rule(
) -> anyhow::Result<()> {
    let repo = repository()?;
    let root = repo.path();
    let rules = root.join(".gitignore");
    let nested = root.join("build/scope/.gitignore");
    fs::create_dir_all(root.join("build/scope"))?;
    fs::write(&rules, "build/\n!build/\n")?;
    fs::write(&nested, "# local rules\n*.log\n!keep.log\n")?;
    git(root, &["add", "-f", "--", "build/scope/.gitignore"])?;
    for file in [
        "build/other.txt",
        "build/scope/keep.log",
        "build/scope/stay.log",
    ] {
        fs::write(root.join(file), "")?;
    }
    let mut app = AppBuilder::new().with_file(&rules, None).build()?;
    app.test_resize(110, 24).await?;
    keys(&mut app, &format!("j{PREVIEW}<tab>")).await?;
    let output = wait_for_preview(&mut app, "2 changed, 1 unchanged").await?;
    assert!(
        output.contains("other.txt") && output.contains("keep.log") && !output.contains("stay.log"),
        "{output}"
    );
    keys(&mut app, "/scope/").await?;
    let output = screen(&app);
    assert!(
        output.contains("Path filter matches 1 of 2 changed files."),
        "{output}"
    );
    assert!(
        output.contains("> Now included  build/scope/keep.log")
            && !output.contains("other.txt")
            && !output.contains("stay.log"),
        "{output}"
    );
    keys(&mut app, "<tab><down>").await?;
    assert!(screen(&app).contains("> Still ignored  build/scope/stay.log"));
    keys(&mut app, "<tab><ret>").await?;
    let (view, doc) = helix_view::current_ref!(app.editor);
    assert_eq!(doc.path().unwrap(), nested);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    assert_eq!(doc.text().slice(cursor..).to_string(), "!keep.log\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_only_keeps_newly_ignored_files_when_the_path_filter_is_cleared(
) -> anyhow::Result<()> {
    let repo = repository()?;
    let root = repo.path();
    let rules = root.join(".gitignore");
    fs::write(&rules, "*.log\n!skip.log\nalways.log\n")?;
    for file in ["new.log", "skip.log", "always.log"] {
        fs::write(root.join(file), "")?;
    }
    let mut app = AppBuilder::new().with_file(&rules, None).build()?;
    app.test_resize(110, 24).await?;
    preview(&mut app, "1 changed, 2 unchanged").await?;
    keys(&mut app, "/skip<tab><ret>").await?;
    let output = screen(&app);
    assert!(
        output.contains(TITLE) && output.contains("No changed files match the path filter"),
        "{output}"
    );
    keys(&mut app, "<esc>").await?;
    let output = screen(&app);
    assert!(
        output.contains("> Now ignored  new.log")
            && !output.contains("skip.log")
            && !output.contains("always.log"),
        "{output}"
    );
    keys(&mut app, "<tab>").await?;
    let output = screen(&app);
    assert!(
        output.contains("Still included  skip.log") && output.contains("Still ignored  always.log"),
        "{output}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_filter_jumps_to_the_selected_visible_files_rule() -> anyhow::Result<()> {
    let repo = repository()?;
    let root = repo.path();
    let rules = root.join(".gitignore");
    fs::write(&rules, "*.log\n")?;
    fs::write(root.join("elsewhere.log"), "")?;
    let folder = root.join("项目 Key");
    fs::create_dir_all(folder.join("nested"))?;
    let winner = folder.join("nested/.gitignore");
    fs::write(&winner, "# child\n!keep.log\n")?;
    fs::write(folder.join("also.log"), "")?;
    fs::write(folder.join("nested/keep.log"), "")?;
    let mut app = AppBuilder::new().with_file(&rules, None).build()?;
    app.test_resize(110, 26).await?;
    // Submit the query in the same input burst as the command, before scan callbacks run.
    keys(&mut app, &format!("{PREVIEW}/项目 key/")).await?;
    let filtered = wait_for_preview(&mut app, "Path filter matches 2 of 3 files").await?;
    assert!(!filtered.contains("elsewhere.log"), "{filtered}");
    assert!(
        filtered
            .lines()
            .any(|row| row.contains("> Now ignored") && row.contains("Key/also.log")),
        "{filtered}"
    );
    // Narrow and broaden the query after moving the selection.
    keys(&mut app, "<down>n<backspace>").await?;
    assert!(screen(&app)
        .lines()
        .any(|row| row.contains("> Still included") && row.contains("Key/nested/keep.log")));
    keys(&mut app, "<ret>").await?;
    let (view, doc) = helix_view::current_ref!(app.editor);
    assert_eq!(doc.path().unwrap(), winner);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    assert_eq!(doc.text().slice(cursor..).to_string(), "!keep.log\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_filter_recovers_from_no_matches_and_searches_beyond_the_sample() -> anyhow::Result<()>
{
    let repo = repository()?;
    let root = repo.path();
    let rules = root.join(".gitignore");
    fs::write(&rules, "*.log\n")?;
    for n in 0..104 {
        fs::write(root.join(format!("file-{n:03}.log")), "")?;
    }
    let mut app = AppBuilder::new().with_file(&rules, None).build()?;
    app.test_resize(100, 20).await?;
    preview(&mut app, "Showing 100 of 104").await?;
    // Filtering uses the original snapshot even when the working tree changes.
    fs::remove_file(root.join("file-103.log"))?;
    keys(&mut app, "/gitignore<ret>").await?;
    assert!(
        screen(&app).contains("No files match the path filter"),
        "{}",
        screen(&app)
    );
    assert!(screen(&app).contains(TITLE));
    keys(&mut app, "<C-u>103x<backspace>").await?;
    assert!(screen(&app).contains("> Now ignored  file-103.log"));
    keys(&mut app, "<esc>").await?;
    assert!(screen(&app).contains("Showing 100 of 104"));
    keys(&mut app, "<esc>").await?;
    assert!(!screen(&app).contains(TITLE));
    assert!(!doc!(app.editor).is_modified());
    Ok(())
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

    preview(&mut app, "1 changed, 1 unchanged").await?;
    keys(&mut app, "<down><ret>").await?;
    assert!(!screen(&app).contains(TITLE));
    let doc = doc!(app.editor);
    assert_eq!(doc.path().unwrap(), root.join("sub/deep/.gitignore"));
    assert_eq!(
        doc.selection(view_id)
            .primary()
            .cursor(doc.text().slice(..)),
        11
    );
    keys(&mut app, "<C-o>").await?;
    assert_eq!(doc!(app.editor).id(), id);
    assert_eq!(doc!(app.editor).selection(view_id), &selection);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_uses_later_rule_in_the_unsaved_buffer() -> anyhow::Result<()> {
    let repo = repository()?;
    let path = repo.path().join(".gitignore");
    fs::write(repo.path().join("keep.log"), "")?;
    let mut app = AppBuilder::new().with_file(&path, None).build()?;
    keys(&mut app, "i*.log<ret>!keep.log<esc>gg").await?;
    preview(&mut app, "0 changed, 1 unchanged").await?;
    keys(&mut app, "<tab><ret>").await?;
    assert!(screen(&app).contains("No files changed ignore status."));
    keys(&mut app, "<tab><ret>").await?;
    assert!(!screen(&app).contains(TITLE));
    let (view, doc) = helix_view::current_ref!(app.editor);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    assert_eq!(
        doc.text().slice(cursor..).to_string().trim_end(),
        "!keep.log"
    );
    assert!(doc.is_modified() && !path.exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_on_default_inclusion_keeps_the_preview_open() -> anyhow::Result<()> {
    let repo = repository()?;
    let path = repo.path().join(".gitignore");
    fs::create_dir(repo.path().join("out"))?;
    fs::write(repo.path().join("out/a"), "")?;
    fs::write(&path, "out/\n!out/\n")?;
    let mut app = AppBuilder::new().with_file(&path, None).build()?;
    keys(&mut app, "j").await?;
    preview(&mut app, "Included by default").await?;
    keys(&mut app, "<ret>").await?;
    assert!(screen(&app).contains(TITLE));
    assert!(app
        .editor
        .get_status()
        .unwrap()
        .0
        .contains("no rule to jump to"));
    assert_eq!(app.editor.documents().count(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_opens_the_external_parent_rule_and_handles_a_missing_source() -> anyhow::Result<()> {
    let repo = repository()?;
    let root = repo.path();
    let config = tempfile::tempdir()?;
    let exclude = config.path().join("global 忽略");
    let rules = "# first\u{2028}still first\r\ncache/\r\n";
    fs::write(&exclude, rules)?;
    git(
        root,
        &["config", "core.excludesFile", exclude.to_str().unwrap()],
    )?;
    fs::create_dir(root.join("cache"))?;
    fs::write(root.join("cache/debug.log"), "")?;
    let path = root.join(".gitignore");
    fs::write(&path, "*.log\n")?;
    let mut app = AppBuilder::new().with_file(&path, None).build()?;
    preview(&mut app, "0 changed, 1 unchanged").await?;
    fs::remove_file(&exclude)?;
    keys(&mut app, "<ret>").await?;
    assert!(app.editor.is_err() && screen(&app).contains(TITLE));
    assert_eq!(app.editor.documents().count(), 1);
    fs::write(&exclude, rules)?;
    keys(&mut app, "<ret>").await?;
    let (view, doc) = helix_view::current_ref!(app.editor);
    assert_eq!(doc.path().unwrap(), exclude);
    let pos = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    assert_eq!(doc.text().slice(pos..).to_string(), "cache/\r\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_preserves_a_modified_destination_with_a_stale_rule_line() -> anyhow::Result<()> {
    let repo = repository()?;
    let root = repo.path();
    let path = root.join(".gitignore");
    let child = root.join("child/.gitignore");
    fs::create_dir(root.join("child"))?;
    fs::write(&child, "# note\n!keep.log\n")?;
    fs::write(&path, "*.log\n")?;
    fs::write(root.join("child/keep.log"), "")?;
    let mut app = AppBuilder::new().with_file(&child, None).build()?;
    keys(&mut app, "O# draft<esc>").await?;
    app.editor
        .open(&path, helix_view::editor::Action::Replace)?;
    preview(&mut app, "0 changed, 1 unchanged").await?;
    keys(&mut app, "<ret>").await?;
    assert!(app.editor.is_err() && screen(&app).contains(TITLE));
    assert_eq!(doc!(app.editor).path().unwrap(), path);
    assert!(app.editor.document_by_path(&child).unwrap().is_modified());
    assert_eq!(fs::read_to_string(child)?, "# note\n!keep.log\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_follows_a_rule_whose_non_utf8_bytes_are_escaped_in_the_preview() -> anyhow::Result<()>
{
    let repo = repository()?;
    let root = repo.path();
    let path = root.join(".gitignore");
    let child = root.join("cache/.gitignore");
    fs::create_dir(root.join("cache"))?;
    fs::write(&path, "*.log\n")?;
    fs::write(&child, b"# local\n![x\xff].log\n")?;
    fs::write(root.join("cache/x.log"), "")?;
    let mut app = AppBuilder::new().with_file(path, None).build()?;
    preview(&mut app, "0 changed, 1 unchanged").await?;
    keys(&mut app, "<ret>").await?;
    let (view, doc) = helix_view::current_ref!(app.editor);
    assert_eq!(doc.path().unwrap(), child);
    assert_eq!(
        doc.selection(view.id)
            .primary()
            .cursor(doc.text().slice(..)),
        8
    );
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
    assert!(first
        .lines()
        .any(|row| row.contains("> Now ignored  item-00.hxpreview")));

    for (down, up) in [("j", "k"), ("<down>", "<up>")] {
        keys(&mut app, down).await?;
        assert!(screen(&app).contains("> Now ignored  item-01.hxpreview"));
        keys(&mut app, up).await?;
        assert!(screen(&app).contains("> Now ignored  item-00.hxpreview"));
    }
    keys(&mut app, "<pagedown>").await?;
    assert!(!screen(&app).contains("Rule .gitignore:1"));
    keys(&mut app, "<pageup>").await?;
    assert!(screen(&app).contains("> Now ignored  item-00.hxpreview"));
    for _ in 0..20 {
        keys(&mut app, "<pagedown>").await?;
        if screen(&app).contains("> Now ignored  item-19.hxpreview") {
            break;
        }
    }
    let last = screen(&app);
    assert!(
        last.contains("item-19.hxpreview") && !last.contains("item-00.hxpreview"),
        "{last}"
    );
    assert!(last
        .lines()
        .any(|row| row.contains("> Now ignored  item-19.hxpreview")));
    keys(&mut app, "<home>j").await?;
    app.test_resize(28, 9).await?;
    assert!(
        screen(&app).contains("> Now ignored  item-01"),
        "{}",
        screen(&app)
    );
    keys(&mut app, "<end><ret>").await?;
    assert!(!screen(&app).contains(TITLE));
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
    keys(&mut app, "<down><ret>").await?;
    assert!(screen(&app).contains("No matching untracked files"));
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
