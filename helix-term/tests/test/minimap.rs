use super::helpers::*;
use helix_term::application::Application;
use helix_view::{current_ref, graphics::Rect};

async fn keys(app: &mut Application, input: &str) -> anyhow::Result<()> {
    for key in helix_view::input::parse_macro(input)? {
        #[cfg(windows)]
        let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::from(key));
        #[cfg(not(windows))]
        let event = termina::event::Event::Key(termina::event::KeyEvent::from(key));
        app.handle_terminal_events(Ok(event)).await;
    }
    app.editor.reset_idle_timer();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_event_loop_until_idle(app),
    )
    .await?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Mouse {
    Down,
    Drag,
    Up,
    Wheel,
    Right,
    Middle,
}

async fn mouse(app: &mut Application, kind: Mouse, row: u16, column: u16) -> anyhow::Result<()> {
    #[cfg(windows)]
    let event = {
        use crossterm::event::{Event, MouseButton::*, MouseEvent, MouseEventKind::*};
        Event::Mouse(MouseEvent {
            kind: match kind {
                Mouse::Down => Down(Left),
                Mouse::Drag => Drag(Left),
                Mouse::Up => Up(Left),
                Mouse::Wheel => ScrollDown,
                Mouse::Right => Up(Right),
                Mouse::Middle => Up(Middle),
            },
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        })
    };
    #[cfg(not(windows))]
    let event = {
        use termina::event::{Event, MouseButton::*, MouseEvent, MouseEventKind::*};
        Event::Mouse(MouseEvent {
            kind: match kind {
                Mouse::Down => Down(Left),
                Mouse::Drag => Drag(Left),
                Mouse::Up => Up(Left),
                Mouse::Wheel => ScrollDown,
                Mouse::Right => Up(Right),
                Mouse::Middle => Up(Middle),
            },
            column,
            row,
            modifiers: termina::event::KeyModifiers::NONE,
        })
    };
    app.handle_terminal_events(Ok(event)).await;
    keys(app, "").await
}

fn area(app: &Application) -> Rect {
    let (view, doc) = current_ref!(app.editor);
    view.minimap_layout(doc).map.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_navigation_recovers_cursor_from_horizontal_scroll() -> anyhow::Result<()> {
    let text = format!("long_name = '{}'\n{}", "x".repeat(350), source(500));
    let (mut app, _file) = app_with(&text).await?;
    keys(&mut app, "300l").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(doc.view_offset(view.id).horizontal_offset > 100);
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.y + r.height / 2, r.x + 3).await?;
    mouse(&mut app, Mouse::Up, r.y + r.height / 2, r.x + 3).await?;
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(doc.view_offset(view.id).horizontal_offset, 0);
    assert!(doc.text().char_to_line(doc.view_offset(view.id).anchor) > 150);
    assert!(!doc.is_modified());
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_degraded_summary_resamples_when_tiny_view_grows() -> anyhow::Result<()> {
    let text = "value = 1\n".repeat(50_001);
    let file = tempfile::Builder::new().suffix(".py").tempfile()?;
    std::fs::write(file.path(), text)?;
    let mut config = test_config();
    config.editor.minimap.enabled = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    app.test_resize(120, 6).await?;
    keys(&mut app, "gg").await?;
    let (view, doc) = current_ref!(app.editor);
    let initial = doc.minimap.summary.as_ref().unwrap().rows.len();
    assert_eq!(view.minimap_layout(doc).content.unwrap().height, 4);
    assert!(initial <= 128);
    app.test_resize(120, 40).await?;
    keys(&mut app, "").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(doc.minimap.summary.as_ref().unwrap().rows.len() > initial);
    let content = view.minimap_layout(doc).content.unwrap();
    let projection = doc
        .minimap
        .projections
        .iter()
        .find(|p| p.key.height == content.height)
        .unwrap();
    assert!(
        projection
            .cells
            .chunks(content.width as usize)
            .all(|r| r.iter().any(|c| c.dots != 0)),
        "uniformly occupied source must not acquire blank stripes"
    );
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual timing measurement; no machine-dependent CI thresholds"]
async fn minimap_measure_cache_and_rendering() -> anyhow::Result<()> {
    use std::{sync::Arc, time::Instant};
    let file = tempfile::Builder::new().suffix(".py").tempfile()?;
    std::fs::write(file.path(), source(10_000))?;
    let mut config = test_config();
    config.editor.minimap.enabled = true;
    let cold = Instant::now();
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    app.test_resize(120, 40).await?;
    loop {
        keys(&mut app, "").await?;
        if current_ref!(app.editor).1.minimap.summary.is_some() {
            break;
        }
        anyhow::ensure!(
            cold.elapsed().as_secs() < 15,
            "source summary did not finish"
        );
    }
    let cold_ms = cold.elapsed().as_secs_f64() * 1000.0;
    let summary = current_ref!(app.editor)
        .1
        .minimap
        .summary
        .as_ref()
        .unwrap()
        .clone();
    let rows_bytes =
        summary.rows.capacity() * std::mem::size_of::<(usize, helix_view::minimap::SourceRow)>();
    let start = Instant::now();
    for _ in 0..100 {
        app.test_resize(120, 40).await?;
    }
    let enabled_ms = start.elapsed().as_secs_f64() * 10.0;
    assert!(Arc::ptr_eq(
        &summary,
        current_ref!(app.editor).1.minimap.summary.as_ref().unwrap()
    ));
    keys(&mut app, ":set-option minimap.enabled false<ret>").await?;
    let start = Instant::now();
    for _ in 0..100 {
        app.test_resize(120, 40).await?;
    }
    let disabled_ms = start.elapsed().as_secs_f64() * 10.0;
    println!("minimap TestBackend: lines={}, cold_open_and_summary_ms={cold_ms:.2}, row_storage_bytes={rows_bytes}, redraw_on_ms={enabled_ms:.3}, redraw_off_ms={disabled_ms:.3}, cached_frames=100", summary.lines);
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_multiline_syntax_survives_cooperative_batch_boundaries() -> anyhow::Result<()> {
    let text = format!(
        "text = \"\"\"\n{}\"\"\"\n",
        "inside_a_string\n".repeat(1000)
    );
    let (mut app, _file) = app_with(&text).await?;
    let string = app.editor.theme.find_highlight("string").unwrap();
    let summary = current_ref!(app.editor).1.minimap.summary.as_ref().unwrap();
    for (line, row) in &summary.rows {
        if (1..1001).contains(line) {
            assert_eq!(row.highlight(0), Some(string), "source line {line}");
        }
    }
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

fn source(lines: usize) -> String {
    (0..lines)
        .map(|i| format!("value_{i} = '中文 hello 😀' # comment\n"))
        .collect()
}

async fn app_with(source: &str) -> anyhow::Result<(Application, tempfile::NamedTempFile)> {
    let file = tempfile::Builder::new().suffix(".py").tempfile()?;
    std::fs::write(file.path(), source)?;
    let mut config = test_config();
    config.editor.minimap.enabled = true;
    config.editor.mouse_yank_register = 'm';
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(file.path(), None)
        .build()?;
    app.test_resize(120, 32).await?;
    keys(&mut app, "gg").await?;
    Ok((app, file))
}

fn map_symbols(app: &Application) -> String {
    let (view, doc) = current_ref!(app.editor);
    let r = view.minimap_layout(doc).content.unwrap();
    let screen = app.test_screen();
    (r.y..r.bottom())
        .flat_map(|y| (r.x..r.right()).map(move |x| (x, y)))
        .map(|p| screen[p].symbol.as_str())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_render_switch_resize_and_disable() -> anyhow::Result<()> {
    let (mut app, _file) = app_with(&source(80)).await?;
    assert!(map_symbols(&app)
        .chars()
        .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)));
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(view.inner_width(doc), view.inner_area(doc).width);
    let summary = doc
        .minimap
        .summary
        .as_ref()
        .expect("summary published")
        .clone();
    assert!(summary
        .rows
        .iter()
        .any(|(_, row)| (0..160).any(|col| row.highlight(col).is_some())));
    keys(&mut app, "jjjj").await?;
    assert!(std::sync::Arc::ptr_eq(
        &summary,
        current_ref!(app.editor).1.minimap.summary.as_ref().unwrap()
    ));
    keys(&mut app, ":set-option minimap.render blocks<ret>").await?;
    assert!(!map_symbols(&app)
        .chars()
        .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)));
    assert!(map_symbols(&app)
        .chars()
        .any(|c| ('\u{2580}'..='\u{259f}').contains(&c)));
    app.test_resize(70, 20).await?;
    keys(&mut app, "").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(view.minimap_layout(doc).map.is_none());
    assert!(doc.minimap.summary.is_none());
    app.test_resize(120, 32).await?;
    keys(&mut app, "").await?;
    assert!(current_ref!(app.editor).1.minimap.summary.is_some());
    keys(&mut app, ":toggle-option minimap.enabled<ret>").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(view.minimap_layout(doc).map.is_none());
    assert!(!doc.minimap.pending && doc.minimap.summary.is_none());
    assert!(!doc.is_modified());
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_click_drag_and_release_navigate_without_editing() -> anyhow::Result<()> {
    let text = source(1000);
    let (mut app, _file) = app_with(&text).await?;
    let r = area(&app);
    let version = current_ref!(app.editor).1.version();
    mouse(&mut app, Mouse::Down, r.y + r.height / 2, r.x + 3).await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(doc.text().char_to_line(doc.view_offset(view.id).anchor) > 300);
    mouse(
        &mut app,
        Mouse::Drag,
        r.bottom() + 2,
        r.x.saturating_sub(10),
    )
    .await?;
    mouse(&mut app, Mouse::Up, r.bottom() + 2, r.x.saturating_sub(10)).await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(
        doc.text().char_to_line(
            doc.selection(view.id)
                .primary()
                .cursor(doc.text().slice(..))
        ) > 900
    );
    assert_eq!(doc.text().to_string(), text);
    assert_eq!(doc.version(), version);
    assert!(!doc.is_modified());
    // A release must end capture: subsequent ordinary dragging in the map does nothing.
    let old = doc.view_offset(view.id);
    mouse(&mut app, Mouse::Drag, r.y, r.x + 3).await?;
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(doc.view_offset(view.id), old);
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_click_reveals_folded_source_inside_viewport_band() -> anyhow::Result<()> {
    let text = format!(
        "def giant():\n{}\nnext_call()\n",
        (0..600)
            .map(|i| format!("    value_{i} = {i}\n"))
            .collect::<String>()
    );
    let (mut app, _file) = app_with(&text).await?;
    keys(&mut app, "za").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(!doc.folds(view.id).is_empty());
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.y + r.height / 2, r.x + 4).await?;
    mouse(&mut app, Mouse::Up, r.y + r.height / 2, r.x + 4).await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(
        doc.folds(view.id).is_empty(),
        "click must reveal hidden destination even inside the viewport envelope"
    );
    assert!(doc.text().char_to_line(doc.view_offset(view.id).anchor) > 200);
    assert_eq!(doc.text().to_string(), text);
    assert!(!doc.is_modified());
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_select_mode_secondary_ranges_and_canceled_release() -> anyhow::Result<()> {
    use helix_core::{Range, Selection};
    let (mut app, _file) = app_with(&source(1000)).await?;
    let (view, doc) = helix_view::current!(app.editor);
    doc.set_selection(
        view.id,
        Selection::point(0).push(Range::point(doc.text().line_to_char(10))),
    );
    let secondary = *doc
        .selection(view.id)
        .iter()
        .find(|r| **r != doc.selection(view.id).primary())
        .unwrap();
    let anchor = doc.selection(view.id).primary().anchor;
    keys(&mut app, "v").await?;
    app.editor.registers.write('m', vec!["untouched".into()])?;
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.y + r.height / 2, r.x + 3).await?;
    let (view, doc) = current_ref!(app.editor);
    assert_eq!(doc.selection(view.id).primary().anchor, anchor);
    assert!(doc.selection(view.id).iter().any(|r| *r == secondary));
    // Cancel while captured and release over the body. This must not yank the selection.
    keys(&mut app, ":set-option minimap.enabled false<ret>").await?;
    mouse(&mut app, Mouse::Up, 2, 10).await?;
    assert_eq!(
        app.editor
            .registers
            .read('m', &app.editor)
            .unwrap()
            .next()
            .unwrap(),
        "untouched"
    );
    keys(&mut app, ":set-option minimap.enabled true<ret><esc>").await?;
    let r = area(&app);
    let before = current_ref!(app.editor).1.text().to_string();
    mouse(&mut app, Mouse::Right, r.y, r.x + 4).await?;
    mouse(&mut app, Mouse::Middle, r.y, r.x + 4).await?;
    mouse(&mut app, Mouse::Wheel, r.y, r.x + 4).await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), before);
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_split_views_share_source_but_keep_independent_offsets() -> anyhow::Result<()> {
    let (mut app, _file) = app_with(&source(1000)).await?;
    keys(&mut app, ":hsplit<ret>").await?;
    let (view, doc) = current_ref!(app.editor);
    let active = view.id;
    let other = app
        .editor
        .tree
        .views()
        .find(|(v, _)| v.id != active)
        .unwrap()
        .0
        .id;
    let old_other = doc.view_offset(other);
    let summary = doc.minimap.summary.as_ref().unwrap().clone();
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.y + r.height / 2, r.x + 4).await?;
    mouse(&mut app, Mouse::Up, r.y + r.height / 2, r.x + 4).await?;
    let doc = current_ref!(app.editor).1;
    assert_eq!(doc.view_offset(other), old_other);
    assert_ne!(doc.view_offset(active), old_other);
    assert!(std::sync::Arc::ptr_eq(
        &summary,
        doc.minimap.summary.as_ref().unwrap()
    ));
    let r = app.editor.tree.get(other).minimap_layout(doc).map.unwrap();
    mouse(&mut app, Mouse::Wheel, r.y + 1, r.x + 3).await?;
    assert_eq!(
        app.editor.tree.focus, active,
        "wheel must preserve keyboard focus"
    );
    let doc = current_ref!(app.editor).1;
    assert_ne!(doc.view_offset(other), old_other);
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_softwrap_and_virtual_annotations_do_not_enter_source_summary() -> anyhow::Result<()>
{
    use helix_core::text_annotations::InlineAnnotation;
    use helix_view::ai_completion::AiCompletion;
    let text = format!("name = '{}中文😀'\nnext_call()\n", "long text ".repeat(50));
    let (mut app, _file) = app_with(&text).await?;
    keys(&mut app, ":set-option soft-wrap.enable true<ret>i").await?;
    let (view, doc) = helix_view::current!(app.editor);
    let summary = doc.minimap.summary.as_ref().unwrap().clone();
    assert_eq!(summary.lines, 3);
    doc.ai_completion = Some(AiCompletion {
        view: view.id,
        version: doc.version(),
        annotation: InlineAnnotation::new(0, "virtual preview\nsecond virtual line\n"),
    });
    assert!(doc.ai_completion.as_ref().unwrap().is_valid(doc, view.id));
    app.test_resize(120, 32).await?;
    keys(&mut app, "").await?;
    let (view, doc) = current_ref!(app.editor);
    assert!(std::sync::Arc::ptr_eq(
        &summary,
        doc.minimap.summary.as_ref().unwrap()
    ));
    assert_eq!(doc.text().to_string(), text);
    assert_eq!(view.inner_area(doc).width, view.inner_width(doc));
    keys(&mut app, "<esc>:quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_language_theme_edit_and_reload_invalidate_source_colors() -> anyhow::Result<()> {
    let text = source(40);
    let (mut app, _file) = app_with(&text).await?;
    let original = current_ref!(app.editor)
        .1
        .minimap
        .summary
        .as_ref()
        .unwrap()
        .clone();
    keys(&mut app, ":set-language text<ret>").await?;
    let plain = current_ref!(app.editor)
        .1
        .minimap
        .summary
        .as_ref()
        .unwrap()
        .clone();
    assert!(!std::sync::Arc::ptr_eq(&original, &plain));
    assert_eq!(original.key.version, plain.key.version);
    assert!(plain
        .rows
        .iter()
        .all(|(_, row)| (0..160).all(|c| row.highlight(c).is_none())));
    keys(
        &mut app,
        ":set-language python<ret>:theme base16_default<ret>",
    )
    .await?;
    let colored = current_ref!(app.editor)
        .1
        .minimap
        .summary
        .as_ref()
        .unwrap()
        .clone();
    assert_ne!(plain.key.epoch, colored.key.epoch);
    assert!(colored
        .rows
        .iter()
        .any(|(_, row)| (0..160).any(|c| row.highlight(c).is_some())));
    keys(&mut app, "i# new line<ret><esc>").await?;
    let changed = current_ref!(app.editor).1.minimap.summary.as_ref().unwrap();
    assert_ne!(changed.key.version, colored.key.version);
    assert_eq!(changed.lines, colored.lines + 1);
    keys(&mut app, "u").await?;
    assert_eq!(current_ref!(app.editor).1.text().to_string(), text);
    keys(&mut app, ":reload<ret>").await?;
    let doc = current_ref!(app.editor).1;
    assert_eq!(doc.text().to_string(), text);
    assert_eq!(
        doc.minimap.summary.as_ref().unwrap().key.version,
        doc.version()
    );
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_large_files_sample_and_disable_cancels_pending_work() -> anyhow::Result<()> {
    let text = "# comment with a little text\n".repeat(50_001);
    let (mut app, _file) = app_with(&text).await?;
    let doc = current_ref!(app.editor).1;
    let summary = doc.minimap.summary.as_ref().unwrap();
    assert!(summary.degraded);
    assert!(summary.rows.len() <= 4096);
    assert_eq!(summary.rows.first().unwrap().0, 0);
    assert_eq!(summary.rows.last().unwrap().0, doc.text().len_lines() - 1);
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.bottom() - 1, r.x + 4).await?;
    mouse(&mut app, Mouse::Up, r.bottom() - 1, r.x + 4).await?;
    assert!(!current_ref!(app.editor).1.is_modified());
    // Start an edit without waiting for its debounce, then disable before publication.
    for key in helix_view::input::parse_macro("i#").unwrap() {
        #[cfg(windows)]
        app.handle_terminal_events(Ok(crossterm::event::Event::Key(key.into())))
            .await;
        #[cfg(not(windows))]
        app.handle_terminal_events(Ok(termina::event::Event::Key(key.into())))
            .await;
    }
    keys(&mut app, "<esc>:set-option minimap.enabled false<ret>").await?;
    let state = &current_ref!(app.editor).1.minimap;
    assert!(!state.pending && !state.controller.is_running());
    assert!(state.summary.is_none() && state.projections.is_empty());
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_existing_diff_diagnostics_and_marker_switches() -> anyhow::Result<()> {
    use helix_core::{
        diagnostic::{DiagnosticProvider, Severity},
        Diagnostic,
    };
    use helix_view::minimap::LineMap;
    let text = source(300);
    let (mut app, _file) = app_with(&text).await?;
    let (view, doc) = helix_view::current!(app.editor);
    let version = doc.version();
    let start = doc.text().line_to_char(100);
    let end = doc.text().line_to_char(150);
    doc.replace_diagnostics(
        [Diagnostic {
            range: helix_core::diagnostic::Range { start, end },
            starts_at_word: false,
            ends_at_word: false,
            zero_width: false,
            line: 100,
            message: "minimap diagnostic".into(),
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
    let base = text.replace("value_100 =", "before_100 =") + "# deleted at EOF\n";
    doc.set_diff_base(base.into_bytes());
    let map = LineMap::new(
        doc.text().len_lines(),
        view.inner_height() as u16,
        doc.config.load().minimap.render,
    );
    app.test_resize(120, 32).await?;
    keys(&mut app, "").await?;
    let r = area(&app);
    let screen = app.test_screen();
    assert_eq!(
        screen[(r.right() - 1, r.y + map.row(100))].symbol.as_str(),
        "┃"
    );
    assert_eq!(
        screen[(r.right() - 2, r.y + map.row(100))].symbol.as_str(),
        "▍"
    );
    assert_eq!(
        screen[(r.right() - 2, r.y + map.row(300))].symbol.as_str(),
        "▔"
    );
    // Apply each queued config update before serializing the next one.
    keys(&mut app, ":set-option minimap.diagnostics false<ret>").await?;
    keys(&mut app, ":set-option minimap.git-diff false<ret>").await?;
    assert!(!app.editor.config().minimap.diagnostics);
    assert!(!app.editor.config().minimap.git_diff);
    let r = area(&app);
    let screen = app.test_screen();
    for y in r.y..r.bottom() {
        assert_eq!(screen[(r.right() - 1, y)].symbol.as_str(), " ");
        assert_eq!(screen[(r.right() - 2, y)].symbol.as_str(), " ");
    }
    let doc = current_ref!(app.editor).1;
    assert_eq!(doc.version(), version);
    assert!(!doc.is_modified());
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimap_resize_and_buffer_switch_cancel_capture_including_empty_document(
) -> anyhow::Result<()> {
    let (mut app, _file) = app_with(&source(1000)).await?;
    app.editor.registers.write('m', vec!["untouched".into()])?;
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.y + r.height / 2, r.x + 4).await?;
    app.test_resize(110, 32).await?;
    mouse(&mut app, Mouse::Drag, 1, 10).await?;
    mouse(&mut app, Mouse::Up, 1, 10).await?;
    assert_eq!(
        app.editor
            .registers
            .read('m', &app.editor)
            .unwrap()
            .next()
            .unwrap(),
        "untouched"
    );
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.y, r.x + 4).await?;
    let original = current_ref!(app.editor).1.id();
    keys(&mut app, ":new<ret>").await?;
    mouse(&mut app, Mouse::Up, 1, 10).await?;
    assert_eq!(
        app.editor
            .registers
            .read('m', &app.editor)
            .unwrap()
            .next()
            .unwrap(),
        "untouched"
    );
    assert!(app
        .editor
        .document(original)
        .unwrap()
        .minimap
        .summary
        .is_none());
    let doc = current_ref!(app.editor).1;
    assert_eq!(
        doc.text().to_string(),
        helix_core::NATIVE_LINE_ENDING.as_str()
    );
    assert_eq!(
        doc.minimap.summary.as_ref().unwrap().lines,
        doc.text().len_lines()
    );
    let r = area(&app);
    mouse(&mut app, Mouse::Down, r.bottom() - 1, r.x + 3).await?;
    mouse(&mut app, Mouse::Up, r.bottom() - 1, r.x + 3).await?;
    assert!(!current_ref!(app.editor).1.is_modified());
    keys(&mut app, ":quit-all!<ret>").await?;
    app.close().await;
    Ok(())
}
