use super::*;
use helix_view::{graphics::Style, Theme};
use tui::widgets::{Block, Widget};

pub(super) fn safe_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn line(frame: &mut Buffer, area: Rect, row: usize, text: &str, style: Style) {
    if row >= area.height as usize {
        return;
    }
    frame.set_stringn(
        area.x,
        area.y + row as u16,
        &safe_text(text),
        area.width as usize,
        style,
    );
}

fn inner(area: Rect) -> Rect {
    area.clip_top(1).clip_bottom(1).clip_left(1).clip_right(1)
}

fn scroll_to(selected: usize, height: usize, scroll: &mut usize) {
    if height == 0 {
        return;
    }
    if selected < *scroll {
        *scroll = selected;
    }
    if selected >= scroll.saturating_add(height) {
        *scroll = selected + 1 - height;
    }
}

pub(super) fn file_header_rows(details: &GitDetails, area: Rect) -> usize {
    let height = inner(area).height as usize;
    (details
        .message
        .lines()
        .count()
        .min(3)
        .min(height.saturating_sub(3))
        + 2)
    .min(height)
}

impl GitPanel {
    fn pane(&self, frame: &mut Buffer, theme: &Theme, rect: Rect, pane: Pane, title: &str) -> Rect {
        if rect.area() == 0 {
            return rect;
        }
        Block::bordered()
            .style(theme.get("ui.window"))
            .render(rect, frame);
        let title_style = if self.focused && self.focus == pane {
            theme.get("ui.text.focus")
        } else {
            theme.get("ui.text.inactive")
        };
        frame.set_stringn(
            rect.x.saturating_add(1),
            rect.y,
            title,
            rect.width.saturating_sub(2) as usize,
            title_style,
        );
        inner(rect)
    }

    pub(super) fn draw(&mut self, frame: &mut Buffer, theme: &Theme) {
        let l = self.last_layout;
        if l.panel.area() == 0 {
            return;
        }
        let text = theme.get("ui.text");
        let muted = theme.get("ui.text.inactive");
        let selected = theme.get("ui.selection");
        frame.clear_with(l.panel, theme.get("ui.background").patch(text));
        let root = self
            .repository
            .as_ref()
            .map(|r| r.root())
            .unwrap_or(&self.path);
        let name = root.file_name().unwrap_or_default().to_string_lossy();
        let kind = if matches!(self.target, GitTarget::WorkingTree) {
            "Changes"
        } else if self.filter.path.is_some() {
            "File history"
        } else {
            "Log"
        };
        let header = format!(
            " Git · {kind}   {name}   HEAD: {}{}   z/F11: {}   a: actions",
            self.snapshot.head_name,
            if self.busy { " [loading]" } else { "" },
            if self.fullscreen {
                "dock"
            } else {
                "fullscreen"
            }
        );
        line(frame, l.panel, 0, &header, theme.get("ui.statusline"));
        let filter = format!(
            " / {}   branch: {}   A author: {}   D date: {}..{}   p path: {}",
            if self.filter.text.is_empty() {
                "search"
            } else {
                &self.filter.text
            },
            self.filter.revision.as_deref().unwrap_or("all"),
            if self.filter.author.is_empty() {
                "all"
            } else {
                &self.filter.author
            },
            self.filter.since,
            self.filter.until,
            self.filter
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "all".into())
        );
        line(frame, l.panel, 1, &filter, muted);

        if l.branches.area() > 0 {
            let title = format!(
                " Branches [1]{} ",
                if self.branch_query.is_empty() {
                    String::new()
                } else {
                    format!(" / {}", self.branch_query)
                }
            );
            let area = self.pane(frame, theme, l.branches, Pane::Branches, &title);
            let rows = self.branch_rows();
            scroll_to(
                self.selected_branch,
                area.height as usize,
                &mut self.branch_scroll,
            );
            for (row, (index, entry)) in rows
                .iter()
                .enumerate()
                .skip(self.branch_scroll)
                .take(area.height as usize)
                .enumerate()
            {
                let style = if index == self.selected_branch && self.focus == Pane::Branches {
                    selected
                } else if entry
                    .reference
                    .is_some_and(|i| self.snapshot.references[i].current)
                {
                    theme.get("special")
                } else {
                    text
                };
                if index == self.selected_branch && self.focus == Pane::Branches {
                    frame.set_style(area.clip_top(row as u16).with_height(1), style);
                }
                line(frame, area, row, &entry.label, style);
            }
        }
        if l.commits.area() > 0 {
            let title = format!(
                " Commits [2] · {}{} ",
                self.snapshot.commits.len(),
                if self.snapshot.has_more { "+" } else { "" }
            );
            let area = self.pane(frame, theme, l.commits, Pane::Commits, &title);
            let rows: Vec<_> = self
                .snapshot
                .commits
                .iter()
                .enumerate()
                .flat_map(|(i, c)| {
                    std::iter::once((i, c.graph.as_str(), false)).chain(
                        c.graph_after
                            .iter()
                            .map(move |graph| (i, graph.as_str(), true)),
                    )
                })
                .collect();
            let selected_row = rows
                .iter()
                .position(|(i, _, after)| *i == self.selected_commit && !after)
                .unwrap_or(0);
            scroll_to(selected_row, area.height as usize, &mut self.commit_scroll);
            let graph_width = rows
                .iter()
                .map(|(_, g, _)| g.len())
                .max()
                .unwrap_or(2)
                .min((area.width / 3).max(1) as usize);
            for (row, (index, graph, connector)) in rows
                .iter()
                .skip(self.commit_scroll)
                .take(area.height as usize)
                .enumerate()
            {
                let commit = &self.snapshot.commits[*index];
                let active = *index == self.selected_commit
                    && !connector
                    && matches!(self.target, GitTarget::Commit(_));
                let style = if active { selected } else { text };
                if active {
                    frame.set_style(area.clip_top(row as u16).with_height(1), style);
                }
                let graph = graph.replace('*', "●").replace('|', "│");
                line(
                    frame,
                    area.with_width(graph_width as u16),
                    row,
                    &graph,
                    theme
                        .get("special")
                        .patch(if active { selected } else { Style::default() }),
                );
                if *connector {
                    continue;
                }
                let content = area.clip_left(graph_width as u16);
                let description = format!(
                    "{} {}{}",
                    &commit.id[..8.min(commit.id.len())],
                    commit.subject,
                    if commit.decoration.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", commit.decoration)
                    }
                );
                let metadata = if content.width >= 55 {
                    format!(
                        "{} {}",
                        commit.author,
                        commit.date.get(..10).unwrap_or(&commit.date)
                    )
                } else {
                    String::new()
                };
                let meta_width = if metadata.is_empty() {
                    0
                } else {
                    24.min(content.width / 2)
                };
                line(
                    frame,
                    content.clip_right(meta_width + u16::from(meta_width > 0)),
                    row,
                    &description,
                    style,
                );
                if meta_width > 0 {
                    line(
                        frame,
                        content.clip_left(content.width - meta_width),
                        row,
                        &metadata,
                        muted,
                    );
                }
            }
            if self.snapshot.commits.is_empty() {
                line(
                    frame,
                    area,
                    0,
                    if self.busy {
                        "Loading history..."
                    } else {
                        "No matching commits"
                    },
                    muted,
                );
                line(
                    frame,
                    area,
                    1,
                    "s: working changes  R: clear filters",
                    muted,
                );
            }
        }
        if l.files.area() > 0 {
            let area = self.pane(frame, theme, l.files, Pane::Files, " Details / files [3] ");
            let offset = file_header_rows(&self.details, l.files);
            let summary = match &self.target {
                GitTarget::Commit(id) => format!("{}", &id[..8.min(id.len())]),
                GitTarget::WorkingTree => "Working tree vs HEAD".into(),
            };
            let message = format!("{summary}\n{}", self.details.message);
            for (row, textline) in message.lines().take(offset.saturating_sub(1)).enumerate() {
                line(frame, area, row, textline, muted);
            }
            line(
                frame,
                area,
                offset.saturating_sub(1),
                &format!("Files · {}", self.details.files.len()),
                theme.get("ui.text.focus"),
            );
            let files_area = area.clip_top(offset as u16);
            let rows = self.file_rows();
            let selected_row = rows
                .iter()
                .position(|row| matches!(row, FileRow::File(index) if *index == self.selected_file))
                .unwrap_or(0);
            scroll_to(
                selected_row,
                files_area.height as usize,
                &mut self.file_scroll,
            );
            for (row, entry) in rows
                .iter()
                .skip(self.file_scroll)
                .take(files_area.height as usize)
                .enumerate()
            {
                let index = match entry {
                    FileRow::Conflicts(count) => {
                        line(
                            frame,
                            files_area,
                            row,
                            &format!("Conflicts ({count})"),
                            theme.get("ui.text.focus"),
                        );
                        continue;
                    }
                    FileRow::Changes(count) => {
                        line(
                            frame,
                            files_area,
                            row,
                            &format!("Other changes ({count})"),
                            theme.get("ui.text.focus"),
                        );
                        continue;
                    }
                    FileRow::File(index) => *index,
                };
                let file = &self.details.files[index];
                let style = if index == self.selected_file {
                    selected
                } else {
                    text
                };
                if index == self.selected_file {
                    frame.set_style(files_area.clip_top(row as u16).with_height(1), style);
                }
                let display = match &file.old_path {
                    Some(old) => format!(
                        "{} {} → {}",
                        file.status,
                        old.display(),
                        file.path.display()
                    ),
                    None => format!("{} {}", file.status, file.path.display()),
                };
                line(frame, files_area, row, &display, style);
            }
        }
        if l.diff.area() > 0 {
            let title = self
                .details
                .files
                .get(self.selected_file)
                .map(|f| format!(" Diff [4] · {} ", f.path.display()))
                .unwrap_or_else(|| " Diff [4] ".into());
            let area = self.pane(frame, theme, l.diff, Pane::Diff, &title);
            self.diff_scroll = self
                .diff_scroll
                .min(self.patch.lines().count().saturating_sub(1));
            for (row, textline) in self
                .patch
                .lines()
                .skip(self.diff_scroll)
                .take(area.height as usize)
                .enumerate()
            {
                let style = if textline.starts_with('+') {
                    theme.get("diff.plus")
                } else if textline.starts_with('-') {
                    theme.get("diff.minus")
                } else if textline.starts_with("@@") {
                    theme.get("diff.delta")
                } else {
                    text
                };
                let textline = textline
                    .replace('\t', "    ")
                    .chars()
                    .skip(self.diff_horizontal)
                    .collect::<String>();
                line(frame, area, row, &textline, style);
            }
            if self.patch.is_empty() {
                line(
                    frame,
                    area,
                    0,
                    if self.patch_task.is_some() {
                        "Loading diff..."
                    } else {
                        "Select a changed file · o: open file  O: open revision"
                    },
                    muted,
                );
            }
        }
        let footer = if self.message.is_empty() {
            format!(" Tab: panes  j/k: move  Enter: view  s: changes  r: refresh  y: hash  q: close  Space v: Git{}",if self.focused{""}else{" · editor focus"})
        } else {
            format!(
                " {}  |  Tab: panes  a: actions  z: fullscreen  q: close",
                self.message
            )
        };
        line(
            frame,
            l.panel,
            l.panel.height.saturating_sub(1) as usize,
            &footer,
            theme.get("ui.statusline"),
        );
        if self.action_menu {
            let branch = self
                .selected_reference()
                .map(|r| r.name.clone())
                .unwrap_or_else(|| self.snapshot.head_name.clone());
            self.popup(
                frame,
                theme,
                &[
                    format!("Git actions · {branch}"),
                    "c  Checkout selected branch / tag".into(),
                    "n  New branch from selected commit".into(),
                    "m  Merge selected branch into HEAD".into(),
                    "p  Cherry-pick selected commit".into(),
                    "f  Fetch all remotes".into(),
                    "u  Pull (fast-forward only)".into(),
                    "P  Push current branch".into(),
                    "Esc  Cancel".into(),
                ],
            );
        }
        if let Some(operation) = &self.confirmation {
            let mut lines = vec!["Confirm Git operation".into()];
            lines.extend(actions::operation_description(
                operation,
                &self.snapshot.head_name,
            ));
            lines.push("y  Execute     n / Esc  Cancel".into());
            self.popup(frame, theme, &lines);
        }
    }

    fn popup(&self, frame: &mut Buffer, theme: &Theme, lines: &[String]) {
        let panel = self.last_layout.panel;
        let width = panel.width.min(76);
        let height = (lines.len() as u16 + 2).min(panel.height);
        let area = Rect::new(
            panel.right().saturating_sub(width),
            panel.bottom().saturating_sub(height),
            width,
            height,
        );
        frame.clear_with(area, theme.get("ui.popup").patch(theme.get("ui.text")));
        Block::bordered()
            .style(theme.get("ui.window"))
            .render(area, frame);
        let content = inner(area);
        for (row, text) in lines.iter().enumerate() {
            line(frame, content, row, text, theme.get("ui.text"));
        }
    }
}
