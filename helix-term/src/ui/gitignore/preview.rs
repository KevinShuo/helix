use std::{
    fmt::Write,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{ensure, Context as _, Result};
use helix_core::{Position, Selection};
use helix_event::TaskController;
use helix_vcs::ignore_completion::preview::{
    scan_all, Origin, Report, CHANGED_SAMPLES, UNCHANGED_SAMPLES,
};
use helix_view::{
    align_view,
    editor::Action,
    graphics::{CursorKind, Rect},
    input::KeyEvent,
    Align, DocumentId, Editor, ViewId,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Widget},
};

use super::{display_text, existing_buffer};
use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl, job, key,
    ui::{
        ai_preview_text::{scroll_key, PreviewText},
        Prompt,
    },
};

const ID: &str = "gitignore-rule-preview";

struct Target {
    document: DocumentId,
    view: ViewId,
    version: i32,
    cursor: usize,
    path: PathBuf,
}

impl Target {
    fn current(&self, editor: &Editor) -> bool {
        let (view, doc) = helix_view::current_ref!(editor);
        self.view == view.id
            && self.document == doc.id()
            && self.version == doc.version()
            && doc.path() == Some(self.path.as_path())
            && self.cursor
                == doc
                    .selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..))
    }
}

pub(crate) fn open(cx: &mut Context) -> Result<()> {
    let (view, doc) = helix_view::current_ref!(cx.editor);
    let path = doc
        .path()
        .filter(|p| p.file_name().is_some_and(|n| n == ".gitignore"))
        .context("Open a .gitignore file to preview a rule")?;
    let base = path
        .parent()
        .context("Missing .gitignore directory")?
        .to_owned();
    let text = doc.text().clone();
    let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
    let byte = text.char_to_byte(cursor);
    let target = Target {
        document: doc.id(),
        view: view.id,
        version: doc.version(),
        cursor,
        path: path.to_owned(),
    };
    let trust = cx.editor.workspace_trust.clone();
    let mut task = TaskController::new();
    let handle = task.restart();
    let path = target.path.clone();
    cx.jobs
        .after_event
        .push(Box::new(move |editor, compositor| {
            if !target.current(editor) {
                return;
            }
            compositor.replace_or_push(
                ID,
                Preview {
                    target,
                    _task: task,
                    results: None,
                    viewport: Rect::default(),
                    filter: None,
                    changed_only: false,
                    filter_area: Rect::default(),
                },
            );
        }));
    tokio::spawn(async move {
        let worker = handle.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<Results> {
            let text = text.to_string();
            // Git uses LF-delimited patterns, including when the editor recognizes
            // other Unicode line separators. Work from the captured byte offset.
            let line = text[..byte].bytes().filter(|&b| b == b'\n').count();
            let rule = text.lines().nth(line).unwrap_or_default();
            ensure!(
                !matches!(rule.trim_end_matches(' '), "" | "!") && !rule.starts_with('#'),
                "Place the cursor on a .gitignore rule"
            );
            let workspace = helix_loader::find_workspace_in(&base).0;
            let trusted = trust
                .query(&workspace, helix_loader::workspace_trust::TrustQuery::Git)
                .is_trusted();
            let report = scan_all(&base, &text, line, trusted, || worker.is_canceled())?;
            Ok(format_report(&path, line, rule, report))
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|r| r);
        job::dispatch(move |editor, compositor| {
            if handle.is_canceled() {
                return;
            }
            let Some(preview) = compositor.find_id::<Preview>(ID) else {
                return;
            };
            if !preview.target.current(editor) {
                compositor.remove(ID);
                return;
            }
            match result {
                Ok(results) => {
                    preview.results = Some(results);
                    if preview.changed_only || preview.filter.is_some() {
                        preview.apply_filters();
                    }
                }
                Err(error) => {
                    compositor.remove(ID);
                    editor.set_error(format!("{error:#}"));
                }
            }
        })
        .await;
    });
    Ok(())
}

fn display_path(path: &Path) -> String {
    let Some(path_text) = path.to_str() else {
        return format!("{:?}", path.as_os_str());
    };
    #[cfg(windows)]
    let path_text = path_text.replace('\\', "/");
    // Quote unusual names so that a newline or tab cannot masquerade as
    // another row or as a filename containing spaces.
    if path_text
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
    {
        format!("{path_text:?}")
    } else {
        path_text.into()
    }
}

fn display_rule(rule: &str) -> String {
    // One very long glob may win for every example in the panel.
    match rule.char_indices().nth(1024) {
        Some((end, _)) => format!("{}…", display_text(&rule[..end])),
        None => display_text(rule),
    }
}

struct FileRow {
    bytes: Range<usize>,
    match_index: usize,
}

struct FileMatch {
    path: String,
    search_path: String,
    state: &'static str,
    winner: Option<Arc<Origin>>,
}

struct Results {
    heading: String,
    matches: Vec<FileMatch>,
    // The scan orders changed files before those whose final status is unchanged.
    changed_end: usize,
    text: PreviewText,
    files: Vec<FileRow>,
    root: PathBuf,
    selected: usize,
    scroll: usize,
}

fn format_report(path: &Path, line: usize, rule: &str, report: Report) -> Results {
    let origin = path.strip_prefix(&report.root).unwrap_or(path);
    let mut heading = format!(
        "Rule {}:{}  {}\n{} changed, {} unchanged\n",
        display_path(origin),
        line + 1,
        display_rule(rule),
        report.changed,
        report.unchanged
    );
    if report.partial {
        heading.push_str("Partial scan. Counts cover the paths examined.\n");
    }
    let matches = report
        .entries
        .into_iter()
        .map(|entry| {
            let state = match (entry.before_ignored, entry.after_ignored) {
                (false, true) => "Now ignored",
                (true, false) => "Now included",
                (true, true) => "Still ignored",
                (false, false) => "Still included",
            };
            let path = display_path(&entry.path);
            FileMatch {
                search_path: path.to_lowercase(),
                path,
                state,
                winner: entry.winner,
            }
        })
        .collect();
    let mut results = Results {
        heading,
        matches,
        changed_end: report.changed,
        text: PreviewText::new(String::new()),
        files: Vec::new(),
        root: report.root,
        selected: 0,
        scroll: 0,
    };
    results.filter("", false);
    results
}

struct Preview {
    target: Target,
    // Dropping the panel cancels its walk and invalidates queued callbacks.
    _task: TaskController,
    results: Option<Results>,
    viewport: Rect,
    filter: Option<Prompt>,
    changed_only: bool,
    filter_area: Rect,
}

impl Preview {
    fn apply_filters(&mut self) {
        if let Some(results) = &mut self.results {
            let query = self
                .filter
                .as_ref()
                .map_or("", |prompt| prompt.line().as_str());
            results.filter(query, self.changed_only);
            results.reveal(self.viewport);
        }
    }
}

impl Results {
    fn filter(&mut self, query: &str, changed_only: bool) {
        let query = query.to_lowercase();
        #[cfg(windows)]
        let query = query.replace('\\', "/");
        let previous = self.files.get(self.selected).map(|file| file.match_index);
        let mut visible = Vec::new();
        let mut changed = 0;
        let mut unchanged = 0;
        let candidates = if changed_only {
            &self.matches[..self.changed_end]
        } else {
            &self.matches[..]
        };
        for (index, file) in candidates.iter().enumerate() {
            if !file.search_path.contains(&query) {
                continue;
            }
            let (count, limit) = if index < self.changed_end {
                (&mut changed, CHANGED_SAMPLES)
            } else {
                (&mut unchanged, UNCHANGED_SAMPLES)
            };
            *count += 1;
            if *count <= limit {
                visible.push(index);
            }
        }
        let total = changed + unchanged;
        let mut text = self.heading.clone();
        if !query.is_empty() && total > 0 {
            writeln!(
                text,
                "Path filter matches {total} of {} {}.",
                candidates.len(),
                if changed_only {
                    "changed files"
                } else {
                    "files"
                }
            )
            .unwrap();
        }
        if visible.len() < total {
            writeln!(text, "Showing {} of {total} matching files.", visible.len()).unwrap();
        }
        if total == 0 {
            text.push_str(match (changed_only, query.is_empty()) {
                (true, true) => "\nNo files changed ignore status.",
                (true, false) => "\nNo changed files match the path filter.",
                (false, true) => "\nNo matching untracked files found.",
                (false, false) => "\nNo files match the path filter.",
            });
        }
        self.files.clear();
        self.selected = 0;
        for index in visible {
            let file = &self.matches[index];
            if Some(index) == previous {
                self.selected = self.files.len();
            }
            text.push('\n');
            let offset = text.len();
            writeln!(text, "{}  {}", file.state, file.path).unwrap();
            if let Some(winner) = &file.winner {
                writeln!(
                    text,
                    "  {}:{}  {}",
                    display_path(&winner.source),
                    winner.line,
                    display_rule(&winner.pattern)
                )
                .unwrap();
            } else {
                text.push_str("  Included by default\n");
            }
            self.files.push(FileRow {
                bytes: offset..text.len(),
                match_index: index,
            });
        }
        self.text = PreviewText::new(text);
        self.scroll = 0;
    }

    fn selected_rows(&mut self, width: u16) -> Option<Range<usize>> {
        let bytes = &self.files.get(self.selected)?.bytes;
        Some(self.text.row_at_byte(width, bytes.start)..self.text.row_at_byte(width, bytes.end))
    }

    fn reveal(&mut self, area: Rect) {
        let height = usize::from(area.height);
        if let Some(rows) = self.selected_rows(area.width) {
            if self.selected == 0 && rows.end <= height {
                self.scroll = 0;
            } else {
                // Keep the whole item in view if it fits, otherwise show its first row.
                self.scroll = self
                    .scroll
                    .max(rows.end.saturating_sub(height))
                    .min(rows.start);
            }
        }
    }

    fn navigate(&mut self, key: KeyEvent, area: Rect) {
        let Some(last) = self.files.len().checked_sub(1) else {
            scroll_key(key, &mut self.scroll);
            return;
        };
        self.selected = match key {
            key!(Down) | key!('j') => (self.selected + 1).min(last),
            key!(Up) | key!('k') => self.selected.saturating_sub(1),
            key!(Home) | ctrl!(Home) => 0,
            key!(End) | ctrl!(End) => last,
            key!(PageDown) | ctrl!('d') | key!(PageUp) | ctrl!('u') => {
                let height = usize::from(area.height.max(1));
                let down = matches!(key, key!(PageDown) | ctrl!('d'));
                let bottom = self
                    .text
                    .row_at_byte(area.width, usize::MAX)
                    .saturating_sub(height - 1);
                self.scroll = if down {
                    self.scroll.saturating_add(height).min(bottom)
                } else {
                    self.scroll.saturating_sub(height)
                };
                self.selected = if self.scroll == bottom && down {
                    last
                } else if self.scroll == 0 {
                    0
                } else {
                    self.files
                        .partition_point(|file| {
                            self.text.row_at_byte(area.width, file.bytes.end) <= self.scroll
                        })
                        .min(last)
                };
                return;
            }
            _ => return,
        };
        self.reveal(area);
    }
}

fn jump_to_rule(
    editor: &mut Editor,
    root: &Path,
    origin: &Origin,
    source: DocumentId,
) -> Result<()> {
    let path = root.join(&origin.source);
    let id = if let Some(id) = existing_buffer(editor, &path) {
        id
    } else {
        ensure!(
            path.is_file(),
            "Rule source is unavailable: {}",
            display_path(&path)
        );
        editor.open(&path, Action::Load)?
    };
    let doc = editor.document(id).context("Rule source is unavailable")?;
    // Git counts LF-delimited lines even when Helix recognizes other line endings.
    let text = doc.text();
    let position = if origin.line == 1 {
        0
    } else {
        let (byte, _) = text
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .nth(origin.line - 2)
            .context("Rule source changed. Reopen the preview.")?;
        text.byte_to_char(byte + 1)
    };
    let line: String = text.chars_at(position).take_while(|&c| c != '\n').collect();
    let line = line.strip_suffix('\r').unwrap_or(&line);
    let line = if position == 0 {
        line.strip_prefix('\u{feff}').unwrap_or(line)
    } else {
        line
    };
    let matches = if source == id {
        origin.matches_source_line(line.as_bytes())
    } else {
        let (bytes, _, lossy) = doc.encoding().encode(line);
        !lossy && origin.matches_source_line(&bytes)
    };
    ensure!(
        matches,
        "The previewed rule no longer matches {}:{} in the editor.",
        display_path(&origin.source),
        origin.line
    );
    editor.switch(id, Action::Replace);
    let (view, doc) = helix_view::current!(editor);
    ensure!(doc.id() == id, "Could not switch to the rule source");
    doc.set_selection(view.id, Selection::point(position));
    align_view(doc, view, Align::Center);
    editor.clear_status();
    Ok(())
}

impl Component for Preview {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let close = || {
            EventResult::Consumed(Some(Box::new(|compositor, _| {
                compositor.remove(ID);
            })))
        };
        if !self.target.current(cx.editor) || matches!(event, Event::Key(ctrl!('c'))) {
            return close();
        }
        match event {
            Event::Key(key!(Esc)) => {
                if self.filter.take().is_none() {
                    return close();
                }
                self.apply_filters();
                return EventResult::Consumed(None);
            }
            Event::Key(key!(Tab)) => {
                self.changed_only = !self.changed_only;
                self.apply_filters();
                return EventResult::Consumed(None);
            }
            Event::Key(key!('/')) if self.filter.is_none() => {
                self.filter = Some(Prompt::new(
                    "Path: ".into(),
                    None,
                    |_, _| Vec::new(),
                    |_, _, _| {},
                ));
                return EventResult::Consumed(None);
            }
            Event::Key(key!(Enter) | key!(Down) | key!(Up) | key!(PageUp) | key!(PageDown)) => (),
            _ => {
                if let Some(prompt) = &mut self.filter {
                    let previous = prompt.line().clone();
                    if let Event::Paste(data) = event {
                        prompt.insert_str(&display_text(data), cx.editor);
                    } else {
                        prompt.handle_event(event, cx);
                    }
                    if *prompt.line() != previous {
                        self.apply_filters();
                    }
                    return EventResult::Consumed(None);
                }
            }
        }
        let (Event::Key(key), Some(results)) = (event, &mut self.results) else {
            return EventResult::Consumed(None);
        };
        if *key != key!(Enter) {
            results.navigate(*key, self.viewport);
        } else if let Some(file) = results.files.get(results.selected) {
            let Some(origin) = &results.matches[file.match_index].winner else {
                cx.editor
                    .set_status("Included by default. There is no rule to jump to.");
                return EventResult::Consumed(None);
            };
            match jump_to_rule(cx.editor, &results.root, origin, self.target.document) {
                Ok(()) => return close(),
                Err(err) => cx.editor.set_error(err.to_string()),
            }
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        self.filter_area = Rect::default();
        if area.area() == 0 {
            return;
        }
        let width = area.width.saturating_sub(2).clamp(1, 110);
        let height = area.height.saturating_sub(2).clamp(1, 32);
        let area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let style = cx.editor.theme.get("ui.popup");
        surface.clear_with(area, style);
        let checked = if self.changed_only { 'x' } else { ' ' };
        let controls = if self.filter.is_some() {
            "↑↓ select · Enter jump · Esc clear path"
        } else {
            "/ path · ↑↓ select · Enter jump · Esc close"
        };
        let block = Block::bordered().title(format!(
            " Gitignore rule preview | Tab [{checked}] changed only · {controls} "
        ));
        let body = block.inner(area);
        block.render(area, surface);
        if !self.target.current(cx.editor) {
            if body.area() > 0 {
                surface.set_stringn(
                    body.x,
                    body.y,
                    "Source changed. Reopen the preview.",
                    body.width as usize,
                    style,
                );
            }
            return;
        }
        if body.area() == 0 {
            return;
        }
        let body = if let Some(prompt) = &mut self.filter {
            let field = body.clip_top(body.height - 1);
            if field.width >= 10 {
                self.filter_area = field;
                prompt.render(field, surface, cx);
            } else {
                surface.set_stringn(field.x, field.y, "Path: ", field.width as usize, style);
            }
            body.clip_bottom(1)
        } else {
            body
        };
        if body.height == 0 {
            return;
        }
        let Some(results) = &mut self.results else {
            surface.set_stringn(
                body.x,
                body.y,
                "Scanning untracked files…",
                body.width as usize,
                style,
            );
            return;
        };
        let list = body.clip_left(2);
        if list.area() == 0 {
            return;
        }
        if list != self.viewport {
            results.reveal(list);
            self.viewport = list;
        }
        results
            .text
            .render(list, surface, style, &mut results.scroll);
        if let Some(rows) = results.selected_rows(list.width) {
            let first = rows.start.saturating_sub(results.scroll);
            let last = rows
                .end
                .saturating_sub(results.scroll)
                .min(usize::from(body.height));
            if first < last {
                let area = body
                    .clip_top(first as u16)
                    .with_height((last - first) as u16);
                let selected = cx.editor.theme.get("ui.menu.selected");
                surface.set_style(area, selected);
                surface.set_stringn(area.x, area.y, ">", 1, selected);
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }

    fn cursor(&self, _area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        match &self.filter {
            Some(prompt) if self.filter_area.area() > 0 => prompt.cursor(self.filter_area, editor),
            _ => (None, CursorKind::Hidden),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignore_preview_escapes_unusual_names_and_bounds_long_patterns() {
        assert_eq!(display_path(Path::new("正常 file.log")), "正常 file.log");
        assert_eq!(display_path(Path::new("two\nrows")), "\"two\\nrows\"");
        assert_eq!(
            display_rule(&"界".repeat(1200)),
            format!("{}…", "界".repeat(1024))
        );
    }
}
