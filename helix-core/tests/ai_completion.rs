use helix_core::{
    char_idx_at_visual_offset,
    doc_formatter::{DocumentFormatter, TextFormat},
    text_annotations::{InlineAnnotation, TextAnnotations},
    visual_offset_from_anchor, visual_offset_from_block, Position, Rope,
};

#[test]
fn ghost_multiline_text_keeps_caret_before_preview() {
    let text = Rope::from("abZ\n");
    let suggestion = InlineAnnotation::new(2, "值\nx");
    let mut annotations = TextAnnotations::default();
    annotations.add_inline_completion(&suggestion, None);
    let format = TextFormat::default();
    assert_eq!(
        visual_offset_from_block(text.slice(..), 0, 2, &format, &annotations).0,
        Position::new(0, 2)
    );
    assert_eq!(
        visual_offset_from_anchor(text.slice(..), 2, 2, &format, &annotations, 10)
            .unwrap()
            .0,
        Position::new(0, 2)
    );
    assert_eq!(
        char_idx_at_visual_offset(text.slice(..), 0, 1, 1, &format, &annotations).0,
        2
    );
    let rendered: String =
        DocumentFormatter::new_at_prev_checkpoint(text.slice(..), &format, &annotations, 0)
            .map(|g| g.raw.to_string())
            .collect();
    assert!(rendered.contains("值"));
    assert_eq!(text.to_string(), "abZ\n");
}

#[test]
fn ghost_caret_at_end_of_file_and_softwrap() {
    let text = Rope::from("abc");
    let suggestion = InlineAnnotation::new(3, " 世界\nnext");
    let mut annotations = TextAnnotations::default();
    annotations.add_inline_completion(&suggestion, None);
    for soft_wrap in [false, true] {
        let format = TextFormat {
            soft_wrap,
            viewport_width: 8,
            ..TextFormat::default()
        };
        assert_eq!(
            visual_offset_from_block(text.slice(..), 0, 3, &format, &annotations).0,
            Position::new(0, 3)
        );
    }
}
