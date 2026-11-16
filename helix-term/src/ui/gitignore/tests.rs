use super::*;

#[test]
fn selection_keeps_hidden_items_and_does_not_reselect_recommendations() {
    let mut selected = TemplateSelection::new(&["rust"]);
    selected.filter("macOS");
    selected.toggle();
    assert_eq!(
        selected.selected,
        std::collections::BTreeSet::from(["rust", "macos"])
    );
    selected.filter("rust");
    selected.toggle();
    selected.filter("");
    assert!(!selected.selected.contains("rust"));
    assert_eq!(selected.current().unwrap().id, "rust");
    selected.filter("no-match");
    selected.toggle();
    selected.move_by(false);
    assert!(selected.current().is_none());
    assert_eq!(
        selected.selected,
        std::collections::BTreeSet::from(["macos"])
    );
    selected.filter("js");
    assert_eq!(selected.current().unwrap().id, "node");
    selected.filter("os win");
    assert_eq!(selected.current().unwrap().id, "windows");
}
