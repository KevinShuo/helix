use super::helpers::*;
use helix_term::application::Application;
use helix_vcs::git_tools::GitRepository;
use helix_view::{current_ref, input::parse_macro, merge::MergeLeave};
use std::{fs, path::Path, process::Command, time::Duration};

#[path = "git_conflicts_discovery.rs"]
mod discovery;

#[path = "git_conflicts_acceptance.rs"]
mod acceptance;

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args([
            "-c",
            "user.name=Merge Test",
            "-c",
            "user.email=merge@example.test",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.output().unwrap()
}

fn checked_git(root: &Path, args: &[&str]) -> String {
    let output = git(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn repository(merge_now: bool) -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    checked_git(root, &["init", "-b", "main"]);
    checked_git(root, &["config", "user.name", "Merge Test"]);
    checked_git(root, &["config", "user.email", "merge@example.test"]);
    checked_git(root, &["config", "commit.gpgsign", "false"]);
    checked_git(root, &["config", "core.autocrlf", "false"]);
    for (branch, text) in [
        ("base", "before\nbase\nafter\n"),
        ("topic", "before\nincoming\nafter\n"),
        ("main", "before\ncurrent\nafter\n"),
    ] {
        if branch == "topic" {
            checked_git(root, &["switch", "-c", "topic"]);
        }
        if branch == "main" {
            checked_git(root, &["switch", "main"]);
        }
        fs::write(root.join("file.txt"), text).unwrap();
        fs::write(root.join("second.txt"), text).unwrap();
        fs::write(
            root.join("clean.txt"),
            if branch == "topic" {
                "clean incoming\n"
            } else {
                "clean base\n"
            },
        )
        .unwrap();
        checked_git(root, &["add", "--", "file.txt", "second.txt", "clean.txt"]);
        checked_git(root, &["commit", "-m", branch]);
    }
    if merge_now {
        assert!(!git(root, &["merge", "--no-edit", "topic"]).status.success());
    }
    repo
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
    tokio::time::timeout(Duration::from_secs(8), run_event_loop_until_idle(app)).await?;
    if input.contains("<space>vma") || input.contains("<space>vmt") {
        for _ in 0..100 {
            if !app
                .editor
                .documents()
                .any(|doc| doc.merge().is_some_and(|state| state.busy))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
            app.editor.reset_idle_timer();
            tokio::time::timeout(Duration::from_secs(8), run_event_loop_until_idle(app)).await?;
        }
        assert!(
            !app.editor
                .documents()
                .any(|doc| doc.merge().is_some_and(|state| state.busy)),
            "Apply is still busy"
        );
    }
    Ok(())
}

#[allow(dead_code)] // Used by the workspace UI tests added next.
fn screen(app: &Application) -> String {
    let buffer = app.test_screen();
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|c| c.symbol.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

async fn enter(
    app: &mut Application,
    root: &Path,
) -> anyhow::Result<helix_view::merge::MergeSessionId> {
    let git = GitRepository::open(root.to_path_buf(), true).await?;
    let data = git.conflict_file(Path::new("file.txt")).await?;
    let candidate = git.merge_candidate(&data).await?;
    app.editor.enter_merge(data, candidate)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_native_draft_restores_views_and_resumes_edits() -> anyhow::Result<()> {
    let repo = repository(true);
    let folded_path = repo.path().join("fold.cpp");
    fs::write(&folded_path, "void f() {\n  hidden();\n}\nint tail;\n")?;
    let mut app = AppBuilder::new().with_file(&folded_path, None).build()?;
    app.test_resize(120, 30).await?;
    keys(&mut app, "za").await?;
    let fold_view = app.editor.tree.focus;
    let fold_doc = current_ref!(app.editor).1.id();
    assert_eq!(app.editor.documents[&fold_doc].folds(fold_view).len(), 1);
    app.editor.open(
        &repo.path().join("second.txt"),
        helix_view::editor::Action::VerticalSplit,
    )?;
    let views: Vec<_> = app
        .editor
        .tree
        .views()
        .map(|(v, _)| (v.id, v.doc))
        .collect();
    let original_focus = app.editor.tree.focus;
    let original_document = current_ref!(app.editor).1.id();
    let original_bytes = fs::read(repo.path().join("file.txt"))?;
    let session = enter(&mut app, repo.path()).await?;
    let draft_doc = current_ref!(app.editor).1.id();
    keys(&mut app, "iuser draft<esc>").await?;
    let draft = current_ref!(app.editor).1.text().to_string();
    app.editor.leave_merge(MergeLeave::KeepDraft)?;
    assert_eq!(app.editor.tree.focus, original_focus);
    assert_eq!(current_ref!(app.editor).1.id(), original_document);
    assert_eq!(
        app.editor
            .tree
            .views()
            .map(|(v, _)| (v.id, v.doc))
            .collect::<Vec<_>>(),
        views
    );
    assert_eq!(app.editor.documents[&fold_doc].folds(fold_view).len(), 1);
    app.editor.resume_merge(session)?;
    assert_eq!(current_ref!(app.editor).1.id(), draft_doc);
    assert_eq!(current_ref!(app.editor).1.text().to_string(), draft);
    assert_eq!(fs::read(repo.path().join("file.txt"))?, original_bytes);
    keys(&mut app, ":q<ret>").await?;
    assert!(app.editor.active_merge().is_none());
    assert!(app.editor.documents.contains_key(&draft_doc));
    app.editor.resume_merge(session)?;
    keys(&mut app, ":q!<ret>").await?;
    assert!(app.editor.active_merge().is_none());
    assert!(!app.editor.documents.contains_key(&draft_doc));
    assert_eq!(current_ref!(app.editor).1.id(), original_document);
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_native_save_cannot_bypass_apply() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    enter(&mut app, repo.path()).await?;
    keys(&mut app, "iunsaved<esc>").await?;
    let before = current_ref!(app.editor).1.text().to_string();
    let disk = fs::read(repo.path().join("file.txt"))?;
    keys(&mut app, ":w<ret>").await?;
    assert!(app.editor.is_err());
    assert_eq!(current_ref!(app.editor).1.text().to_string(), before);
    let export = repo.path().join("export.txt");
    let id = current_ref!(app.editor).1.id();
    assert!(app
        .editor
        .documents
        .get_mut(&id)
        .unwrap()
        .save(Some(export.clone()), true)
        .is_err());
    assert!(!export.exists());
    assert_eq!(fs::read(repo.path().join("file.txt"))?, disk);
    assert_eq!(
        GitRepository::open(repo.path().to_owned(), true)
            .await?
            .conflicts()
            .await?
            .files
            .len(),
        2
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_document_undo_and_freeze() -> anyhow::Result<()> {
    use helix_vcs::merge::ConflictChoice;
    use helix_view::current;
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    enter(&mut app, repo.path()).await?;
    let initial = current_ref!(app.editor).1.text().to_string();
    {
        let (view, doc) = current!(app.editor);
        assert!(!doc.is_modified());
        let tx =
            doc.merge()
                .unwrap()
                .model
                .choice(0, ConflictChoice::Incoming, doc.text().slice(..))?;
        assert!(doc.apply(&tx, view.id));
        doc.merge_mut().unwrap().model.mark_reviewed(Some(0));
        doc.append_changes_to_history(view);
        assert!(doc.merge().unwrap().model.ready_to_apply());
    }
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nincoming\nafter\n"
    );
    keys(&mut app, "u").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), initial);
    assert!(!current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    keys(&mut app, "U").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nincoming\nafter\n"
    );
    assert!(!current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    {
        let (view, doc) = current!(app.editor);
        doc.merge_mut().unwrap().busy = true;
        let tx =
            helix_core::Transaction::insert(doc.text(), doc.selection(view.id), "blocked".into());
        assert!(!doc.apply(&tx, view.id));
        assert!(!doc.undo(view));
        doc.merge_mut().unwrap().busy = false;
        assert!(doc.undo(view));
        assert_eq!(doc.text().to_string(), initial);
    }
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_manual_work_and_split_preserve_drafts() -> anyhow::Result<()> {
    let repo = repository(true);
    fs::write(repo.path().join("file.txt"), "external manual resolution")?;
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("second.txt"), None)
        .build()?;
    let original = current_ref!(app.editor).1.id();
    let session = enter(&mut app, repo.path()).await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "external manual resolution"
    );
    assert_eq!(
        current_ref!(app.editor).1.merge().unwrap().model.mode(),
        helix_vcs::merge::MergeMode::WholeFileReview
    );
    keys(&mut app, ":vs<ret>").await?;
    assert!(app.editor.active_merge().is_none());
    assert_eq!(app.editor.tree.views().count(), 2);
    assert!(app
        .editor
        .tree
        .views()
        .all(|(view, _)| view.doc == original));
    app.editor.resume_merge(session)?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "external manual resolution"
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_multiple_drafts_write_all_and_quit_guards() -> anyhow::Result<()> {
    let repo = repository(true);
    let ordinary = repo.path().join("ordinary.txt");
    fs::write(&ordinary, "ordinary\n")?;
    let anchor = repo.path().join("anchor.txt");
    fs::write(&anchor, "anchor\n")?;
    let mut app = AppBuilder::new().with_file(&anchor, None).build()?;
    app.editor
        .open(&ordinary, helix_view::editor::Action::VerticalSplit)?;
    keys(&mut app, "iedited <esc>").await?;
    let one = enter(&mut app, repo.path()).await?;
    keys(&mut app, "ifirst draft<esc>").await?;
    let one_doc = current_ref!(app.editor).1.id();
    let one_text = current_ref!(app.editor).1.text().to_string();
    let git = GitRepository::open(repo.path().to_owned(), true).await?;
    let data = git.conflict_file(Path::new("second.txt")).await?;
    let candidate = git.merge_candidate(&data).await?;
    let two = app.editor.enter_merge(data, candidate)?;
    assert_ne!(one, two);
    keys(&mut app, "isecond draft<esc>").await?;
    keys(&mut app, ":wa<ret>").await?;
    assert!(app.editor.is_err());
    app.editor.flush_writes().await?;
    assert!(fs::read_to_string(&ordinary)?.starts_with("edited ordinary"));
    assert!(git.conflicts().await?.files.len() == 2);
    keys(&mut app, ":reload-all<ret>").await?;
    assert!(app.editor.active_merge().is_none());
    keys(&mut app, ":qa<ret>").await?;
    assert!(app.editor.is_err());
    assert!(!app.editor.should_close());
    assert_eq!(app.editor.documents[&one_doc].text().to_string(), one_text);
    assert_eq!(enter(&mut app, repo.path()).await?, one);
    assert_eq!(current_ref!(app.editor).1.id(), one_doc);
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_closing_other_buffers_restores_saved_tree() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("second.txt"), None)
        .build()?;
    let ordinary = current_ref!(app.editor).1.id();
    let session = enter(&mut app, repo.path()).await?;
    // A close-all/close-others command may close a document from the saved Tree.
    assert!(app.editor.close_document(ordinary, true).is_ok());
    assert!(app.editor.active_merge().is_none());
    assert!(current_ref!(app.editor).1.merge().is_none());
    assert!(app
        .editor
        .tree
        .views()
        .all(|(v, _)| app.editor.documents.contains_key(&v.doc)));
    app.editor.resume_merge(session)?;
    app.editor.leave_merge(MergeLeave::KeepDraft)?;
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_busy_draft_buffer_close_reports_refusal() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("second.txt"), None)
        .build()?;
    let original = current_ref!(app.editor).1.id();
    let session = enter(&mut app, repo.path()).await?;
    let draft = current_ref!(app.editor).1.id();
    let text = current_ref!(app.editor).1.text().to_string();
    app.editor
        .documents
        .get_mut(&draft)
        .unwrap()
        .merge_mut()
        .unwrap()
        .busy = true;

    keys(&mut app, ":bc!<ret>").await?;

    assert_eq!(app.editor.active_merge(), Some(session));
    assert_eq!(current_ref!(app.editor).1.id(), draft);
    assert_eq!(app.editor.documents[&draft].text().to_string(), text);
    assert!(
        app.editor.is_err(),
        "buffer close must report the busy-draft refusal"
    );
    assert!(app.editor.get_status().unwrap().0.contains("being applied"));

    app.editor
        .documents
        .get_mut(&draft)
        .unwrap()
        .merge_mut()
        .unwrap()
        .busy = false;
    keys(&mut app, ":bc!<ret>").await?;
    assert!(app.editor.active_merge().is_none());
    assert!(!app.editor.documents.contains_key(&draft));
    assert_eq!(current_ref!(app.editor).1.id(), original);
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_manual_list_and_apply() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    assert!(screen(&app).contains("Current"));
    keys(&mut app, "<space>vma").await?;
    assert!(app.editor.is_err());
    keys(&mut app, "<space>vmr").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nincoming\nafter\n"
    );
    keys(&mut app, "<space>vma").await?;
    assert_eq!(
        checked_git(repo.path(), &["ls-files", "-u", "--", "file.txt"]),
        ""
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("file.txt"))?,
        "before\nincoming\nafter\n"
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

async fn source_mouse(
    app: &mut Application,
    column: u16,
    row: u16,
    scroll: bool,
) -> anyhow::Result<()> {
    #[cfg(windows)]
    use crossterm::event as terminal;
    #[cfg(not(windows))]
    use termina::event as terminal;
    let event = terminal::Event::Mouse(terminal::MouseEvent {
        kind: if scroll {
            terminal::MouseEventKind::ScrollDown
        } else {
            terminal::MouseEventKind::Down(terminal::MouseButton::Left)
        },
        column,
        row,
        modifiers: terminal::KeyModifiers::NONE,
    });
    app.handle_terminal_events(Ok(event)).await;
    keys(app, "").await
}

async fn wait_merge(app: &mut Application) -> anyhow::Result<()> {
    for _ in 0..100 {
        if app.editor.active_merge().is_some() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(app, "").await?;
    }
    anyhow::bail!("merge workspace did not open: {}", screen(app))
}
async fn open_ui(app: &mut Application) -> anyhow::Result<()> {
    keys(app, "<space>vr").await?;
    for _ in 0..100 {
        if screen(app).contains("Enter resolve") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(app, "").await?;
    }
    keys(app, "<ret>").await?;
    wait_merge(app).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_ui_preview_readonly_and_native_close() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(80, 24).await?;
    open_ui(&mut app).await?;
    let text = current_ref!(app.editor).1.text().to_string();
    let doc = current_ref!(app.editor).1.id();
    keys(&mut app, "<space>vmvijjddx1ialter3dd").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), text);
    assert_eq!(current_ref!(app.editor).1.id(), doc);
    keys(&mut app, "2iresult<esc>").await?;
    assert!(current_ref!(app.editor)
        .1
        .text()
        .to_string()
        .starts_with("result"));
    app.test_resize(160, 40).await?;
    let edited = current_ref!(app.editor).1.text().to_string();
    source_mouse(&mut app, 5, 4, false).await?;
    source_mouse(&mut app, 5, 4, true).await?;
    keys(&mut app, "iddx").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), edited);
    source_mouse(&mut app, 80, 1, false).await?;
    keys(&mut app, ":q<ret>").await?;
    assert!(app.editor.active_merge().is_none());
    assert!(!screen(&app).contains("Result [editable]"));
    assert!(!screen(&app).contains("Enter resolve"));
    open_ui(&mut app).await?;
    assert_eq!(current_ref!(app.editor).1.id(), doc);
    assert!(current_ref!(app.editor)
        .1
        .text()
        .to_string()
        .starts_with("result"));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_readonly_preview_footer_cannot_review_or_apply() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    let working = fs::read(repo.path().join("file.txt"))?;
    let unmerged = checked_git(repo.path(), &["ls-files", "-u"]);
    keys(&mut app, "<space>vmv").await?;
    assert!(screen(&app).contains("Read-only preview"));
    // These positions contain Review and Apply only while Result is active.
    source_mouse(&mut app, 54, 38, false).await?;
    assert!(!current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    source_mouse(&mut app, 64, 38, false).await?;
    assert_eq!(fs::read(repo.path().join("file.txt"))?, working);
    assert_eq!(checked_git(repo.path(), &["ls-files", "-u"]), unmerged);
    keys(&mut app, "<esc>:qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_narrow_confirmation_shows_the_discarded_side() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(40, 12).await?;
    open_ui(&mut app).await?;
    for (key, warning) in [
        ("L", "ALL Incoming"),
        ("R", "ALL Current"),
        ("B", "ALL draft edits"),
    ] {
        keys(&mut app, &format!("<space>vm{key}")).await?;
        assert!(
            screen(&app).contains(warning),
            "Hidden consequence: {}",
            screen(&app)
        );
        keys(&mut app, "n").await?;
    }
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_native_buffer_switch_restores_the_older_merge_workspace(
) -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    let first = current_ref!(app.editor).1.id();
    keys(&mut app, "<space>vmf").await?;
    discovery::wait_screen(&mut app, "Enter resolve").await?;
    keys(&mut app, "j<ret>").await?;
    wait_merge(&mut app).await?;
    assert_ne!(current_ref!(app.editor).1.id(), first);
    keys(&mut app, ":q<ret>").await?;
    // Native buffer picker/cycling and quit guards use this same editor path.
    app.editor
        .switch(first, helix_view::editor::Action::Replace);
    keys(&mut app, "<space>vml").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\ncurrent\nafter\n"
    );
    assert!(current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    assert!(screen(&app).contains("Result [editable]"));
    assert!(screen(&app).contains("Current [read-only]"));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_native_resume_refresh_preserves_other_worktree_drafts() -> anyhow::Result<()>
{
    let first_repo = repository(true);
    let other_repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(first_repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    keys(&mut app, "iFirst draft<esc>").await?;
    let first = current_ref!(app.editor).1.id();
    keys(&mut app, ":q<ret>").await?;
    app.editor.open(
        &other_repo.path().join("file.txt"),
        helix_view::editor::Action::Replace,
    )?;
    open_ui(&mut app).await?;
    keys(&mut app, "iOther draft<esc>").await?;
    let other = current_ref!(app.editor).1.id();
    let other_text = current_ref!(app.editor).1.text().to_string();
    keys(&mut app, ":q<ret>").await?;
    app.editor
        .switch(first, helix_view::editor::Action::Replace);
    keys(&mut app, "<space>vmFy").await?;
    for _ in 0..200 {
        if current_ref!(app.editor).1.id() != first {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        keys(&mut app, "").await?;
    }
    assert!(
        app.editor.documents.contains_key(&other),
        "Refresh of first worktree discarded another worktree's draft"
    );
    assert_eq!(app.editor.documents[&other].text().to_string(), other_text);
    assert_ne!(current_ref!(app.editor).1.id(), first);
    let root = current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .data
        .version
        .root();
    assert_eq!(
        fs::canonicalize(root)?,
        fs::canonicalize(first_repo.path())?
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_applied_result_is_readonly_including_history_and_choices(
) -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    keys(&mut app, "<space>vml<space>vma").await?;
    let text = current_ref!(app.editor).1.text().to_string();
    let version = current_ref!(app.editor).1.version();
    keys(&mut app, "iunsaved change<esc>u<U><space>vmr").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), text);
    assert_eq!(current_ref!(app.editor).1.version(), version);
    assert!(!current_ref!(app.editor).1.is_modified());
    assert!(screen(&app).contains("staged, read-only"));
    assert_eq!(fs::read_to_string(repo.path().join("file.txt"))?, text);
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_staged_draft_is_stale_when_the_path_conflicts_again() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    keys(&mut app, "<space>vml<space>vma<space>vmf").await?;
    let old_draft = app.editor.merge_sessions.iter().next().unwrap().1;
    checked_git(repo.path(), &["merge", "--abort"]);
    assert!(!git(repo.path(), &["merge", "--no-edit", "topic"])
        .status
        .success());
    keys(&mut app, "r").await?;
    discovery::wait_screen(&mut app, "Stale draft").await?;
    keys(&mut app, "<ret>").await?;
    wait_merge(&mut app).await?;
    assert_eq!(current_ref!(app.editor).1.id(), old_draft);
    assert!(matches!(
        current_ref!(app.editor).1.merge().unwrap().write_status,
        helix_view::merge::MergeWriteStatus::NeedsRefresh(_)
    ));
    keys(&mut app, "<space>vmFy").await?;
    for _ in 0..100 {
        if current_ref!(app.editor).1.id() != old_draft {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        keys(&mut app, "").await?;
    }
    assert_ne!(current_ref!(app.editor).1.id(), old_draft);
    keys(&mut app, "<space>vmr<space>vma").await?;
    assert_eq!(
        checked_git(repo.path(), &["show", ":file.txt"]),
        "before\nincoming\nafter"
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_ui_whole_file_rebuild_and_stale_refresh_confirmations() -> anyhow::Result<()>
{
    let repo = repository(true);
    fs::write(repo.path().join("file.txt"), "external manual")?;
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("second.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    keys(&mut app, "<space>vmLn").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "external manual"
    );
    keys(&mut app, "<space>vmLy").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\ncurrent\nafter\n"
    );
    assert!(!current_ref!(app.editor)
        .1
        .merge()
        .unwrap()
        .model
        .ready_to_apply());
    assert_eq!(
        fs::read_to_string(repo.path().join("file.txt"))?,
        "external manual"
    );
    keys(&mut app, "<space>vmBy").await?;
    for _ in 0..100 {
        if current_ref!(app.editor).1.merge().unwrap().model.mode()
            == helix_vcs::merge::MergeMode::Blockwise
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\nbase\nafter\n"
    );
    keys(&mut app, "<space>vml").await?;
    fs::write(repo.path().join("file.txt"), "new external work")?;
    keys(&mut app, "<space>vma").await?;
    assert!(matches!(
        current_ref!(app.editor).1.merge().unwrap().write_status,
        helix_view::merge::MergeWriteStatus::NeedsRefresh(_)
    ));
    keys(&mut app, ":q<ret>").await?;
    open_ui(&mut app).await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\ncurrent\nafter\n"
    );
    keys(&mut app, "<space>vmFn").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "before\ncurrent\nafter\n"
    );
    let old_id = current_ref!(app.editor).1.id();
    keys(&mut app, "<space>vmFy").await?;
    for _ in 0..100 {
        if current_ref!(app.editor).1.id() != old_id {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        "new external work"
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_ui_dirty_buffer_blocks_apply_and_retry_survives_reentry(
) -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    let ordinary = current_ref!(app.editor).1.id();
    keys(&mut app, "idirty<esc>").await?;
    open_ui(&mut app).await?;
    keys(&mut app, "<space>vml<space>vma").await?;
    assert!(app
        .editor
        .get_status()
        .unwrap()
        .0
        .contains("unsaved buffer"));
    assert!(!checked_git(repo.path(), &["ls-files", "-u", "--", "file.txt"]).is_empty());
    // Undo the ordinary edit through its real view, then return to the same draft.
    let session = app.editor.active_merge().unwrap();
    keys(&mut app, ":q<ret>u").await?;
    assert!(!app.editor.documents[&ordinary].is_modified());
    app.editor.resume_merge(session)?;
    fs::write(repo.path().join(".git/index.lock"), "")?;
    keys(&mut app, "<space>vma").await?;
    assert!(matches!(
        current_ref!(app.editor).1.merge().unwrap().write_status,
        helix_view::merge::MergeWriteStatus::Retry { .. }
    ));
    let draft = current_ref!(app.editor).1.id();
    assert_eq!(
        app.editor.documents[&ordinary].text().to_string(),
        "before\ncurrent\nafter\n"
    );
    keys(&mut app, ":q<ret>").await?;
    open_ui(&mut app).await?;
    assert_eq!(current_ref!(app.editor).1.id(), draft);
    assert!(matches!(
        current_ref!(app.editor).1.merge().unwrap().write_status,
        helix_view::merge::MergeWriteStatus::Retry { .. }
    ));
    fs::remove_file(repo.path().join(".git/index.lock"))?;
    keys(&mut app, "<space>vmt").await?;
    assert!(matches!(
        current_ref!(app.editor).1.merge().unwrap().write_status,
        helix_view::merge::MergeWriteStatus::Staged
    ));
    assert_eq!(
        checked_git(repo.path(), &["ls-files", "-u", "--", "file.txt"]),
        ""
    );
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_ui_finish_and_abort_are_separate_confirmations() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    open_ui(&mut app).await?;
    keys(&mut app, "<space>vml<space>vma<space>vmf").await?;
    for _ in 0..100 {
        if screen(&app).contains("Enter resolve") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    keys(&mut app, "j<ret>").await?;
    wait_merge(&mut app).await?;
    assert_eq!(
        current_ref!(app.editor).1.merge().unwrap().data.file.path,
        Path::new("second.txt")
    );
    keys(&mut app, "<space>vmr<space>vma<space>vmf").await?;
    for _ in 0..100 {
        if screen(&app).contains("Enter resolve") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
    keys(&mut app, "f").await?;
    for _ in 0..100 {
        if screen(&app).contains("Confirm Finish") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    assert!(screen(&app).contains("file.txt"));
    assert!(screen(&app).contains("second.txt"));
    keys(&mut app, "y").await?;
    for _ in 0..100 {
        if !repo.path().join(".git/MERGE_HEAD").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        keys(&mut app, "").await?;
    }
    assert!(!repo.path().join(".git/MERGE_HEAD").exists());
    assert_eq!(
        checked_git(repo.path(), &["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
    keys(&mut app, "q:qa!<ret>").await?;
    app.close().await;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_ui_abort_requires_confirmation() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    keys(&mut app, "<space>vr").await?;
    for _ in 0..100 {
        if screen(&app).contains("Enter resolve") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    keys(&mut app, "a").await?;
    for _ in 0..100 {
        if screen(&app).contains("Confirm Abort") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    keys(&mut app, "n").await?;
    assert!(repo.path().join(".git/MERGE_HEAD").exists());
    keys(&mut app, "a").await?;
    for _ in 0..100 {
        if screen(&app).contains("Confirm Abort") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        keys(&mut app, "").await?;
    }
    keys(&mut app, "y").await?;
    for _ in 0..100 {
        if !repo.path().join(".git/MERGE_HEAD").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        keys(&mut app, "").await?;
    }
    assert!(!repo.path().join(".git/MERGE_HEAD").exists());
    keys(&mut app, "q:qa!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_conflicts_ui_trust_and_apply_completion_after_close() -> anyhow::Result<()> {
    let repo = repository(true);
    let mut app = AppBuilder::new()
        .with_file(repo.path().join("file.txt"), None)
        .build()?;
    app.test_resize(160, 40).await?;
    let ordinary = current_ref!(app.editor).1.id();
    open_ui(&mut app).await?;
    keys(&mut app, "<space>vml").await?;
    let draft = current_ref!(app.editor).1.id();
    app.editor.workspace_trust.set_config(Default::default());
    let workspace = helix_loader::find_workspace_in(repo.path()).0;
    app.editor.workspace_trust.deny_once(&workspace);
    keys(&mut app, "<space>vma").await?;
    assert!(app.editor.get_status().unwrap().0.contains("trusted"));
    assert!(!checked_git(repo.path(), &["ls-files", "-u", "--", "file.txt"]).is_empty());
    app.editor.workspace_trust = helix_loader::workspace_trust::WorkspaceTrust::fully_trusted();
    // All input is delivered before driving Jobs: the result view is already
    // closed when the confirmed write completes.
    keys(&mut app, "<space>vma:q<ret>").await?;
    assert!(app.editor.active_merge().is_none());
    assert_eq!(current_ref!(app.editor).1.id(), ordinary);
    assert!(!app.editor.documents[&draft].merge().unwrap().busy);
    assert!(matches!(
        app.editor.documents[&draft].merge().unwrap().write_status,
        helix_view::merge::MergeWriteStatus::Staged
    ));
    assert_eq!(
        app.editor.documents[&ordinary].text().to_string(),
        "before\ncurrent\nafter\n"
    );
    assert!(!screen(&app).contains("Result [editable]"));
    assert!(!screen(&app).contains("Enter resolve"));
    keys(&mut app, ":qa!<ret>").await?;
    app.close().await;
    Ok(())
}
