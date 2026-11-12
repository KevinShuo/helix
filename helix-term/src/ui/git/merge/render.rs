use super::*;
use helix_view::{current_ref, graphics::Style};
use tui::buffer::Buffer;

pub(crate) fn line(frame: &mut Buffer, area: Rect, row: usize, text: &str, style: Style) {
    if row >= area.height as usize || area.width == 0 {
        return;
    }
    frame.set_stringn(
        area.x,
        area.y + row as u16,
        &crate::ui::git::render::safe_text(text),
        area.width as usize,
        style,
    );
}
const BUTTONS: &[(&str, MergeUiAction)] = &[
    ("Prev", MergeUiAction::Previous),
    ("Next", MergeUiAction::Next),
    ("Current", MergeUiAction::UseCurrent),
    ("Incoming", MergeUiAction::UseIncoming),
    ("Both L+R", MergeUiAction::KeepBoth),
    ("Base", MergeUiAction::ShowBase),
    ("Review", MergeUiAction::MarkReviewed),
    ("Apply", MergeUiAction::Apply),
    ("Files", MergeUiAction::Files),
];
pub(super) fn button_at(area: Rect, x: u16) -> Option<MergeUiAction> {
    let mut start = area.x;
    for (label, action) in BUTTONS {
        let end = start + label.len() as u16 + 3;
        if x >= start && x < end.min(area.right()) {
            return Some(*action);
        }
        start = end;
    }
    None
}
fn source(
    frame: &mut Buffer,
    area: Rect,
    text: &helix_core::Rope,
    top: usize,
    style: Style,
    selected: Option<std::ops::Range<usize>>,
    highlight: Style,
) {
    if area.area() == 0 {
        return;
    }
    frame.clear_with(area, style);
    for (row, text_line) in text
        .lines()
        .skip(top)
        .take(area.height as usize)
        .enumerate()
    {
        let at = text.line_to_char((row + top).min(text.len_lines() - 1));
        let active = selected.as_ref().is_some_and(|range| {
            at < range.end && at + text_line.len_chars() > range.start
                || range.is_empty() && at == range.start
        });
        line(
            frame,
            area,
            row,
            &text_line.to_string(),
            if active { highlight } else { style },
        );
    }
}
impl MergeEditor {
    pub(super) fn draw(&mut self, frame: &mut Buffer, cx: &mut Context) {
        let (view, doc) = current_ref!(cx.editor);
        let state = doc.merge().unwrap();
        let layout = self.last_layout;
        let style = cx.editor.theme.get("ui.text");
        let highlight = cx.editor.theme.get("ui.selection");
        let model = &state.model;
        if self
            .maps
            .as_ref()
            .is_none_or(|(version, _)| *version != doc.version())
        {
            self.maps = Some((doc.version(), model.line_maps(doc.text().slice(..))));
        }
        let top = doc
            .text()
            .char_to_line(doc.view_offset(view.id).anchor.min(doc.text().len_chars()));
        let maps = &self.maps.as_ref().unwrap().1;
        let mapped = |map: &[usize]| map.get(top).copied().unwrap_or(0);
        let block = model.blocks().get(self.selected);
        frame.clear_with(layout.header, style);
        frame.clear_with(layout.footer, style);
        let status = match &state.write_status {
            MergeWriteStatus::Draft => {
                if model.ready_to_apply() {
                    "Ready to Apply"
                } else {
                    "Review required"
                }
            }
            MergeWriteStatus::Staged => "Staged",
            MergeWriteStatus::NeedsRefresh(message) => message,
            MergeWriteStatus::Retry { version, .. } if *version != doc.version() => {
                "Draft changed after write; refresh required"
            }
            MergeWriteStatus::Retry { message, .. } => message,
        };
        let identity = match &state.data.operation {
            helix_vcs::git_tools::ConflictOperation::Merge { head, parents } => format!(
                "{} <- {}",
                &head[..head.len().min(8)],
                parents
                    .iter()
                    .map(|p| &p[..p.len().min(8)])
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            _ => String::new(),
        };
        line(
            frame,
            layout.header,
            0,
            &format!(
                "Merge {identity} · {} · block {}/{} · {}{}",
                state.data.file.path.display(),
                self.selected + 1,
                model.blocks().len(),
                status,
                if state.busy { " · Applying..." } else { "" }
            ),
            style,
        );
        if layout.wide {
            for (rect, label) in [
                (layout.current, "Current [read-only]"),
                (
                    layout.result,
                    if matches!(state.write_status, MergeWriteStatus::Staged) {
                        "Result [staged, read-only]"
                    } else {
                        "Result [editable]"
                    },
                ),
                (layout.incoming, "Incoming [read-only]"),
            ] {
                line(
                    frame,
                    Rect::new(rect.x, layout.header.y, rect.width, layout.header.height),
                    1,
                    label,
                    style,
                );
            }
        } else {
            line(
                frame,
                layout.header,
                1,
                if matches!(state.write_status, MergeWriteStatus::Staged) {
                    "Current [RO]      Result [staged]   Incoming [RO]    Base [RO]"
                } else {
                    "Current [RO]      Result [editable] Incoming [RO]    Base [RO]"
                },
                style,
            );
        }
        if layout.wide {
            source(
                frame,
                layout.current,
                model.sources().current(),
                mapped(&maps.current) + self.scroll[1],
                style,
                block.map(|b| b.current.clone()),
                highlight,
            );
            source(
                frame,
                layout.incoming,
                model.sources().incoming(),
                mapped(&maps.incoming) + self.scroll[2],
                style,
                block.map(|b| b.incoming.clone()),
                highlight,
            );
        }
        if self.preview != Preview::Result {
            let (text, map, range) = match self.preview {
                Preview::Current => (
                    model.sources().current(),
                    &maps.current,
                    block.map(|b| b.current.clone()),
                ),
                Preview::Incoming => (
                    model.sources().incoming(),
                    &maps.incoming,
                    block.map(|b| b.incoming.clone()),
                ),
                _ => (
                    model.sources().base(),
                    &maps.base,
                    block.map(|b| b.base.clone()),
                ),
            };
            if !layout.wide || self.preview == Preview::Base {
                source(
                    frame,
                    layout.result,
                    text,
                    mapped(map) + self.scroll[self.source_index()],
                    style,
                    range,
                    highlight,
                );
            }
            line(
                frame,
                layout.footer,
                0,
                "Read-only preview · j/k scroll · 1 Current · 2/Esc Result · 3 Incoming · 4 Base",
                style,
            );
        } else {
            let mut x = layout.footer.x;
            for (label, action) in BUTTONS {
                let width = label.len() as u16 + 3;
                let button_style = if *action == MergeUiAction::Apply
                    && (!model.ready_to_apply()
                        || state.busy
                        || !matches!(state.write_status, MergeWriteStatus::Draft))
                {
                    cx.editor.theme.get("ui.text.inactive")
                } else {
                    style
                };
                if x < layout.footer.right() {
                    line(
                        frame,
                        Rect::new(
                            x,
                            layout.footer.y,
                            width.min(layout.footer.right() - x),
                            layout.footer.height,
                        ),
                        0,
                        &format!("[{label}] "),
                        button_style,
                    );
                }
                x = x.saturating_add(width);
            }
        }
        if let Some(action) = self.confirmation {
            let warning = match action {
                MergeUiAction::AcceptCurrentFile => "Discard ALL Incoming changes? y/n",
                MergeUiAction::AcceptIncomingFile => "Discard ALL Current changes? y/n",
                MergeUiAction::Rebuild => "Discard ALL draft edits? y/n",
                _ => "Discard draft; reload disk/index? y/n",
            };
            line(frame, layout.footer, 0, warning, highlight);
        }
    }
}
