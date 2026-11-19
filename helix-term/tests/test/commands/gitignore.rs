use super::*;
use helix_view::{doc, document::Mode};
use std::{
    fs,
    time::{Duration, Instant},
};

mod completion;
#[cfg(feature = "git")]
mod preview;

#[cfg(not(feature = "git"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_preview_reports_missing_git_support() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let file = root.path().join(".gitignore");
    fs::write(&file, "*.hxpreview\n")?;
    let mut app = AppBuilder::new().with_file(&file, None).build()?;
    keys(&mut app, ":gitignore-preview<ret>").await?;
    assert!(app.editor.is_err());
    assert!(app.editor.get_status().unwrap().0.contains("Git support"));
    assert!(!doc!(app.editor).is_modified());
    Ok(())
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

async fn keys(app: &mut Application, keys: &str) -> anyhow::Result<()> {
    for key in helix_view::input::parse_macro(keys)? {
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

async fn ready(app: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !screen(app).contains("Tab toggle") && Instant::now() < deadline {
        app.editor.reset_idle_timer();
        tokio::time::timeout(Duration::from_secs(5), run_event_loop_until_idle(app))
            .await
            .unwrap();
    }
    assert!(screen(app).contains("Tab toggle"), "{}", screen(app));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_shortcut_generates_editable_draft_and_saves_only_on_write() -> anyhow::Result<()>
{
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join(".git"))?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    let source = root.path().join("original.txt");
    fs::write(&source, "original\n")?;
    let target = root.path().join(".gitignore");
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    keys(&mut app, "iunsaved <esc><space>vi").await?;
    ready(&mut app).await;
    assert!(screen(&app).contains("[x] Rust"));
    keys(&mut app, "macos<tab><ret>").await?;
    assert_eq!(
        doc!(app.editor).path(),
        Some(helix_stdx::path::normalize(&target).as_path())
    );
    assert_eq!(app.editor.mode(), Mode::Normal);
    assert!(doc!(app.editor).is_modified());
    assert!(doc!(app.editor)
        .text()
        .to_string()
        .contains("templates: rust, macos"));
    assert!(!target.exists());
    assert!(app.editor.document_by_path(&source).unwrap().is_modified());
    keys(&mut app, "Go# checked<esc>:w<ret>").await?;
    let saved = fs::read_to_string(&target)?;
    assert!(saved.contains("# checked"));
    assert!(saved.contains("/target/"));
    assert_eq!(fs::read_to_string(&source)?, "original\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_existing_files_and_unsaved_buffers_are_reused() -> anyhow::Result<()> {
    for disk in [true, false] {
        let root = tempfile::tempdir()?;
        let target = root.path().join(".gitignore");
        let source = root.path().join("source.txt");
        if disk {
            fs::write(&target, "keep-this\n")?;
        }
        let mut app = AppBuilder::new().with_file(&source, None).build()?;
        if !disk {
            keys(&mut app, ":new-file .gitignore<ret>keep-this<esc>").await?;
        }
        keys(&mut app, ":gitignore<ret>").await?;
        assert_eq!(
            doc!(app.editor).path(),
            Some(helix_stdx::path::normalize(&target).as_path())
        );
        assert_eq!(doc!(app.editor).text().to_string().trim_end(), "keep-this");
        assert_eq!(target.exists(), disk);
        assert!(!screen(&app).contains("Gitignore templates"));
        let count = app.editor.documents().count();
        keys(&mut app, ":gitignore<ret>").await?;
        assert_eq!(app.editor.documents().count(), count);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_cancel_discards_late_detection_and_preserves_document() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.txt");
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    keys(&mut app, "<space>vi<esc>").await?;
    app.editor.reset_idle_timer();
    run_event_loop_until_idle(&mut app).await;
    assert_eq!(app.editor.documents().count(), 1);
    assert_eq!(
        doc!(app.editor).path(),
        Some(helix_stdx::path::normalize(&source).as_path())
    );
    assert!(!root.path().join(".gitignore").exists());
    assert!(!screen(&app).contains("Gitignore templates"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_no_detection_and_hidden_selections() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    keys(&mut app, ":gitignore<ret>").await?;
    ready(&mut app).await;
    assert!(screen(&app).contains("No Git repository detected"));
    keys(&mut app, "<ret>").await?;
    assert!(screen(&app).contains("Select at least one template"));
    keys(&mut app, "rust<tab><C-u>no-such-template<ret>").await?;
    assert!(doc!(app.editor)
        .text()
        .to_string()
        .contains("templates: rust\n"));
    assert!(!root.path().join(".gitignore").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_target_appearing_during_selection_keeps_external_content() -> anyhow::Result<()>
{
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    keys(&mut app, ":gitignore<ret>").await?;
    ready(&mut app).await;
    fs::write(root.path().join(".gitignore"), "external-content\n")?;
    keys(&mut app, "<ret>").await?;
    assert_eq!(doc!(app.editor).text().to_string(), "external-content\n");
    assert!(!doc!(app.editor).is_modified());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_rejects_directory_target_and_deleted_root() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join(".gitignore"))?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    keys(&mut app, ":gitignore<ret>").await?;
    assert!(app.editor.is_err());
    assert_eq!(app.editor.documents().count(), 1);
    fs::remove_dir(root.path().join(".gitignore"))?;
    keys(&mut app, ":gitignore<ret>").await?;
    ready(&mut app).await;
    keys(&mut app, "rust<tab>").await?;
    fs::remove_dir(root.path())?;
    keys(&mut app, "<ret>").await?;
    assert_eq!(app.editor.documents().count(), 1);
    assert!(app.editor.is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_empty_files_and_buffers_are_not_regenerated() -> anyhow::Result<()> {
    for disk in [true, false] {
        let root = tempfile::tempdir()?;
        let target = root.path().join(".gitignore");
        if disk {
            fs::write(&target, "")?;
        }
        let mut app = AppBuilder::new()
            .with_file(root.path().join("source.txt"), None)
            .build()?;
        if !disk {
            keys(&mut app, ":new-file .gitignore<ret><esc>").await?;
        }
        keys(&mut app, ":gitignore<ret>").await?;
        assert_eq!(
            doc!(app.editor).path(),
            Some(helix_stdx::path::normalize(&target).as_path())
        );
        assert!(!doc!(app.editor).is_modified());
        assert!(!screen(&app).contains("Gitignore templates"));
        assert_eq!(target.exists(), disk);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_small_window_keeps_selection_and_allows_cancel() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    keys(&mut app, ":gitignore<ret>").await?;
    ready(&mut app).await;
    assert!(screen(&app).contains("Cargo.toml"));
    app.test_resize(24, 6).await?;
    app.test_resize(100, 25).await?;
    assert!(screen(&app).contains("[x] Rust"));
    app.test_resize(8, 3).await?;
    keys(&mut app, "<esc>").await?;
    assert_eq!(app.editor.documents().count(), 1);
    assert!(!root.path().join(".gitignore").exists());
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_reuses_unsaved_windows_case_variants() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    keys(&mut app, ":new-file .GITIGNORE<ret>original draft<esc>").await?;
    let id = doc!(app.editor).id();
    let count = app.editor.documents().count();
    keys(&mut app, ":gitignore<ret>").await?;
    assert!(
        !screen(&app).contains("Gitignore templates"),
        "case variant must reuse the original buffer"
    );
    assert_eq!(doc!(app.editor).id(), id);
    assert_eq!(app.editor.documents().count(), count);
    assert_eq!(
        doc!(app.editor).text().to_string().trim_end(),
        "original draft"
    );
    assert!(!root.path().join(".gitignore").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_file_appearing_inside_document_open_is_loaded_before_save() -> anyhow::Result<()>
{
    use helix_view::events::DocumentDidOpen;
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    let target = helix_stdx::path::normalize(root.path().join(".gitignore"));
    let mut app = AppBuilder::new()
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    let watched = target.clone();
    helix_event::register_hook!(move |event: &mut DocumentDidOpen<'_>| {
        if event.editor.document(event.doc).and_then(|doc| doc.path()) == Some(watched.as_path()) {
            fs::write(&watched, "created-during-open\n")?;
        }
        Ok(())
    });
    keys(&mut app, ":gitignore<ret>").await?;
    ready(&mut app).await;
    keys(&mut app, "<ret>").await?;
    assert_eq!(doc!(app.editor).text().to_string(), "created-during-open\n");
    assert!(!doc!(app.editor).is_modified());
    keys(&mut app, ":w<ret>").await?;
    assert_eq!(fs::read_to_string(target)?, "created-during-open\n");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_replaces_initial_newline_with_exact_lf_draft() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.toml"), "")?;
    let mut config = helix_term::config::Config::default();
    config.editor.default_line_ending = helix_view::editor::LineEndingConfig::Crlf;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    keys(&mut app, ":gitignore<ret>").await?;
    ready(&mut app).await;
    keys(&mut app, "<ret>").await?;
    let text = doc!(app.editor).text().to_string();
    assert!(
        !text.contains('\r'),
        "generated document must contain only LF"
    );
    assert!(
        text.ends_with('\n') && !text.ends_with("\n\n"),
        "must have one final newline"
    );
    keys(&mut app, "Go# edit<esc>:w<ret>").await?;
    let saved = fs::read_to_string(root.path().join(".gitignore"))?;
    assert!(
        !saved.contains('\r'),
        "further edits must use the draft's LF policy"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_refuses_symlink_target() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let external = root.path().join("other");
    fs::write(&external, "keep")?;
    std::os::unix::fs::symlink(&external, root.path().join(".gitignore"))?;
    let mut app = AppBuilder::new()
        .with_file(root.path().join("source.txt"), None)
        .build()?;
    keys(&mut app, ":gitignore<ret>").await?;
    assert!(app.editor.is_err());
    assert_eq!(app.editor.documents().count(), 1);
    assert_eq!(fs::read_to_string(external)?, "keep");
    Ok(())
}
