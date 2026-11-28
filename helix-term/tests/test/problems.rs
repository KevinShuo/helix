use super::helpers::*;
use helix_core::diagnostic::{Diagnostic, DiagnosticProvider, Severity};
use helix_term::application::Application;
use helix_view::{current_ref, editor::Action};
use std::path::Path;

async fn keys(app: &mut Application, input: &str) -> anyhow::Result<()> {
    for key in helix_view::input::parse_macro(input)? {
        #[cfg(windows)]
        let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::from(key));
        #[cfg(not(windows))]
        let event = termina::event::Event::Key(termina::event::KeyEvent::from(key));
        app.handle_terminal_events(Ok(event)).await;
    }
    app.editor.reset_idle_timer();
    // A running compiler can keep producing output forever; pump events without
    // requiring a globally idle application before the next user command.
    if app
        .editor
        .problems
        .compilation
        .current
        .as_ref()
        .is_some_and(|r| r.running)
    {
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            run_event_loop_until_idle(app),
        )
        .await;
    } else {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_event_loop_until_idle(app),
        )
        .await?;
    }
    Ok(())
}

fn diagnostic(start: usize, message: &str, severity: Severity) -> Diagnostic {
    Diagnostic {
        range: helix_core::diagnostic::Range {
            start,
            end: start + 1,
        },
        ends_at_word: false,
        starts_at_word: false,
        zero_width: false,
        line: 0,
        message: message.into(),
        severity: Some(severity),
        code: None,
        provider: DiagnosticProvider::Lsp {
            server_id: Default::default(),
            identifier: None,
        },
        tags: vec![],
        source: Some("fixture".into()),
        data: None,
    }
}

fn at(app: &Application, path: &Path, offset: usize) {
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(doc.path().unwrap(), &helix_stdx::path::canonicalize(path));
    assert_eq!(
        doc.selection(view.id)
            .primary()
            .cursor(doc.text().slice(..)),
        offset
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_file_navigation_filters_and_equal_locations() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    std::fs::write(&a, "abc\ndef\n")?;
    std::fs::write(&b, "abc\ndef\n")?;
    let mut app = AppBuilder::new().with_file(&a, None).build()?;
    let a_id = app.editor.open(&a, Action::Replace)?;
    app.editor
        .documents
        .get_mut(&a_id)
        .unwrap()
        .replace_diagnostics(
            [
                diagnostic(1, "a error", Severity::Error),
                diagnostic(1, "b warning", Severity::Warning),
                diagnostic(5, "hint", Severity::Hint),
            ],
            &[],
            None,
        );
    let b_id = app.editor.open(&b, Action::Replace)?;
    app.editor
        .documents
        .get_mut(&b_id)
        .unwrap()
        .replace_diagnostics([diagnostic(4, "other file", Severity::Error)], &[], None);
    app.editor.switch(a_id, Action::Replace);
    keys(&mut app, ":next-problem<ret>").await?;
    at(&app, &a, 1);
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("a error"));
    keys(&mut app, "]q").await?;
    at(&app, &a, 1);
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("b warning"));
    keys(&mut app, "]q").await?;
    at(&app, &b, 4);
    keys(&mut app, "[q").await?;
    at(&app, &a, 1);
    keys(&mut app, ":next-problem --severity error<ret>").await?;
    at(&app, &b, 4);
    keys(&mut app, ":previous-problem --scope document<ret>").await?;
    at(&app, &b, 4);
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("No previous problem"));
    keys(&mut app, ":previous-problem --scope workspace<ret>").await?;
    at(&app, &a, 1);
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn problem_list_filters_are_atomic_and_navigation_refreshes() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "abcdef\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    let id = current_ref!(app.editor).1.id();
    app.editor
        .documents
        .get_mut(&id)
        .unwrap()
        .replace_diagnostics(
            [
                diagnostic(1, "first", Severity::Error),
                diagnostic(3, "removed", Severity::Error),
                diagnostic(5, "last", Severity::Warning),
            ],
            &[],
            None,
        );
    keys(&mut app, "]q]q").await?;
    at(&app, file.path(), 3);
    app.editor
        .documents
        .get_mut(&id)
        .unwrap()
        .replace_diagnostics(
            [
                diagnostic(1, "first", Severity::Error),
                diagnostic(5, "last", Severity::Warning),
            ],
            &[],
            None,
        );
    keys(&mut app, "]q").await?;
    at(&app, file.path(), 5);
    keys(
        &mut app,
        ":previous-problem --source build --scope invalid<ret>",
    )
    .await?;
    keys(&mut app, "[q").await?;
    at(&app, file.path(), 1);
    keys(&mut app, " E").await?;
    assert!(!app.test_screen().content.is_empty());
    keys(&mut app, "<esc>:quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn navigation_reveals_folds_and_handles_unicode_and_picker() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".py").tempfile()?;
    let source = "def f():\n    𐐀needle()\n    return 1\n\nf()\n";
    std::fs::write(file.path(), source)?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    let id = current_ref!(app.editor).1.id();
    let start = source[..source.find("needle").unwrap()].chars().count();
    app.editor
        .documents
        .get_mut(&id)
        .unwrap()
        .replace_diagnostics(
            [diagnostic(start, "Unicode in fold", Severity::Error)],
            &[],
            None,
        );
    keys(&mut app, "zM").await?;
    assert!(!current_ref!(app.editor)
        .1
        .folds(current_ref!(app.editor).0.id)
        .is_empty());
    keys(&mut app, "]q").await?;
    at(&app, file.path(), start);
    assert!(current_ref!(app.editor)
        .1
        .folds(current_ref!(app.editor).0.id)
        .is_empty());
    keys(&mut app, "<C-o>").await?;
    at(&app, file.path(), 0);
    keys(&mut app, " E").await?;
    keys(&mut app, "<ret>").await?;
    at(&app, file.path(), start);
    assert!(!current_ref!(app.editor).1.is_modified());
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

fn fixture(root: &Path) -> anyhow::Result<std::path::PathBuf> {
    let executable = root.join(if cfg!(windows) {
        "compiler fixture.exe"
    } else {
        "compiler fixture"
    });
    let mut rustc =
        std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc
        .args(["--edition=2021"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compilation.rs"))
        .arg("-o")
        .arg(&executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        rustc.creation_flags(0x08000000);
    }
    let output = rustc.output()?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(executable)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_position_diagnostics_inside_a_grapheme_make_progress() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "e\u{301}abc\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    let id = current_ref!(app.editor).1.id();
    app.editor
        .documents
        .get_mut(&id)
        .unwrap()
        .replace_diagnostics(
            [
                diagnostic(1, "first message", Severity::Error),
                diagnostic(1, "second message", Severity::Warning),
            ],
            &[],
            None,
        );
    keys(&mut app, "]q").await?;
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("first message"));
    keys(&mut app, "]q").await?;
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("second message"));
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

fn shell_command(executable: &Path, mode: &str, path: &Path) -> String {
    if cfg!(windows) {
        format!("\"{}\" {mode} \"{}\"", executable.display(), path.display())
    } else {
        format!(
            "'{}' {mode} '{}'",
            executable.display().to_string().replace('\'', "'\\''"),
            path.display().to_string().replace('\'', "'\\''")
        )
    }
}

async fn wait_build(
    app: &mut Application,
    predicate: impl Fn(&Application) -> bool,
) -> anyhow::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while !predicate(app) {
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                run_event_loop_until_idle(app),
            )
            .await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|err| {
        anyhow::anyhow!(
            "{err}; editor={:?}; build={:?}",
            app.editor.status_msg,
            app.editor.problems.compilation.current.as_ref().map(|r| (
                &r.command,
                &r.status,
                &r.log,
                r.problems.len()
            ))
        )
    })?;
    Ok(())
}

async fn assert_prompt(app: &mut Application, expected: &str) -> anyhow::Result<()> {
    app.test_resize(80, 24).await?;
    let screen = app.test_screen();
    let line = screen
        .content
        .chunks_exact(screen.area.width as usize)
        .next_back()
        .unwrap()
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert_eq!(line.trim_end(), expected);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compile_history_arrows_recall_builds_and_restore_the_draft() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().build()?;
    for command in ["echo first", "echo second"] {
        keys(&mut app, &format!(":compile {command}<ret>")).await?;
        wait_build(&mut app, |app| {
            app.editor
                .problems
                .compilation
                .current
                .as_ref()
                .is_some_and(|run| !run.running)
        })
        .await?;
    }
    keys(&mut app, ":compile-log<ret>").await?;
    keys(&mut app, ":echo unrelated<ret>").await?;
    keys(&mut app, ":compile echo café").await?;
    assert_prompt(&mut app, ":compile echo café").await?;

    for (key, expected) in [
        ("<up>", ":compile echo second"),
        ("<up>", ":compile echo first"),
        ("<up>", ":compile echo first"),
        ("<down>", ":compile echo second"),
        ("<down>", ":compile echo café"),
        ("<down>", ":compile echo café"),
    ] {
        keys(&mut app, key).await?;
        assert_prompt(&mut app, expected).await?;
    }

    keys(&mut app, "<esc>:compile<up> edited<ret>").await?;
    let run = app.editor.problems.compilation.current.as_ref().unwrap();
    assert_eq!(run.command, "echo second edited");
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compile_history_without_builds_preserves_input() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().build()?;
    keys(
        &mut app,
        ":echo unrelated<ret>:compile <down><up>echo draft",
    )
    .await?;
    assert_prompt(&mut app, ":compile echo draft").await?;
    keys(&mut app, "<esc>:<up>").await?;
    assert_prompt(&mut app, ":echo unrelated").await?;
    keys(&mut app, "<esc>:quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compilation_streams_both_pipes_and_navigates_without_stealing_focus() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let exe = fixture(dir.path())?;
    let source = dir.path().join("中文 source.txt");
    std::fs::write(&source, "abc\ndef\nghi\njkl\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    let command = shell_command(&exe, "emit", &source);
    keys(&mut app, &format!(":compile {command}<ret>")).await?;
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| r.problems.len() >= 2)
    })
    .await?;
    at(&app, &source, 0);
    assert!(
        app.editor
            .problems
            .compilation
            .current
            .as_ref()
            .unwrap()
            .running
    );
    keys(&mut app, ":next-problem --source build<ret>").await?;
    at(&app, &source, 5);
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| !r.running)
    })
    .await?;
    let run = app.editor.problems.compilation.current.as_ref().unwrap();
    assert_eq!(run.problems.len(), 3);
    assert!(run.log.contains("stderr warning"));
    assert!(run.status.contains('3'));
    assert_eq!(run.command, command);
    keys(&mut app, ":compile-log<ret>").await?;
    let log_id = current_ref!(app.editor).1.id();
    assert!(current_ref!(app.editor).1.readonly);
    assert!(current_ref!(app.editor)
        .1
        .text()
        .to_string()
        .contains("final unterminated error"));
    keys(&mut app, "u").await?;
    assert!(
        current_ref!(app.editor)
            .1
            .text()
            .to_string()
            .contains("final unterminated error"),
        "read-only log must not be undoable"
    );
    keys(&mut app, ":next-problem<ret>").await?;
    at(&app, &source, 5);
    keys(&mut app, "]q").await?;
    at(&app, &source, 8);
    // Read-only log is reused by the next run, and callbacks keep source focus.
    keys(&mut app, ":recompile<ret>").await?;
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| !r.running)
    })
    .await?;
    at(&app, &source, 8);
    assert!(app
        .editor
        .documents
        .get(&log_id)
        .unwrap()
        .text()
        .to_string()
        .contains("Finished"));
    keys(&mut app, "iX<esc>gg:next-problem<ret>").await?;
    assert!(app.editor.status_msg.as_ref().unwrap().0.contains("stale"));
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compilation_cancel_stops_descendants_and_rejects_obsolete_output() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let exe = fixture(dir.path())?;
    let heartbeat = dir.path().join("heartbeat.txt");
    let mut app = AppBuilder::new().build()?;
    keys(
        &mut app,
        &format!(":compile {}<ret>", shell_command(&exe, "hold", &heartbeat)),
    )
    .await?;
    wait_build(&mut app, |_| heartbeat.exists()).await?;
    keys(&mut app, ":compile-cancel<ret>").await?;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let stopped = std::fs::read_to_string(&heartbeat)?;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(std::fs::read_to_string(&heartbeat)?, stopped);
    let run = app.editor.problems.compilation.current.as_ref().unwrap();
    assert!(!run.running);
    assert!(run.status.contains("Canceled"));
    keys(&mut app, ":compile echo replacement<ret>").await?;
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| !r.running)
    })
    .await?;
    let run = app.editor.problems.compilation.current.as_ref().unwrap();
    assert!(run.log.contains("replacement"));
    assert!(!run.log.contains("heartbeat"));
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compilation_log_rejects_user_edits_and_survives_navigation() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "abc\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    keys(&mut app, ":compile echo log-content<ret>").await?;
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| !r.running)
    })
    .await?;
    keys(&mut app, ":compile-log<ret>").await?;
    let id = current_ref!(app.editor).1.id();
    let before = current_ref!(app.editor).1.text().to_string();
    keys(&mut app, "iX<esc>xdu").await?;
    assert_eq!(
        current_ref!(app.editor).1.text().to_string(),
        before,
        "compiler output cannot be edited"
    );
    assert!(!current_ref!(app.editor).1.is_modified());
    let source_id = app.editor.document_by_path(file.path()).unwrap().id();
    app.editor.switch(source_id, Action::Replace);
    assert!(
        app.editor.documents.contains_key(&id),
        "clean output must survive switching away"
    );
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn build_positions_reject_line_endings_and_preserve_utf16_columns() -> anyhow::Result<()> {
    use helix_lsp::{lsp, OffsetEncoding};
    use helix_view::problems::{Problem, ProblemSource};
    let dir = tempfile::tempdir()?;
    let file = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile_in(dir.path())?;
    std::fs::write(file.path(), "abc\r\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    keys(&mut app, ":compile echo ready<ret>").await?;
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| !r.running)
    })
    .await?;
    let position = lsp::Position::new(0, 4);
    let mut problem = Problem {
        path: helix_stdx::path::canonicalize(file.path()),
        range: lsp::Range::new(position, position),
        encoding: OffsetEncoding::Utf8,
        severity: Severity::Error,
        source: ProblemSource::Build,
        origin: "compiler".into(),
        message: "invalid column".into(),
    };
    app.editor
        .problems
        .compilation
        .current
        .as_mut()
        .unwrap()
        .problems = vec![problem.clone()];
    keys(&mut app, ":next-problem --source build<ret>").await?;
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("out of bounds"));
    at(&app, file.path(), 0);
    problem.path = file.path().with_file_name("missing-source.txt");
    app.editor
        .problems
        .compilation
        .current
        .as_mut()
        .unwrap()
        .problems = vec![problem];
    keys(&mut app, ":next-problem<ret>").await?;
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("does not exist"));
    // Reopen a second document whose LSP-style UTF-16 coordinates differ from chars.
    let unicode = file.path().with_file_name("problem-utf16.txt");
    std::fs::write(&unicode, "a😀bc\r\n")?;
    let position = lsp::Position::new(0, 3);
    app.editor
        .problems
        .compilation
        .current
        .as_mut()
        .unwrap()
        .problems = vec![Problem {
        path: helix_stdx::path::canonicalize(&unicode),
        range: lsp::Range::new(position, position),
        encoding: OffsetEncoding::Utf16,
        severity: Severity::Error,
        source: ProblemSource::Build,
        origin: "rustc".into(),
        message: "utf16 position".into(),
    }];
    keys(&mut app, ":next-problem<ret>").await?;
    at(&app, &unicode, 2);
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    std::fs::remove_file(&unicode)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compilation_bounds_output_and_ends_detached_descendants() -> anyhow::Result<()> {
    use helix_view::problems::{BUILD_LOG_LIMIT, BUILD_PROBLEM_LIMIT};
    let dir = tempfile::tempdir()?;
    let exe = fixture(dir.path())?;
    let source = dir.path().join("source.txt");
    std::fs::write(&source, "abc\ndef\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    keys(
        &mut app,
        &format!(":compile {}<ret>", shell_command(&exe, "flood", &source)),
    )
    .await?;
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| !r.running)
    })
    .await?;
    let run = app.editor.problems.compilation.current.as_ref().unwrap();
    assert!(run.truncated);
    assert!(run.log.len() <= BUILD_LOG_LIMIT);
    assert_eq!(run.problems.len(), BUILD_PROBLEM_LIMIT);
    keys(&mut app, ":compile-log<ret>").await?;
    assert!(current_ref!(app.editor)
        .1
        .text()
        .to_string()
        .contains("output truncated"));
    let log_id = current_ref!(app.editor).1.id();
    assert_eq!(
        app.editor
            .documents
            .get_mut(&log_id)
            .unwrap()
            .get_current_revision(),
        0
    );
    let heartbeat = dir.path().join("detached-heartbeat.txt");
    keys(
        &mut app,
        &format!(
            ":compile {}<ret>",
            shell_command(&exe, "detach", &heartbeat)
        ),
    )
    .await?;
    wait_build(&mut app, |a| {
        a.editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| !r.running)
    })
    .await?;
    let snapshot = std::fs::read(&heartbeat).ok();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(std::fs::read(&heartbeat).ok(), snapshot);
    let id = app.editor.problems.compilation.current.as_ref().unwrap().id;
    keys(
        &mut app,
        ":set-option shell [\"helix-missing-shell-executable\"]<ret>",
    )
    .await?;
    assert_eq!(
        app.editor.config().shell,
        ["helix-missing-shell-executable"]
    );
    keys(&mut app, ":compile echo failure<ret>").await?;
    assert_eq!(
        app.editor.problems.compilation.current.as_ref().unwrap().id,
        id
    );
    assert!(app
        .editor
        .status_msg
        .as_ref()
        .unwrap()
        .0
        .contains("Could not start"));
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}
