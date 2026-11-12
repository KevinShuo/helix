use super::*;
#[test]
fn merge_layout_wide_narrow_tiny() {
    for area in [
        Rect::new(0, 0, 160, 40),
        Rect::new(0, 0, 120, 18),
        Rect::new(0, 0, 80, 24),
        Rect::new(0, 0, 40, 7),
        Rect::new(13, 7, 160, 40),
    ] {
        let layout = MergeLayout::for_area(area);
        assert_eq!(layout.wide, area.width >= 120 && area.height >= 18);
        for rect in [
            layout.current,
            layout.result,
            layout.incoming,
            layout.header,
            layout.footer,
        ] {
            assert!(
                rect.x >= area.x
                    && rect.y >= area.y
                    && rect.right() <= area.right()
                    && rect.bottom() <= area.bottom()
            );
        }
        assert!(layout.result.y >= layout.header.bottom());
        assert!(layout.result.bottom() <= layout.footer.y);
        if layout.wide {
            assert!(layout.current.right() <= layout.result.x);
            assert!(layout.result.right() <= layout.incoming.x);
        }
    }
}

#[test]
fn merge_apply_disabled_until_reviewed() {
    use helix_core::{Rope, Transaction};
    use helix_vcs::merge::{MergeModel, MergeSources};
    let text = Rope::from("manual\n");
    let mut model = MergeModel::preserve_working(
        text.clone(),
        MergeSources::new(text.clone(), text.clone(), text.clone()),
    );
    assert!(!model.ready_to_apply());
    model.mark_reviewed(None);
    assert!(model.ready_to_apply());
    let tx = Transaction::change(&text, [(0, 0, Some("new".into()))].into_iter());
    model.on_change(tx.changes());
    assert!(!model.ready_to_apply());
}

#[test]
fn merge_source_panes_are_read_only() {
    let mut preview = Preview::Current;
    let mut scroll = [0; 3];
    for code in [
        KeyCode::Char('i'),
        KeyCode::Char('d'),
        KeyCode::Char('x'),
        KeyCode::Char('q'),
    ] {
        source_key(&mut preview, &mut scroll, code);
    }
    assert_eq!(preview, Preview::Current);
    assert_eq!(scroll, [0; 3]);
    source_key(&mut preview, &mut scroll, KeyCode::Down);
    assert_eq!(scroll, [0, 1, 0]);
    source_key(&mut preview, &mut scroll, KeyCode::Char('3'));
    source_key(&mut preview, &mut scroll, KeyCode::Down);
    assert_eq!(scroll, [0, 1, 1]);
    source_key(&mut preview, &mut scroll, KeyCode::Esc);
    assert_eq!(preview, Preview::Result);
}

#[test]
fn merge_previews_clip_unicode_and_remove_terminal_controls() {
    let area = Rect::new(3, 2, 8, 2);
    let mut buffer = tui::buffer::Buffer::empty(area);
    render::line(
        &mut buffer,
        area,
        0,
        "中\t文\x1b[31mtoo long",
        Default::default(),
    );
    assert!(buffer
        .content
        .iter()
        .all(|cell| !cell.symbol.chars().any(char::is_control)));
    assert_eq!(buffer.get(3, 2).unwrap().symbol.as_str(), "中");
    assert_eq!(buffer.content.len(), 16);
}
