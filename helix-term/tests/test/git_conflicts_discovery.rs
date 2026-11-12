//! Discovery through actual Git commands and terminal events.
use super::*;

async fn focus_gained(app: &mut Application) {
    #[cfg(windows)]
    let event = crossterm::event::Event::FocusGained;
    #[cfg(not(windows))]
    let event = termina::event::Event::FocusGained;
    app.handle_terminal_events(Ok(event)).await;
}

async fn finish_scan(app: &mut Application) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    while app.editor.git_conflict_scan.is_running() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(30)).await;
        keys(app, "").await?;
    }
    assert!(
        !app.editor.git_conflict_scan.is_running(),
        "Conflict scan did not finish"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_focus_scan_no_focus_steal() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 30).await?;
    let original = current_ref!(app.editor).1.id();
    focus_gained(&mut app).await;
    assert!(app.editor.git_conflict_scan.is_running());
    finish_scan(&mut app).await?;
    assert!(app
        .editor
        .get_status()
        .is_some_and(|(text, _)| text.contains("2 conflicted files")));
    assert_eq!(current_ref!(app.editor).1.id(), original);
    assert!(app.editor.active_merge().is_none());
    assert!(!screen(&app).contains("Enter resolve"));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

pub(super) async fn wait_screen(app: &mut Application, expected: &str) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !screen(app).contains(expected) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(30)).await;
        keys(app, "").await?;
    }
    assert!(
        screen(app).contains(expected),
        "Missing {expected}:\n{}",
        screen(app)
    );
    Ok(())
}

async fn prepare_merge(app: &mut Application) -> anyhow::Result<()> {
    keys(app, "<space>vb").await?;
    wait_screen(app, "topic").await?;
    // All branches, HEAD, Local, main, topic.
    keys(app, "jjjjam").await?;
    wait_screen(app, "Source: refs/heads/topic").await
}

async fn wait_operation(app: &mut Application, root: &Path) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !root.join(".git/MERGE_HEAD").exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(30)).await;
        keys(app, "").await?;
    }
    assert!(
        root.join(".git/MERGE_HEAD").exists(),
        "Merge did not complete"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_merge_opens_list() -> anyhow::Result<()> {
    let repo = repository(false);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    let hidden = app.editor.open(
        &repo.path().join("second.txt"),
        helix_view::editor::Action::Replace,
    )?;
    app.editor.open(
        &repo.path().join("file.txt"),
        helix_view::editor::Action::Replace,
    )?;
    app.test_resize(160, 40).await?;
    prepare_merge(&mut app).await?;
    keys(&mut app, "y").await?;
    wait_screen(&mut app, "Enter resolve").await?;
    assert!(screen(&app).contains("Conflicts (2)"));
    assert_eq!(
        checked_git(repo.path(), &["ls-files", "-u"])
            .lines()
            .count(),
        6
    );
    // Clean ordinary buffers are refreshed after Git changes their files.
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        fs::read_to_string(repo.path().join("file.txt"))?
    );
    assert_eq!(
        app.editor.documents[&hidden].text().to_string(),
        fs::read_to_string(repo.path().join("second.txt"))?
    );
    keys(&mut app, "<ret>").await?;
    wait_merge(&mut app).await?;
    assert!(screen(&app).contains("Current"));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_non_conflict_error_stays_error() -> anyhow::Result<()> {
    let repo = repository(false);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    prepare_merge(&mut app).await?;
    // The selected ref disappears between preview and execution: an ordinary Git error.
    checked_git(repo.path(), &["branch", "-D", "topic"]);
    keys(&mut app, "y").await?;
    wait_screen(&mut app, "not something we can merge").await?;
    assert!(app.editor.is_err());
    assert!(!screen(&app).contains("Enter resolve"));
    assert!(checked_git(repo.path(), &["ls-files", "-u"]).is_empty());
    keys(&mut app, "q:qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_closed_panel_stays_closed_and_operation_finishes() -> anyhow::Result<()> {
    let repo = repository(false);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    prepare_merge(&mut app).await?;
    // Both keys are delivered before draining jobs, making this lifecycle race deterministic.
    keys(&mut app, "yq").await?;
    wait_operation(&mut app, repo.path()).await?;
    wait_screen(&mut app, "conflicted files").await?;
    assert!(!screen(&app).contains("Enter resolve"));
    assert!(!screen(&app).contains("Git · Log"));
    assert!(app.editor.active_merge().is_none());
    assert_eq!(
        checked_git(repo.path(), &["ls-files", "-u"])
            .lines()
            .count(),
        6
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_switch_repository_ignores_old_operation_result() -> anyhow::Result<()> {
    let conflicted = repository(false);
    let clean = repository(false);
    let mut app = AppBuilder::new()
        .with_file(conflicted.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    prepare_merge(&mut app).await?;
    keys(&mut app, "yq").await?;
    let target = app.editor.open(
        &clean.path().join("file.txt"),
        helix_view::editor::Action::Replace,
    )?;
    keys(&mut app, "<space>vb").await?;
    wait_screen(&mut app, "topic").await?;
    wait_operation(&mut app, conflicted.path()).await?;
    assert_eq!(current_ref!(app.editor).1.id(), target);
    assert!(!screen(&app).contains("Enter resolve"));
    assert!(app.editor.active_merge().is_none());
    keys(&mut app, "q:qa!<ret>").await?;
    assert!(app.close().await.is_empty());
    assert_eq!(
        checked_git(conflicted.path(), &["ls-files", "-u"])
            .lines()
            .count(),
        6
    );
    assert!(checked_git(clean.path(), &["ls-files", "-u"]).is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_confirmed_operation_finishes_on_application_exit() -> anyhow::Result<()> {
    let repo = repository(false);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    prepare_merge(&mut app).await?;
    keys(&mut app, "yq:qa!<ret>").await?;
    assert!(app.close().await.is_empty());
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
    assert_eq!(
        checked_git(repo.path(), &["ls-files", "-u"])
            .lines()
            .count(),
        6
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_focus_scan_uses_pathless_draft_worktree() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new().build()?;
    let session = enter(&mut app, repo.path()).await?;
    assert!(current_ref!(app.editor).1.path().is_none());
    assert_eq!(
        app.editor
            .git_context_path()
            .map(fs::canonicalize)
            .transpose()?,
        Some(fs::canonicalize(repo.path())?)
    );
    focus_gained(&mut app).await;
    assert!(app.editor.git_conflict_scan.is_running());
    finish_scan(&mut app).await?;
    assert!(app
        .editor
        .get_status()
        .is_some_and(|(text, _)| text.contains("2 conflicted files")));
    assert_eq!(app.editor.active_merge(), Some(session));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_switch_repository_ignores_old_focus_result() -> anyhow::Result<()> {
    let conflicted = repository(true);
    let clean = repository(false);
    let mut app = AppBuilder::new()
        .with_file(conflicted.path().join("file.txt"), None)
        .build()?;
    focus_gained(&mut app).await;
    assert!(app.editor.git_conflict_scan.is_running());
    let clean_doc = app.editor.open(
        &clean.path().join("file.txt"),
        helix_view::editor::Action::Replace,
    )?;
    finish_scan(&mut app).await?;
    assert_eq!(current_ref!(app.editor).1.id(), clean_doc);
    assert!(!app
        .editor
        .get_status()
        .is_some_and(|(text, _)| text.contains("conflicted files")));
    assert!(app.editor.active_merge().is_none());
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_merge_menu_requires_an_active_draft() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().build()?;
    let original = current_ref!(app.editor).1.text().to_string();
    keys(&mut app, "<space>vm").await?;
    assert!(app.editor.is_err());
    assert!(app.editor.get_status().unwrap().0.contains("conflict file"));
    // An unavailable merge menu must not consume the next normal-mode input.
    keys(&mut app, "iordinary text<esc>").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        format!("ordinary text{original}")
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_panel_refresh_notifies_without_opening_a_resolver() -> anyhow::Result<()> {
    let repo = repository(false);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    keys(&mut app, "<space>vg").await?;
    wait_screen(&mut app, "Git · Log").await?;
    assert!(!git(repo.path(), &["merge", "--no-edit", "topic"])
        .status
        .success());
    keys(&mut app, "r").await?;
    wait_screen(&mut app, "2 conflicted files").await?;
    assert!(!screen(&app).contains("Enter resolve"));
    assert!(app.editor.active_merge().is_none());
    keys(&mut app, "q:qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_changes_groups_unmerged_files_and_opens_the_resolver() -> anyhow::Result<()>
{
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    keys(&mut app, "<space>vcz").await?;
    wait_screen(&mut app, "Conflicts (2)").await?;
    wait_screen(&mut app, "Unmerged file").await?;
    assert!(app.editor.active_merge().is_none());
    keys(&mut app, "3<ret>").await?;
    wait_merge(&mut app).await?;
    assert_eq!(
        current_ref!(app.editor).1.merge().unwrap().data.file.path,
        Path::new("file.txt")
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}
