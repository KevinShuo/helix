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
