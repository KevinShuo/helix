use super::helpers::*;
use helix_view::current_ref;

async fn keys(app: &mut helix_term::application::Application, input: &str) -> anyhow::Result<()> {
    for key in helix_view::input::parse_macro(input)? {
        #[cfg(windows)]
        let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::from(key));
        #[cfg(not(windows))]
        let event = termina::event::Event::Key(termina::event::KeyEvent::from(key));
        app.handle_terminal_events(Ok(event)).await;
    }
    app.editor.reset_idle_timer();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_event_loop_until_idle(app),
    )
    .await?;
    Ok(())
}

fn screen(app: &helix_term::application::Application) -> String {
    let buffer = app.test_screen();
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folding_hides_inline_diagnostics_at_body_boundary() -> anyhow::Result<()> {
    use helix_core::diagnostic::{Diagnostic, DiagnosticProvider, Severity};
    use helix_view::annotations::diagnostics::DiagnosticFilter;
    let file = tempfile::Builder::new().suffix(".cpp").tempfile()?;
    std::fs::write(file.path(), "void f() {\n  hidden();\n}\nint tail;\n")?;
    let mut config = test_config();
    config.editor.inline_diagnostics.cursor_line = DiagnosticFilter::Enable(Severity::Hint);
    config.editor.inline_diagnostics.other_lines = DiagnosticFilter::Enable(Severity::Hint);
    config.editor.inline_diagnostics.min_diagnostic_width = 10;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    {
        let (_, doc) = helix_view::current!(app.editor);
        doc.replace_diagnostics(
            [Diagnostic {
                range: helix_core::diagnostic::Range { start: 10, end: 11 },
                starts_at_word: false,
                ends_at_word: false,
                zero_width: false,
                line: 0,
                message: "HIDDEN_BOUNDARY_DIAGNOSTIC".into(),
                severity: Some(Severity::Error),
                code: None,
                provider: DiagnosticProvider::Lsp {
                    server_id: Default::default(),
                    identifier: None,
                },
                tags: vec![],
                source: None,
                data: None,
            }],
            &[],
            None,
        );
    }
    app.test_resize(80, 16).await?;
    keys(&mut app, "za").await?;
    assert!(
        !screen(&app).contains("HIDDEN_BOUNDARY_DIAGNOSTIC"),
        "{}",
        screen(&app)
    );
    assert!(screen(&app).lines().nth(1).unwrap().contains("int tail;"));
    keys(&mut app, "zR").await?;
    assert!(
        screen(&app).contains("HIDDEN_BOUNDARY_DIAGNOSTIC"),
        "{}",
        screen(&app)
    );
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn folding_commands_and_vertical_movement() -> anyhow::Result<()> {
    test((
        "#[v|]#oid f() {\n  call();\n}\nint tail;\n",
        ":set-language cpp<ret>zaj;",
        "void f() {\n  call();\n}\n#[i|]#nt tail;\n",
    ))
    .await?;
    test((
        "#[d|]#ef f():\n    pass\nnext_call()\n",
        ":set-language python<ret>zazaj;",
        "def f():\n#[ |]#   pass\nnext_call()\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn folding_search_reveals_and_edit_undo_preserves_source() -> anyhow::Result<()> {
    test((
        "#[d|]#ef f():\n    target()\nnext_call()\n",
        ":set-language python<ret>za/target<ret>",
        "def f():\n    #[target|]#()\nnext_call()\n",
    ))
    .await?;
    test((
        "#[v|]#oid f() {\n  call();\n}\nint tail;\n",
        ":set-language cpp<ret>za%dugg;",
        "#[v|]#oid f() {\n  call();\n}\nint tail;\n",
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn folding_renders_complete_signatures_and_keeps_text_clean() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".py").tempfile()?;
    std::fs::write(
        file.path(),
        "def f(\n    x,\n):\n    hidden_call(x)\nnext_call()\n",
    )?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":set-language python<ret>za"),
                Some(&|app| {
                    let (view, doc) = current_ref!(app.editor);
                    assert_eq!(doc.folds(view.id).len(), 1);
                    assert!(!doc.is_modified());
                    let screen = app.test_screen();
                    let text = screen
                        .content
                        .iter()
                        .map(|cell| cell.symbol.as_str())
                        .collect::<String>();
                    assert!(text.contains("def f("));
                    assert!(text.contains("x,"));
                    assert!(text.contains("):…"));
                    assert!(text.contains("next_call()"));
                    assert!(!text.contains("hidden_call"));
                }),
            ),
            (
                Some("zR"),
                Some(&|app| {
                    let (view, doc) = current_ref!(app.editor);
                    assert!(doc.folds(view.id).is_empty());
                    assert!(!doc.is_modified());
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folding_gutter_mouse_splits_buffers_and_resize() -> anyhow::Result<()> {
    use helix_view::editor::GutterType;
    let file = tempfile::Builder::new().suffix(".cpp").tempfile()?;
    let source = "void f() {\n  hidden();\n}\nint tail;\n";
    std::fs::write(file.path(), source)?;
    let mut config = test_config();
    config.editor.gutters.layout = vec![GutterType::Fold, GutterType::LineNumbers];
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    app.test_resize(60, 16).await?;
    keys(&mut app, "gg").await?;
    assert!(screen(&app).contains('▾'));
    let view_id = current_ref!(app.editor).0.id;
    #[cfg(windows)]
    let event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    #[cfg(not(windows))]
    let event = termina::event::Event::Mouse(termina::event::MouseEvent {
        kind: termina::event::MouseEventKind::Down(termina::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: termina::event::KeyModifiers::NONE,
    });
    app.handle_terminal_events(Ok(event)).await;
    keys(&mut app, "").await?;
    assert!(screen(&app).contains("void f() {…}"), "{}", screen(&app));
    assert!(screen(&app).contains('▸'));
    keys(&mut app, ":vsplit<ret>").await?;
    let (new_view, doc) = current_ref!(app.editor);
    assert_ne!(new_view.id, view_id);
    assert!(doc.folds(new_view.id).is_empty());
    assert_eq!(doc.folds(view_id).len(), 1);
    keys(&mut app, "<C-w>q:new<ret>:buffer-previous<ret>").await?;
    assert_eq!(current_ref!(app.editor).1.folds(view_id).len(), 1);
    app.test_resize(24, 8).await?;
    keys(&mut app, "j").await?;
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(
        doc.text().char_to_line(
            doc.selection(view.id)
                .primary()
                .cursor(doc.text().slice(..))
        ),
        3,
        "view={:?} original={view_id:?} folds={:?}\n{}",
        view.id,
        doc.folds(view.id),
        screen(&app)
    );
    assert_eq!(doc.folds(view.id).len(), 1);
    keys(&mut app, ":write<ret>").await?;
    assert_eq!(std::fs::read_to_string(file.path())?, source);
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folding_large_function_keeps_following_syntax_colors() -> anyhow::Result<()> {
    let file = tempfile::Builder::new().suffix(".cpp").tempfile()?;
    let source = format!(
        "void f() {{\n{}}}\nint tail = 42;\n",
        "  int hidden = 0;\n".repeat(3000)
    );
    std::fs::write(file.path(), source)?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    app.test_resize(80, 20).await?;
    keys(&mut app, "ge").await?;
    let tail_colors = |app: &helix_term::application::Application| {
        let buffer = app.test_screen();
        buffer
            .content
            .chunks(buffer.area.width as usize)
            .find_map(|row| {
                let at = row.windows(3).position(|cells| {
                    cells[0].symbol.as_str() == "i"
                        && cells[1].symbol.as_str() == "n"
                        && cells[2].symbol.as_str() == "t"
                })?;
                let line: String = row.iter().map(|cell| cell.symbol.as_str()).collect();
                line.contains("int tail = 42;").then(|| {
                    row[at + 1..at + 14]
                        .iter()
                        .map(|cell| (cell.fg, cell.bg, cell.modifier))
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_else(|| panic!("tail must be visible:\n{}", screen(app)))
    };
    let colors = tail_colors(&app);
    keys(&mut app, "ggza").await?;
    assert!(screen(&app).contains("void f() {…}"));
    assert!(!screen(&app).contains("hidden"));
    assert_eq!(tail_colors(&app), colors);
    let (view, doc) = current_ref!(app.editor);
    assert!(
        view.visible_text_ranges(doc)
            .iter()
            .map(|range| range.len())
            .sum::<usize>()
            < 50
    );
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}
