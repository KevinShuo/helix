use std::sync::Arc;

use helix_core::{
    graphemes::grapheme_width, syntax::Highlight, unicode::segmentation::UnicodeSegmentation,
    RopeSlice,
};

use super::{LineMap, MinimapRender};

pub const SOURCE_COLUMNS: usize = 160;
const NO_HIGHLIGHT: u16 = u16::MAX;

/// A bounded row: occupied display columns and eight approximate syntax-color bands.
#[derive(Debug, Clone)]
pub struct SourceRow {
    bits: [u64; 3],
    colors: [u16; 8],
}

impl SourceRow {
    pub fn from_line(
        line: RopeSlice<'_>,
        tab_width: usize,
        mut highlight: impl FnMut(usize) -> Option<Highlight>,
    ) -> Self {
        let mut row = Self {
            bits: [0; 3],
            colors: [NO_HIGHLIGHT; 8],
        };
        // Bound the input BEFORE grapheme segmentation: a single cluster may contain millions of marks.
        let prefix = line.slice(..line.len_chars().min(4096)).to_string();
        let mut column = 0;
        for (byte, grapheme) in prefix.grapheme_indices(true) {
            if column >= SOURCE_COLUMNS {
                break;
            }
            if grapheme == "\t" {
                let tab = tab_width.max(1);
                column += tab - column % tab;
                continue;
            }
            if grapheme.contains(['\r', '\n']) {
                break;
            }
            let width = grapheme_width(grapheme);
            if !grapheme.chars().all(char::is_whitespace) {
                let color = highlight(byte)
                    .and_then(|h| u16::try_from(h.get()).ok())
                    .unwrap_or(NO_HIGHLIGHT);
                for col in column..(column + width).min(SOURCE_COLUMNS) {
                    row.bits[col / 64] |= 1 << (col % 64);
                    if color != NO_HIGHLIGHT {
                        row.colors[col / 20] = color;
                    }
                }
            }
            column += width;
        }
        row
    }

    pub fn occupied(&self, column: usize) -> bool {
        column < SOURCE_COLUMNS && self.bits[column / 64] & (1 << (column % 64)) != 0
    }

    pub fn highlight(&self, column: usize) -> Option<Highlight> {
        let color = self.colors[column.min(SOURCE_COLUMNS - 1) / 20];
        (color != NO_HIGHLIGHT).then_some(Highlight::new(color as u32))
    }
}

pub fn source_lines(lines: usize, degraded: bool, height: u16) -> Vec<usize> {
    let lines = lines.max(1);
    if !degraded {
        return (0..lines).collect();
    }
    let count = source_sample_count(lines, degraded, height);
    (0..count)
        .map(|i| (i as u128 * (lines - 1) as u128 / (count - 1).max(1) as u128) as usize)
        .collect()
}

pub fn source_sample_count(lines: usize, degraded: bool, height: u16) -> usize {
    if degraded {
        (height as usize * 4 * 8).clamp(2, 4096).min(lines.max(1))
    } else {
        lines.max(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceKey {
    pub version: i32,
    pub epoch: u64,
    pub tab_width: usize,
}

#[derive(Debug)]
pub struct MinimapSummary {
    pub key: SourceKey,
    pub lines: usize,
    pub degraded: bool,
    pub rows: Vec<(usize, SourceRow)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectionKey {
    pub width: u16,
    pub height: u16,
    pub render: MinimapRender,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MiniCell {
    pub dots: u8,
    pub highlight: Option<Highlight>,
    weight: u16,
}

#[derive(Debug)]
pub struct Projection {
    pub key: ProjectionKey,
    pub cells: Vec<MiniCell>,
    map: LineMap,
}

impl Projection {
    pub fn new(lines: usize, key: ProjectionKey) -> Self {
        Self {
            key,
            cells: vec![MiniCell::default(); key.width as usize * key.height as usize],
            map: LineMap::new(lines, key.height, key.render),
        }
    }

    pub fn add_row(&mut self, line: usize, row: &SourceRow) {
        let pixel_y = self.map.pixel(line);
        let scale = self.key.render.pixel_height();
        let y = pixel_y / scale;
        for col in 0..SOURCE_COLUMNS {
            if !row.occupied(col) {
                continue;
            }
            let pixel_x = col * self.key.width as usize * 2 / SOURCE_COLUMNS;
            let cell = &mut self.cells[y * self.key.width as usize + pixel_x / 2];
            cell.dots |= 1 << ((pixel_y % scale) * 2 + pixel_x % 2);
            let color = row.highlight(col);
            if color == cell.highlight {
                cell.weight = cell.weight.saturating_add(1);
            } else if cell.weight == 0 {
                cell.highlight = color;
                cell.weight = 1;
            } else {
                cell.weight -= 1;
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct MinimapState {
    pub epoch: u64,
    pub serial: u64,
    pub pending: bool,
    pub controller: helix_event::TaskController,
    pub source_budget: usize,
    pub requested: Vec<ProjectionKey>,
    pub summary: Option<Arc<MinimapSummary>>,
    pub projections: Vec<Projection>,
}

impl MinimapState {
    pub fn invalidate(&mut self) {
        self.controller.cancel();
        self.epoch = self.epoch.wrapping_add(1);
        self.serial = self.serial.wrapping_add(1);
        self.pending = false;
        self.requested.clear();
        self.summary = None;
        self.projections.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minimap::{LineMap, MinimapRender};
    use helix_core::{syntax::Highlight, Rope};

    #[test]
    fn minimap_summary_preserves_indentation_unicode_and_color() {
        let text = Rope::from_str("\t中e\u{301}😀 x\n");
        let row = SourceRow::from_line(text.slice(..), 4, |_| Some(Highlight::new(7)));
        assert!(!(0..4).any(|col| row.occupied(col)));
        assert!((4..9).all(|col| row.occupied(col)));
        assert!(!row.occupied(9));
        assert!(row.occupied(10));
        assert_eq!(row.highlight(4), Some(Highlight::new(7)));
        let long = Rope::from_str(&format!("{}X", " ".repeat(160)));
        let row = SourceRow::from_line(long.slice(..), 4, |_| None);
        assert!(!(0..160).any(|col| row.occupied(col)));
        let zero_width = Rope::from_str(&format!("a{}X", "\u{301}".repeat(100_000)));
        let row = SourceRow::from_line(zero_width.slice(..), 4, |_| None);
        assert!(row.occupied(0));
        assert!(!row.occupied(1));
    }

    #[test]
    fn minimap_sampling_and_projection_are_bounded() {
        let plan = source_lines(1_000_000, true, 50);
        assert!(plan.len() <= 1600);
        assert_eq!(plan[0], 0);
        assert_eq!(plan.last(), Some(&999_999));
        for render in [MinimapRender::Braille, MinimapRender::Blocks] {
            let map = LineMap::new(100, 10, render);
            let mut p = Projection::new(
                100,
                ProjectionKey {
                    width: 13,
                    height: 10,
                    render,
                },
            );
            let line = Rope::from_str("    fn hello() {}\n");
            p.add_row(
                99,
                &SourceRow::from_line(line.slice(..), 4, |_| Some(Highlight::new(3))),
            );
            assert_eq!(p.cells.len(), 130);
            assert!(p.cells.iter().any(|cell| cell.dots != 0));
            assert!(p.cells[(map.row(99) as usize * 13)..]
                .iter()
                .any(|c| c.highlight == Some(Highlight::new(3))));
        }
    }

    #[test]
    fn minimap_invalidation_cancels_work_and_drops_cached_geometry() {
        let mut state = MinimapState::default();
        let handle = state.controller.restart();
        state.pending = true;
        state.requested.push(ProjectionKey {
            width: 13,
            height: 20,
            render: MinimapRender::Braille,
        });
        let epoch = state.epoch;
        state.invalidate();
        assert!(handle.is_canceled());
        assert!(!state.pending);
        assert!(state.requested.is_empty());
        assert_ne!(state.epoch, epoch);
    }
}
