use super::*;
use helix_vcs::git_tools::GitCommit;
use helix_view::graphics::Rect;

#[test]
fn git_panel_docks_and_fullscreen_preserves_state() {
    let area = Rect::new(4, 3, 140, 44);
    let mut panel = GitPanel::new(PathBuf::from("."), OpenAction::Log);
    let dock = panel.layout(area);
    assert_eq!(dock.panel.bottom(), area.bottom() - 1);
    assert!(dock.panel.y > area.y + 5);
    assert_eq!(
        panel.editor_area(area).height + dock.panel.height,
        area.height
    );
    panel.selected_commit = 12;
    panel.diff_scroll = 7;
    panel.fullscreen = true;
    let full = panel.layout(area);
    assert_eq!(full.panel, area.clip_bottom(1));
    assert_eq!(panel.selected_commit, 12);
    assert_eq!(panel.diff_scroll, 7);
    panel.fullscreen = false;
    assert_eq!(panel.layout(area).panel, dock.panel);
}

#[test]
fn git_panel_tiny_layouts_stay_inside_screen() {
    for width in [0, 1, 20, 60, 90, 140] {
        for height in [0, 1, 3, 8, 24, 50] {
            let area = Rect::new(7, 5, width, height);
            let mut panel = GitPanel::new(PathBuf::new(), OpenAction::Log);
            for focus in [Pane::Branches, Pane::Commits, Pane::Files, Pane::Diff] {
                panel.focus = focus;
                for fullscreen in [false, true] {
                    panel.fullscreen = fullscreen;
                    let l = panel.layout(area);
                    for rect in [l.panel, l.branches, l.commits, l.files, l.diff] {
                        assert!(rect.x >= area.x && rect.y >= area.y, "{area:?}: {rect:?}");
                        assert!(
                            rect.right() <= area.right() && rect.bottom() <= area.bottom(),
                            "{area:?}: {rect:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn git_panel_stale_snapshots_and_selection() {
    let mut panel = GitPanel::new(PathBuf::new(), OpenAction::Log);
    panel.load_version = 3;
    let mut snapshot = GitSnapshot::default();
    snapshot.head_name = "main".into();
    assert!(!panel.apply_snapshot(2, snapshot.clone()));
    assert_eq!(panel.snapshot.head_name, "");
    assert!(panel.apply_snapshot(3, snapshot));
    assert_eq!(panel.snapshot.head_name, "main");
}

#[test]
fn git_panel_branch_filter_does_not_checkout() {
    let mut panel = GitPanel::new(PathBuf::new(), OpenAction::Log);
    panel.snapshot.head_name = "main".into();
    panel.snapshot.references.push(GitReference {
        full_name: "refs/heads/topic".into(),
        name: "topic".into(),
        id: "a".repeat(40),
        kind: GitRefKind::Local,
        current: false,
    });
    let row = panel
        .branch_rows()
        .iter()
        .position(|r| r.reference == Some(0))
        .unwrap();
    panel.selected_branch = row;
    panel.filter_selected_branch();
    assert_eq!(panel.filter.revision.as_deref(), Some("refs/heads/topic"));
    assert_eq!(panel.snapshot.head_name, "main");
}

#[test]
fn git_panel_graph_connectors_belong_to_previous_commit() {
    let mut panel = GitPanel::new(PathBuf::new(), OpenAction::Log);
    for (id, after) in [("a", vec!["|\\ ".to_owned()]), ("b", Vec::new())] {
        panel.snapshot.commits.push(GitCommit {
            id: id.repeat(40),
            parents: vec![],
            author: "Author".into(),
            date: "2026-11-11".into(),
            subject: id.into(),
            decoration: String::new(),
            graph: "* ".into(),
            graph_after: after,
        });
    }
    assert_eq!(panel.commit_rows(), vec![(0, false), (0, true), (1, false)]);
}

#[test]
fn git_panel_render_handles_control_characters_and_unicode() {
    assert_eq!(
        render::safe_text("中文\tname\u{1b}[31m\r"),
        "中文 name [31m "
    );
}

#[test]
fn git_panel_branch_search_keeps_group_structure() {
    let mut panel = GitPanel::new(PathBuf::new(), OpenAction::Branches);
    for name in ["main", "feature/topic"] {
        panel.snapshot.references.push(GitReference {
            full_name: format!("refs/heads/{name}"),
            name: name.into(),
            id: "a".repeat(40),
            kind: GitRefKind::Local,
            current: name == "main",
        });
    }
    panel.branch_query = "topic".into();
    let rows = panel.branch_rows();
    assert!(rows.iter().any(|r| r.label.contains("Local")));
    assert!(rows.iter().any(|r| r.label.contains("feature/topic")));
    assert!(!rows.iter().any(|r| r.reference == Some(0)));
}
