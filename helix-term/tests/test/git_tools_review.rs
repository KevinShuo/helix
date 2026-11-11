use super::*;
use helix_view::editor::Action;

fn alias_directory(target: &Path) -> (tempfile::TempDir, std::path::PathBuf) {
    let holder = tempfile::tempdir().unwrap();
    let alias = holder.path().join("alias");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(target)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &alias).unwrap();
    (holder, alias)
}

async fn focus_panel(app: &mut Application) {
    #[cfg(windows)]
    let event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 3,
        row: 35,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    #[cfg(not(windows))]
    let event = termina::event::Event::Mouse(termina::event::MouseEvent {
        kind: termina::event::MouseEventKind::Down(termina::event::MouseButton::Left),
        column: 3,
        row: 35,
        modifiers: termina::event::KeyModifiers::NONE,
    });
    app.handle_terminal_events(Ok(event)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_review_parent_nested_parent_repository_navigation() -> anyhow::Result<()> {
    let repo = repository();
    let nested = repo.path().join("nested");
    fs::create_dir(&nested)?;
    git(&nested, &["init", "-b", "nested-main"]);
    git(&nested, &["config", "core.autocrlf", "false"]);
    fs::write(nested.join("child.txt"), "child\n")?;
    git(&nested, &["add", "."]);
    git(&nested, &["commit", "-m", "Nested repository fixture"]);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<space>vg").await?;
    wait_for(&mut app, "Topic changes").await;
    keys(&mut app, "<C-w>w").await?;
    app.editor
        .open(&nested.join("child.txt"), Action::Replace)?;
    keys(&mut app, "<space>vg").await?;
    wait_for(&mut app, "Nested repository fixture").await;
    assert!(screen(&app).contains("HEAD: nested-main"));
    keys(&mut app, "<C-w>w").await?;
    app.editor
        .open(&repo.path().join("file.txt"), Action::Replace)?;
    keys(&mut app, "<space>vg").await?;
    wait_for(&mut app, "Topic changes").await;
    assert!(screen(&app).contains("HEAD: main"));
    keys(&mut app, "q:q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_review_aliased_unsaved_buffer_blocks_checkout() -> anyhow::Result<()> {
    let repo = repository();
    let (_holder, alias) = alias_directory(repo.path());
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<space>vg").await?;
    wait_for(&mut app, "Topic changes").await;
    keys(&mut app, "<C-w>w").await?;
    app.editor.open(&alias.join("file.txt"), Action::Replace)?;
    keys(&mut app, "iunsaved<esc>").await?;
    focus_panel(&mut app).await;
    keys(&mut app, "1<home>jjjj<ret>ac").await?;
    assert!(
        screen(&app).contains("Save modified buffers"),
        "{}",
        screen(&app)
    );
    assert_eq!(git(repo.path(), &["branch", "--show-current"]), "main");
    keys(&mut app, "<esc>:q!<ret>").await?;
    app.close().await;
    Ok(())
}

async fn fullscreen_handoff(key: &str) -> anyhow::Result<()> {
    let repo = repository();
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    let height = app.editor.tree.area().height;
    keys(&mut app, "<space>vg").await?;
    wait_for(&mut app, "+topic content").await;
    keys(&mut app, "z3").await?;
    keys(&mut app, key).await?;
    assert!(
        app.editor.tree.area().height < height,
        "The editor is still hidden behind fullscreen Git"
    );
    assert!(screen(&app).contains("z/F11: fullscreen"));
    if key == "O" {
        assert_eq!(
            current_ref!(app.editor).1.text().to_string(),
            "topic content\n"
        );
    }
    keys(&mut app, "iXYZ<esc>").await?;
    assert!(current_ref!(app.editor)
        .1
        .text()
        .to_string()
        .starts_with("XYZ"));
    keys(&mut app, "<space>vgq:qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_review_working_file_handoff_leaves_fullscreen() -> anyhow::Result<()> {
    fullscreen_handoff("o").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_review_historical_file_handoff_leaves_fullscreen() -> anyhow::Result<()> {
    fullscreen_handoff("O").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_review_window_focus_handoff_leaves_fullscreen() -> anyhow::Result<()> {
    fullscreen_handoff("<C-w>w").await
}

fn remote(repo: &Path) -> tempfile::TempDir {
    let bare = tempfile::tempdir().unwrap();
    git(repo, &["init", "--bare", bare.path().to_str().unwrap()]);
    git(
        repo,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    git(repo, &["push", "-u", "origin", "main"]);
    bare
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_review_push_confirms_and_freezes_actual_destination() -> anyhow::Result<()> {
    let repo = repository();
    let _remote = remote(repo.path());
    git(repo.path(), &["checkout", "-b", "feature"]);
    fs::write(repo.path().join("feature.txt"), "feature\n")?;
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "Feature push fixture"]);
    let id = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["config", "branch.feature.remote", "origin"]);
    git(
        repo.path(),
        &["config", "branch.feature.merge", "refs/heads/main"],
    );
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<space>vP").await?;
    wait_for(&mut app, "Confirm Git operation").await;
    assert!(screen(&app).contains("Remote: origin"), "{}", screen(&app));
    assert!(screen(&app).contains("Destination: refs/heads/main"));
    git(
        repo.path(),
        &["config", "branch.feature.merge", "refs/heads/other"],
    );
    keys(&mut app, "y").await?;
    let deadline = Instant::now() + Duration::from_secs(12);
    while !git(repo.path(), &["ls-remote", "origin", "refs/heads/main"]).starts_with(&id)
        && Instant::now() < deadline
    {
        app.editor.reset_idle_timer();
        tokio::time::timeout(Duration::from_secs(5), run_event_loop_until_idle(&mut app)).await?;
    }
    assert!(git(repo.path(), &["ls-remote", "origin", "refs/heads/main"]).starts_with(&id));
    assert!(git(repo.path(), &["ls-remote", "origin", "refs/heads/other"]).is_empty());
    keys(&mut app, "q:q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_review_operation_survives_reopen_and_refreshes_new_panel() -> anyhow::Result<()> {
    let repo = repository();
    let _remote = remote(repo.path());
    let hook = repo.path().join(".git/hooks/pre-push");
    fs::write(
        &hook,
        "#!/bin/sh\ntouch .git/git-tools-ready\nsleep 5\nprintf result > hook-result.txt\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
    }
    git(repo.path(), &["config", "core.hooksPath", ".git/hooks"]);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<space>vP").await?;
    wait_for(&mut app, "Confirm Git operation").await;
    keys(&mut app, "y").await?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !repo.path().join(".git/git-tools-ready").exists() && Instant::now() < deadline {
        app.editor.reset_idle_timer();
        tokio::time::timeout(Duration::from_secs(5), run_event_loop_until_idle(&mut app)).await?;
    }
    assert!(repo.path().join(".git/git-tools-ready").exists());
    keys(&mut app, "q<space>vc").await?;
    wait_for(&mut app, "Working tree vs HEAD").await;
    keys(&mut app, "af").await?;
    assert!(screen(&app).contains("already running"), "{}", screen(&app));
    wait_for(&mut app, "hook-result.txt").await;
    keys(&mut app, "q:q!<ret>").await?;
    app.close().await;
    Ok(())
}
