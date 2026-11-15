//! Docked results. Execution and history belong to the editor, not the component.
use crate::{
    compositor::{Component, Context, Event, EventResult},
    handlers::test,
};
use helix_core::{Position, Selection};
use helix_view::{
    editor::Action,
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    test::{Outcome, TestState},
    Editor,
};
use std::collections::{BTreeMap, BTreeSet};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

pub(crate) const ID: &str = "test-workbench";
const ACTIONS: &[(char, &str)] = &[
    ('r', "Run"),
    ('f', "Failed"),
    ('R', "Rerun"),
    ('s', "Stop"),
    ('q', "Close"),
];
struct Row {
    key: Option<String>,
    label: String,
    depth: usize,
    cases: Vec<usize>,
}
pub(crate) struct TestPanel {
    focused: bool,
    details: bool,
    selected: usize,
    scroll: usize,
    detail_scroll: usize,
    collapsed: BTreeSet<String>,
    area: Rect,
    tree: Rect,
    detail: Rect,
    run_id: u64,
}
impl TestPanel {
    pub(crate) fn focus(&mut self) {
        self.focused = true;
    }
    pub(crate) fn new() -> Self {
        Self {
            focused: true,
            details: false,
            selected: 0,
            scroll: 0,
            detail_scroll: 0,
            collapsed: BTreeSet::new(),
            area: Rect::default(),
            tree: Rect::default(),
            detail: Rect::default(),
            run_id: 0,
        }
    }
    fn layout(area: Rect) -> Rect {
        let height = (area.height / 2).max(6).min(area.height.saturating_sub(4));
        Rect::new(
            area.x,
            area.bottom().saturating_sub(1 + height),
            area.width,
            height,
        )
    }
    fn rows(&self, state: &TestState) -> Vec<Row> {
        let mut files: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
        for (index, case) in state.cases.iter().enumerate() {
            let file = if case.path.as_os_str().is_empty() {
                "CTest".into()
            } else {
                state
                    .request
                    .as_ref()
                    .and_then(|r| case.path.strip_prefix(&r.root).ok())
                    .unwrap_or(&case.path)
                    .display()
                    .to_string()
            };
            let id = case.id.split('[').next().unwrap_or(&case.id);
            let parts: Vec<_> = id.split("::").collect();
            let suite = if parts.len() > 2 {
                parts[1].to_owned()
            } else if !id.contains("::") {
                id.split('.').next().unwrap_or("").to_owned()
            } else {
                String::new()
            };
            files
                .entry(file)
                .or_default()
                .entry(suite)
                .or_default()
                .push(index);
        }
        let mut rows = Vec::new();
        for (file, suites) in files {
            let key = format!("file:{file}");
            rows.push(Row {
                key: Some(key.clone()),
                label: file.clone(),
                depth: 0,
                cases: suites.values().flatten().copied().collect(),
            });
            if self.collapsed.contains(&key) {
                continue;
            }
            for (suite, cases) in suites {
                if !suite.is_empty() {
                    let key = format!("suite:{file}:{suite}");
                    rows.push(Row {
                        key: Some(key.clone()),
                        label: suite,
                        depth: 1,
                        cases: cases.clone(),
                    });
                    if self.collapsed.contains(&key) {
                        continue;
                    }
                }
                for index in cases {
                    rows.push(Row {
                        key: None,
                        label: state.cases[index].name.clone(),
                        depth: 2,
                        cases: vec![index],
                    });
                }
            }
        }
        rows
    }
    fn act(&mut self, action: char, cx: &mut Context) -> EventResult {
        if action == 'q' {
            cx.editor.tests.panel_visible = false;
            return EventResult::Consumed(Some(Box::new(|c, _| {
                c.remove(ID);
            })));
        }
        let result = match action {
            's' => {
                test::stop(cx.editor);
                Ok(())
            }
            'f' => test::rerun(cx.editor, true, None),
            'R' => test::rerun(cx.editor, false, None),
            'r' => {
                let rows = self.rows(&cx.editor.tests);
                let ids = rows
                    .get(self.selected)
                    .map(|r| {
                        r.cases
                            .iter()
                            .map(|i| cx.editor.tests.cases[*i].id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                test::rerun(cx.editor, false, Some(ids))
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            cx.editor.set_error(error.to_string());
        }
        EventResult::Consumed(None)
    }
    fn open(&mut self, cx: &mut Context) {
        let rows = self.rows(&cx.editor.tests);
        let Some(row) = rows.get(self.selected) else {
            return;
        };
        if let Some(key) = &row.key {
            if !self.collapsed.remove(key) {
                self.collapsed.insert(key.clone());
            }
            return;
        }
        let case = &cx.editor.tests.cases[row.cases[0]];
        let path = case.failure_path.as_ref().unwrap_or(&case.path).clone();
        let line = case.failure_line.unwrap_or(case.line);
        if path.as_os_str().is_empty() {
            self.details = true;
            return;
        }
        match cx.editor.open(&path, Action::Replace) {
            Ok(_) => {
                let (view, doc) = helix_view::current!(cx.editor);
                let id = view.id;
                let line = line.min(doc.text().len_lines().saturating_sub(1));
                doc.set_selection(id, Selection::point(doc.text().line_to_char(line)));
                cx.editor.ensure_cursor_in_view(id);
                self.focused = false;
            }
            Err(error) => cx.editor.set_error(format!("无法打开测试位置：{error}")),
        }
    }
}
fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}
fn inside(rect: Rect, row: u16, col: u16) -> bool {
    row >= rect.y && row < rect.bottom() && col >= rect.x && col < rect.right()
}
impl Component for TestPanel {
    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
    fn takes_keyboard_input(&self) -> bool {
        self.focused
    }
    fn editor_area_for(&self, area: Rect, editor: &Editor) -> Rect {
        if editor.active_merge().is_some() {
            area
        } else {
            area.clip_bottom(Self::layout(area).height)
        }
    }
    fn render(&mut self, area: Rect, frame: &mut Buffer, cx: &mut Context) {
        if cx.editor.active_merge().is_some() {
            return;
        }
        if self.run_id != cx.editor.tests.id {
            self.run_id = cx.editor.tests.id;
            self.selected = 0;
            self.scroll = 0;
            self.detail_scroll = 0;
            self.collapsed.clear();
        }
        self.area = Self::layout(area);
        let panel = self.area;
        frame.clear_with(panel, cx.editor.theme.get("ui.window"));
        let state = &cx.editor.tests;
        let count = |outcome| state.cases.iter().filter(|c| c.outcome == outcome).count();
        let title = format!(
            " Tests · {} · ✓{} ✗{} !{} −{}{} ",
            safe(&state.phase),
            count(Outcome::Passed),
            count(Outcome::Failed),
            count(Outcome::Error),
            count(Outcome::Skipped),
            if state.stale {
                " · 结果已过期"
            } else {
                ""
            }
        );
        Block::default()
            .borders(Borders::TOP)
            .title(title)
            .render(panel, frame);
        let mut column = panel.x + 1;
        for (key, label) in ACTIONS {
            let text = format!("[{key} {label}] ");
            frame.set_stringn(
                column,
                panel.y + 1,
                &text,
                panel.right().saturating_sub(column) as usize,
                cx.editor.theme.get("ui.text.focus"),
            );
            column = column.saturating_add(text.len() as u16);
        }
        let body = panel.clip_top(2).clip_bottom(1);
        if area.width >= 75 {
            self.tree = Rect::new(body.x, body.y, body.width * 2 / 5, body.height);
            self.detail = body.clip_left(self.tree.width + 1);
        } else {
            self.tree = if self.details { Rect::default() } else { body };
            self.detail = if self.details { body } else { Rect::default() };
        }
        let rows = self.rows(state);
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected >= self.scroll + self.tree.height as usize {
            self.scroll = (self.selected + 1).saturating_sub(self.tree.height as usize);
        }
        for (offset, row) in rows
            .iter()
            .skip(self.scroll)
            .take(self.tree.height as usize)
            .enumerate()
        {
            let status = TestState::aggregate(row.cases.iter().map(|i| &state.cases[*i]));
            let branch = row
                .key
                .as_ref()
                .map(|k| {
                    if self.collapsed.contains(k) {
                        "▸ "
                    } else {
                        "▾ "
                    }
                })
                .unwrap_or("  ");
            let elapsed: f64 = row.cases.iter().map(|i| state.cases[*i].duration).sum();
            let text = format!(
                "{}{}{} {}  {:.2}s",
                " ".repeat(row.depth * 2),
                branch,
                if state.stale { "?" } else { status.symbol() },
                safe(&row.label),
                elapsed
            );
            let style = if offset + self.scroll == self.selected {
                cx.editor.theme.get("ui.selection")
            } else {
                cx.editor.theme.get(if status.failed() {
                    "error"
                } else if status == Outcome::Passed {
                    "diagnostic.info"
                } else {
                    "ui.text"
                })
            };
            frame.set_stringn(
                self.tree.x,
                self.tree.y + offset as u16,
                &text,
                self.tree.width as usize,
                style,
            );
        }
        let details = if let Some(row) = rows.get(self.selected).filter(|r| r.key.is_none()) {
            let c = &state.cases[row.cases[0]];
            format!(
                "{} · {:?} · {:.3}s\n{}:{}\n\n{}",
                c.name,
                c.outcome,
                c.duration,
                c.path.display(),
                c.line + 1,
                c.details
            )
        } else {
            format!("{}\n\n{}", state.phase, state.log)
        };
        if self.detail.width > 0 {
            Paragraph::new(&tui::text::Text::raw(safe(&details)))
                .wrap(Wrap { trim: false })
                .scroll((self.detail_scroll.min(u16::MAX as usize) as u16, 0))
                .render(self.detail, frame);
        }
        if panel.height > 2 {
            frame.set_stringn(
                panel.x,
                panel.bottom() - 1,
                " Enter: source/expand · Tab: details · Esc: edit · :test-show: focus ",
                panel.width as usize,
                cx.editor.theme.get("ui.text.inactive"),
            );
        }
        if !self.focused {
            if let Some(mut info) = cx.editor.autoinfo.take() {
                info.render(area, frame, cx);
                cx.editor.autoinfo = Some(info);
            }
        }
    }
    fn cursor(&self, _area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        (
            (self.focused && editor.active_merge().is_none())
                .then(|| Position::new(self.area.y as usize, self.area.x as usize)),
            CursorKind::Hidden,
        )
    }
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if cx.editor.active_merge().is_some() {
            return EventResult::Ignored(None);
        }
        if cx.editor.tests.focus_requested {
            self.focused = true;
            cx.editor.tests.focus_requested = false;
        }
        if let Event::Mouse(mouse) = event {
            if !inside(self.area, mouse.row, mouse.column) {
                if matches!(mouse.kind, MouseEventKind::Down(_)) {
                    self.focused = false;
                }
                return EventResult::Ignored(None);
            }
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.focused = true;
                if mouse.row == self.area.y + 1 {
                    let mut col = self.area.x + 1;
                    for (key, label) in ACTIONS {
                        let end = col + label.len() as u16 + 5;
                        if mouse.column >= col && mouse.column < end {
                            return self.act(*key, cx);
                        }
                        col = end;
                    }
                } else if inside(self.tree, mouse.row, mouse.column) {
                    self.selected = self.scroll + (mouse.row - self.tree.y) as usize;
                    self.details = false;
                    self.detail_scroll = 0;
                } else {
                    self.details = true;
                }
            } else if matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            ) {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                if inside(self.detail, mouse.row, mouse.column) {
                    self.detail_scroll = if down {
                        self.detail_scroll.saturating_add(3)
                    } else {
                        self.detail_scroll.saturating_sub(3)
                    };
                } else {
                    self.selected = if down {
                        self.selected.saturating_add(3)
                    } else {
                        self.selected.saturating_sub(3)
                    };
                }
            }
            return EventResult::Consumed(None);
        }
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        if !self.focused {
            return EventResult::Ignored(None);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('w') {
            self.focused = false;
            return EventResult::Ignored(None);
        }
        if key.code == KeyCode::Esc {
            self.focused = false;
            return EventResult::Consumed(None);
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return EventResult::Consumed(None);
        }
        match key.code {
            KeyCode::Char(c) if ACTIONS.iter().any(|(key, _)| *key == c) => return self.act(c, cx),
            KeyCode::Tab => self.details = !self.details,
            KeyCode::Enter => self.open(cx),
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(key) = self
                    .rows(&cx.editor.tests)
                    .get(self.selected)
                    .and_then(|r| r.key.as_ref())
                {
                    self.collapsed.remove(key);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if let Some(key) = self
                    .rows(&cx.editor.tests)
                    .get(self.selected)
                    .and_then(|r| r.key.as_ref())
                {
                    self.collapsed.insert(key.clone());
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::PageDown => {
                let n = if key.code == KeyCode::PageDown { 10 } else { 1 };
                if self.details {
                    self.detail_scroll = self.detail_scroll.saturating_add(n);
                } else {
                    self.selected = self.selected.saturating_add(n);
                    self.detail_scroll = 0;
                }
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::PageUp => {
                let n = if key.code == KeyCode::PageUp { 10 } else { 1 };
                if self.details {
                    self.detail_scroll = self.detail_scroll.saturating_sub(n);
                } else {
                    self.selected = self.selected.saturating_sub(n);
                    self.detail_scroll = 0;
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }
}
