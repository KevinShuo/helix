use super::helpers::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_tools_preserve_function_folding_through_dock_resize_and_fullscreen(
) -> anyhow::Result<()> {
    let repo = repository();
    let path = repo.path().join("fold.cpp");
    fs::write(&path, "void f() {\n  hidden();\n}\nint tail;\n")?;
    git(repo.path(), &["add", "fold.cpp"]);
    git(repo.path(), &["commit", "-m", "Add folding fixture"]);
    let mut app = AppBuilder::new().with_file(&path, None).build()?;
    app.test_resize(100, 30).await?;
    keys(&mut app, "za<space>vg").await?;
    wait_for(&mut app, "Git · Log").await;
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(doc.folds(view.id).len(), 1);
    app.test_resize(70, 18).await?;
    keys(&mut app, "z").await?;
    keys(&mut app, "zq").await?;
    app.test_resize(100, 30).await?;
    assert!(screen(&app).contains("void f() {…}"));
    keys(&mut app, "j").await?;
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(
        doc.text().char_to_line(
            doc.selection(view.id)
                .primary()
                .cursor(doc.text().slice(..))
        ),
        3
    );
    assert_eq!(doc.folds(view.id).len(), 1);
    keys(&mut app, ":q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[path = "git_tools_review.rs"]
mod review;
use helix_term::application::Application;
use helix_view::{current_ref, input::parse_macro};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args([
            "-c",
            "user.name=Git Tool Test",
            "-c",
            "user.email=git-tools@example.test",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn screen(app: &Application) -> String {
    let buffer = app.test_screen();
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|c| c.symbol.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

async fn keys(app: &mut Application, input: &str) -> anyhow::Result<()> {
    for key in parse_macro(input)? {
        #[cfg(windows)]
        let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::from(key));
        #[cfg(not(windows))]
        let event = termina::event::Event::Key(termina::event::KeyEvent::from(key));
        app.handle_terminal_events(Ok(event)).await;
    }
    app.editor.reset_idle_timer();
    tokio::time::timeout(Duration::from_secs(5), run_event_loop_until_idle(app)).await?;
    Ok(())
}

async fn wait_for(app: &mut Application, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !screen(app).contains(text) && Instant::now() < deadline {
        app.editor.reset_idle_timer();
        tokio::time::timeout(Duration::from_secs(5), run_event_loop_until_idle(app))
            .await
            .expect("editor did not become idle");
    }
    assert!(
        screen(app).contains(text),
        "Missing {text:?}:\n{}",
        screen(app)
    );
}

fn repository() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "core.autocrlf", "false"]);
    fs::write(repo.path().join("file.txt"), "base content\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "Initial Git tools fixture"]);
    git(repo.path(), &["checkout", "-b", "topic"]);
    fs::write(repo.path().join("file.txt"), "topic content\n").unwrap();
    git(
        repo.path(),
        &[
            "commit",
            "-am",
            "Topic changes\n\nDetailed commit explanation.\nFinal message line.",
        ],
    );
    git(repo.path(), &["checkout", "main"]);
    repo
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_tools_native_dock_fullscreen_and_diff_flow() -> anyhow::Result<()> {
    let repo = repository();
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<esc>").await?;
    let original = app.editor.tree.area();
    keys(&mut app, "<space>vg").await?;
    wait_for(&mut app, "Topic changes").await;
    wait_for(&mut app, "+topic content").await;
    keys(&mut app, "d").await?;
    assert!(screen(&app).contains("Final message line."));
    keys(&mut app, "<esc>").await?;
    assert!(app.editor.tree.area().height < original.height);
    assert!(screen(&app).contains("Branches [1]"));
    let dock = app.editor.tree.area();
    fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/git-tools-dock-screen.txt"),
        screen(&app),
    )?;
    app.test_resize(70, 18).await?;
    assert!(screen(&app).contains("Commits [2]"));
    assert!(!screen(&app).contains("Branches [1]"));
    app.test_resize(120, 40).await?;
    assert!(screen(&app).contains("+topic content"));
    keys(&mut app, "z").await?;
    assert_eq!(app.editor.tree.area(), original);
    assert!(screen(&app).contains("+topic content"));
    keys(&mut app, "z").await?;
    assert_eq!(app.editor.tree.area(), dock);
    keys(&mut app, "/Initial<ret>").await?;
    wait_for(&mut app, "+base content").await;
    keys(&mut app, "q").await?;
    assert_eq!(app.editor.tree.area(), original);
    assert!(!screen(&app).contains("Git · Log"));
    // The original two-key binding still opens the original changed-file picker.
    // This picker uses the process CWD, not the opened fixture repository.
    // Its identity must remain testable even when the checkout is clean.
    keys(&mut app, "<space>g").await?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let rendered = screen(&app);
        if rendered.lines().any(|line| {
            line.split_whitespace()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|columns| columns == ["change", "path"])
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Changed-file picker missing:\n{rendered}"
        );
        keys(&mut app, "").await?;
    }
    assert_eq!(app.editor.tree.area(), original);
    // Picker previews may contain source code mentioning the Git panel title.
    assert!(!screen(&app)
        .lines()
        .any(|line| line.trim_start().starts_with("Git · Log")));
    keys(&mut app, "<esc>:q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_tools_checkout_is_explicit_and_reloads_clean_buffer() -> anyhow::Result<()> {
    let repo = repository();
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<space>vb").await?;
    wait_for(&mut app, "Topic changes").await;
    keys(&mut app, "jjjj<ret>").await?;
    assert_eq!(git(repo.path(), &["branch", "--show-current"]), "main");
    keys(&mut app, "ac").await?;
    assert!(screen(&app).contains("Switch to: topic"));
    assert_eq!(git(repo.path(), &["branch", "--show-current"]), "main");
    keys(&mut app, "<C-y>").await?;
    assert_eq!(
        git(repo.path(), &["branch", "--show-current"]),
        "main",
        "Ctrl-y must not confirm checkout"
    );
    keys(&mut app, "y").await?;
    wait_for(&mut app, "HEAD: topic").await;
    assert_eq!(git(repo.path(), &["branch", "--show-current"]), "topic");
    let (_, doc) = current_ref!(app.editor);
    assert_eq!(doc.text().to_string(), "topic content\n");
    keys(&mut app, "q:q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_tools_unsaved_buffers_and_reopen_are_safe() -> anyhow::Result<()> {
    let repo = repository();
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "iunsaved <esc><space>vgq<space>vb").await?;
    wait_for(&mut app, "Topic changes").await;
    keys(&mut app, "jjjj<ret>ac").await?;
    assert!(screen(&app).contains("Save modified buffers"));
    assert_eq!(git(repo.path(), &["branch", "--show-current"]), "main");
    let (_, doc) = current_ref!(app.editor);
    assert!(doc.text().to_string().starts_with("unsaved "));
    keys(&mut app, "q:q!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_tools_pointer_movement_keeps_editor_focus() -> anyhow::Result<()> {
    let repo = repository();
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(120, 40).await?;
    keys(&mut app, "<space>vg").await?;
    wait_for(&mut app, "Topic changes").await;
    keys(&mut app, "<C-w>w").await?;
    #[cfg(windows)]
    let event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Moved,
        column: 3,
        row: 35,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    #[cfg(not(windows))]
    let event = termina::event::Event::Mouse(termina::event::MouseEvent {
        kind: termina::event::MouseEventKind::Moved,
        column: 3,
        row: 35,
        modifiers: termina::event::KeyModifiers::NONE,
    });
    app.handle_terminal_events(Ok(event)).await;
    keys(&mut app, "iXYZ<esc>").await?;
    assert!(current_ref!(app.editor)
        .1
        .text()
        .to_string()
        .starts_with("XYZ"));
    keys(&mut app, "<space>vgq:q!<ret>").await?;
    app.close().await;
    Ok(())
}
