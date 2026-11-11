use std::{fs, path::Path, process::Command, time::Duration};

use helix_core::doc_formatter::{DocumentFormatter, TextFormat};
use helix_view::{current_ref, function_hints::FunctionHintsConfig};

use super::helpers::*;

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

    let text = "def hello():\n    return 1\n\nhello()\nhello()\n";
    let file = temp.path().join("example.py");
    fs::write(&file, text)?;
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
    app.close().await;
    Ok(())
}
