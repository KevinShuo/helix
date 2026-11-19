use super::helpers::*;
use helix_core::{diagnostic::DiagnosticProvider, Transaction};
use helix_lsp::{jsonrpc, lsp};
use helix_view::{current, current_ref};

fn provider() -> DiagnosticProvider {
    DiagnosticProvider::Lsp {
        server_id: Default::default(),
        identifier: None,
    }
}

fn error() -> lsp::Diagnostic {
    lsp::Diagnostic::new_simple(
        lsp::Range::new(lsp::Position::new(0, 13), lsp::Position::new(0, 14)),
        "Extraneous closing brace ('}')".into(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delayed_clear_after_editing_removes_old_diagnostics() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "namespace ws };\r\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    let uri = current_ref!(app.editor).1.uri().unwrap();
    let old_version = current_ref!(app.editor).1.version();
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), Some(old_version), vec![error()]);
    {
        let (_, doc) = current!(app.editor);
        let diagnostic = helix_view::Document::lsp_diagnostic_to_diagnostic(
            doc.text(),
            None,
            &error(),
            provider(),
            helix_lsp::OffsetEncoding::Utf16,
        )
        .unwrap();
        doc.replace_diagnostics([diagnostic], &[], Some(&provider()));
    }
    let fixed_version = {
        let (view, doc) = current!(app.editor);
        let fix = Transaction::change(doc.text(), [(13, 13, Some("{".into()))].into_iter());
        assert!(doc.apply(&fix, view.id));
        doc.append_changes_to_history(view);
        let version = doc.version();
        let next_edit = Transaction::change(doc.text(), [(0, 0, Some(" ".into()))].into_iter());
        assert!(doc.apply(&next_edit, view.id));
        assert!(doc.undo(view));
        assert_eq!(doc.text().to_string(), "namespace ws {};\r\n");
        version
    };
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), Some(fixed_version), vec![]);
    assert!(
        app.editor.diagnostics[&uri].is_empty(),
        "Delayed clear must remove the older error after subsequent typing"
    );
    assert!(current_ref!(app.editor).1.diagnostics().is_empty());
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), Some(old_version), vec![error()]);
    assert!(
        app.editor.diagnostics[&uri].is_empty(),
        "An older error must not resurrect after a newer clear"
    );
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn older_clear_does_not_erase_newer_errors() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "namespace ws };\r\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    let uri = current_ref!(app.editor).1.uri().unwrap();
    let old_version = current_ref!(app.editor).1.version();
    {
        let (view, doc) = current!(app.editor);
        let edit = Transaction::change(doc.text(), [(0, 0, Some(" ".into()))].into_iter());
        assert!(doc.apply(&edit, view.id));
    }
    let new_version = current_ref!(app.editor).1.version();
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), Some(new_version), vec![error()]);
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), Some(old_version), vec![]);
    assert_eq!(app.editor.diagnostics[&uri].len(), 1);
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), Some(new_version + 1), vec![]);
    assert_eq!(
        app.editor.diagnostics[&uri].len(),
        1,
        "Future versions must still be rejected"
    );
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn older_clear_does_not_erase_unversioned_errors() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "namespace ws };\r\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    let uri = current_ref!(app.editor).1.uri().unwrap();
    let old_version = current_ref!(app.editor).1.version();
    {
        let (view, doc) = current!(app.editor);
        let edit = Transaction::change(doc.text(), [(0, 0, Some(" ".into()))].into_iter());
        assert!(doc.apply(&edit, view.id));
    }
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), None, vec![error()]);
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), Some(old_version), vec![]);
    assert_eq!(
        app.editor.diagnostics[&uri].len(),
        1,
        "An unversioned report must conservatively supersede older versioned clears"
    );
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exit_of_already_removed_server_clears_its_diagnostics() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    std::fs::write(file.path(), "namespace ws {};\r\n")?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    let uri = current_ref!(app.editor).1.uri().unwrap();
    app.editor
        .handle_lsp_diagnostics(&provider(), uri.clone(), None, vec![error()]);
    // restart_server removes the client from the registry before its exit arrives.
    app.handle_language_server_message(
        jsonrpc::Call::Notification(jsonrpc::Notification {
            jsonrpc: None,
            method: "exit".into(),
            params: jsonrpc::Params::None,
        }),
        Default::default(),
    )
    .await;
    assert!(
        !app.editor.diagnostics.contains_key(&uri),
        "A retired server must not leave diagnostics behind"
    );
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_immediately_clears_retired_server_diagnostics() -> anyhow::Result<()> {
    use std::time::Duration;
    let dir = tempfile::tempdir()?;
    let exe = compile_fixture("diagnostics_lsp", dir.path())?;
    let file = dir.path().join("example.py");
    std::fs::write(&file, "namespace ws {};\r\n")?;
    let log = dir.path().join("requests.jsonl");
    let overrides = format!(
        r#"
[language-server.diagnostics-fixture]
command = {}
args = [{}]
[[language]]
name = "python"
language-servers = ["diagnostics-fixture"]
"#,
        toml::Value::String(exe.to_string_lossy().into_owned()),
        toml::Value::String(log.to_string_lossy().into_owned())
    );
    let mut config = test_config();
    config.editor.lsp.enable = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_lang_loader(test_syntax_loader(Some(overrides)))
        .with_file(&file, None)
        .build()?;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            run_event_loop_until_idle(&mut app).await;
            if std::fs::read_to_string(&log)
                .unwrap_or_default()
                .contains("textDocument/diagnostic")
            {
                break;
            }
        }
    })
    .await?;
    let server_id = current_ref!(app.editor)
        .1
        .language_servers()
        .next()
        .unwrap()
        .id();
    let uri = current_ref!(app.editor).1.uri().unwrap();
    let provider = DiagnosticProvider::Lsp {
        server_id,
        identifier: None,
    };
    app.editor
        .handle_lsp_diagnostics(&provider, uri.clone(), None, vec![error()]);
    assert_eq!(current_ref!(app.editor).1.diagnostics().len(), 1);
    for key in helix_view::input::parse_macro(":lsp-restart<ret>")? {
        #[cfg(windows)]
        let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::from(key));
        #[cfg(not(windows))]
        let event = termina::event::Event::Key(termina::event::KeyEvent::from(key));
        app.handle_terminal_events(Ok(event)).await;
    }
    assert!(
        app.editor
            .diagnostics
            .get(&uri)
            .is_none_or(|diags| diags.iter().all(|(_, p)| p != &provider)),
        "Restart must immediately clear the retired server"
    );
    // Drain queued shutdown messages; old results must not reappear.
    run_event_loop_until_idle(&mut app).await;
    assert!(
        app.editor
            .diagnostics
            .get(&uri)
            .is_none_or(|diags| diags.iter().all(|(_, p)| p != &provider)),
        "Retired pull response repopulated the diagnostic cache"
    );
    assert!(!current_ref!(app.editor)
        .1
        .previous_diagnostic_ids
        .contains_key(&server_id));
    assert!(current_ref!(app.editor).1.diagnostics().is_empty());
    app.close().await;
    Ok(())
}
