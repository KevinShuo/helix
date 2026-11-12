use std::{fs, path::Path, process::Command, time::Duration};

use helix_core::doc_formatter::{DocumentFormatter, TextFormat};
use helix_view::{current_ref, function_hints::FunctionHintsConfig};

use super::helpers::*;

fn virtual_text(app: &helix_term::application::Application) -> String {
    let (view, doc) = current_ref!(app.editor);
    let annotations = view.text_annotations(doc, None);
    DocumentFormatter::new_at_prev_checkpoint(
        doc.text().slice(..),
        &TextFormat::default(),
        &annotations,
        0,
    )
    .filter(|grapheme| grapheme.is_virtual())
    .map(|grapheme| grapheme.raw.to_string())
    .collect()
}

async fn scroll(app: &mut helix_term::application::Application, down: bool) {
    #[cfg(windows)]
    use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
    #[cfg(not(windows))]
    use termina::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
    app.handle_terminal_events(Ok(Event::Mouse(MouseEvent {
        kind: if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        column: 10,
        row: 5,
        modifiers: KeyModifiers::NONE,
    })))
    .await;
    app.handle_idle_timeout().await;
}

fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn git(root: &Path, args: &[&str]) {
    let output = hidden(
        Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Hint Author",
                "-c",
                "user.email=hints@example.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=",
            ])
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .env_remove("GIT_NAMESPACE")
            .env_remove("GIT_CONFIG_COUNT")
            .env("GIT_AUTHOR_NAME", "Hint Author")
            .env("GIT_AUTHOR_EMAIL", "hints@example.com")
            .env("GIT_COMMITTER_NAME", "Hint Author")
            .env("GIT_COMMITTER_EMAIL", "hints@example.com")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            ),
    )
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn function_hints_flow_from_lsp_and_git_to_virtual_text() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let executable = temp.path().join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/function_hints_lsp.rs");
    let output = hidden(
        Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--edition=2021")
            .arg(source)
            .arg("-o")
            .arg(&executable),
    )
    .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = format!(
        "def hello():\n    return 1\n\nhello()\nhello()\n{}",
        "\n".repeat(80)
    );
    let file = temp.path().join("example.py");
    fs::write(&file, &text)?;
    git(temp.path(), &["init", "--initial-branch=main"]);
    git(temp.path(), &["add", "example.py"]);
    git(temp.path(), &["commit", "-m", "Define hello"]);
    let log_path = temp.path().join("requests.jsonl");
    let overrides = format!(
        r#"
[language-server.function-hints-fixture]
command = {}
args = [{}]
[[language]]
name = "python"
language-servers = ["function-hints-fixture"]
"#,
        toml::Value::String(executable.to_string_lossy().into_owned()),
        toml::Value::String(log_path.to_string_lossy().into_owned())
    );
    let mut config = test_config();
    config.editor.lsp.enable = true;
    config.editor.function_hints = FunctionHintsConfig {
        references: true,
        git_author: true,
    };
    config.editor.idle_timeout = Duration::from_millis(20);
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_lang_loader(test_syntax_loader(Some(overrides)))
        .with_file(&file, None)
        .build()?;
    app.test_resize(80, 16).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        app.editor.reset_idle_timer();
        tokio::time::timeout(Duration::from_secs(3), run_event_loop_until_idle(&mut app)).await?;
        let (view, doc) = current_ref!(app.editor);
        if doc
            .function_hints
            .get(&view.id)
            .is_some_and(|h| !h.references.is_empty() && !h.authors.is_empty())
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "function hints did not arrive; requests: {}",
            fs::read_to_string(&log_path).unwrap_or_default()
        );
    }
    let (view, doc) = current_ref!(app.editor);
    let annotations = view.text_annotations(doc, None);
    let displayed: String = DocumentFormatter::new_at_prev_checkpoint(
        doc.text().slice(..),
        &TextFormat::default(),
        &annotations,
        0,
    )
    .filter(|grapheme| grapheme.is_virtual())
    .map(|grapheme| grapheme.raw.to_string())
    .collect();
    assert_eq!(displayed, "  2 usages  Hint Author");
    assert_eq!(doc.text().to_string(), text);
    assert_eq!(fs::read_to_string(&file)?, text);
    drop(annotations);
    assert!(
        app.editor.idle_timer.deadline() < tokio::time::Instant::now() + Duration::from_secs(16),
        "idle refresh must be rearmed"
    );
    // Re-entering idle immediately reuses the same snapshot, with no repeated LSP search.
    app.editor.reset_idle_timer();
    tokio::time::timeout(Duration::from_secs(3), run_event_loop_until_idle(&mut app)).await?;
    let requests = fs::read_to_string(&log_path)?;
    assert_eq!(requests.matches("textDocument/references").count(), 1);

    // Leave the annotated function, finish the empty viewport request, and
    // return repeatedly. Cached text must survive both the scroll and its result.
    for down in [true, false, true, false] {
        scroll(&mut app, down).await;
        let (view, doc) = current_ref!(app.editor);
        assert_eq!(
            doc.view_offset(view.id).anchor == 0,
            !down,
            "mouse wheel did not scroll"
        );
        assert_eq!(
            virtual_text(&app),
            "  2 usages  Hint Author",
            "scroll refresh hid cached hints"
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.editor.reset_idle_timer();
            tokio::time::timeout(Duration::from_secs(3), run_event_loop_until_idle(&mut app))
                .await?;
            let (view, doc) = current_ref!(app.editor);
            if !doc.function_hints[&view.id].controller.is_running() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "scroll refresh did not complete"
            );
        }
        assert_eq!(
            virtual_text(&app),
            "  2 usages  Hint Author",
            "viewport result discarded offscreen hints"
        );
    }

    // A periodic refresh must keep the current virtual text visible until its
    // replacement arrives, including when an external commit changes the author.
    git(
        temp.path(),
        &[
            "commit",
            "--amend",
            "--no-edit",
            "--author=New Author <new@example.com>",
        ],
    );
    let (view, doc) = helix_view::current!(app.editor);
    doc.function_hints.get_mut(&view.id).unwrap().requested_at =
        std::time::Instant::now() - Duration::from_secs(16);
    app.handle_idle_timeout().await;
    let (view, doc) = current_ref!(app.editor);
    let annotations = view.text_annotations(doc, None);
    let displayed: String = DocumentFormatter::new_at_prev_checkpoint(
        doc.text().slice(..),
        &TextFormat::default(),
        &annotations,
        0,
    )
    .filter(|grapheme| grapheme.is_virtual())
    .map(|grapheme| grapheme.raw.to_string())
    .collect();
    assert_eq!(
        displayed, "  2 usages  Hint Author",
        "refresh must not clear the displayed hints while requests are pending"
    );
    drop(annotations);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        app.editor.reset_idle_timer();
        tokio::time::timeout(Duration::from_secs(3), run_event_loop_until_idle(&mut app)).await?;
        let (view, doc) = current_ref!(app.editor);
        if doc.function_hints.get(&view.id).is_some_and(|hints| {
            hints
                .authors
                .first()
                .is_some_and(|a| a.text == "  New Author")
                && !hints.controller.is_running()
        }) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "refreshed Git author did not arrive"
        );
    }

    // A successful empty symbol result must still remove previous annotations.
    fs::write(log_path.with_extension("symbols"), "null")?;
    let (view, doc) = helix_view::current!(app.editor);
    doc.function_hints.get_mut(&view.id).unwrap().requested_at =
        std::time::Instant::now() - Duration::from_secs(16);
    app.handle_idle_timeout().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        app.editor.reset_idle_timer();
        tokio::time::timeout(Duration::from_secs(3), run_event_loop_until_idle(&mut app)).await?;
        let (view, doc) = current_ref!(app.editor);
        let hints = &doc.function_hints[&view.id];
        if !hints.controller.is_running() {
            assert!(
                hints.references.is_empty(),
                "stale reference hints remained"
            );
            assert!(hints.authors.is_empty(), "stale Git author hints remained");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "empty symbol refresh did not complete"
        );
    }
    app.close().await;
    Ok(())
}
