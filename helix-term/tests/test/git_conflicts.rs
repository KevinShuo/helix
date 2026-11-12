use super::helpers::*;
use helix_term::application::Application;
use helix_vcs::git_tools::GitRepository;
use helix_view::{current_ref, input::parse_macro, merge::MergeLeave};
use std::{fs, path::Path, process::Command, time::Duration};

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
        checked_git(root, &["add", "--", "file.txt", "second.txt"]);
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
