use helix_core::{
    fold::{function_candidates, FoldCandidate},
    Assoc, ChangeSet, Range,
};

use super::{Document, ViewId};

#[derive(Debug, Default)]
pub(super) struct FoldState {
    closed: Vec<FoldCandidate>,
    effective: Vec<std::ops::Range<usize>>,
}

impl FoldState {
    fn rebuild(&mut self) {
        self.closed
            .sort_by_key(|fold| (fold.range.start, std::cmp::Reverse(fold.range.end)));
        self.closed.dedup_by(|a, b| a.range == b.range);
        self.effective.clear();
        for fold in &self.closed {
            if self
                .effective
                .last()
                .is_none_or(|last| last.end <= fold.range.start)
            {
                self.effective.push(fold.range.clone());
            }
        }
    }
}

impl Document {
    pub fn fold_candidates(&self) -> &[FoldCandidate] {
        self.fold_candidates.get_or_init(|| {
            self.syntax().map_or_else(Vec::new, |syntax| {
                function_candidates(self.text().slice(..), syntax, &self.syn_loader.load())
            })
        })
    }

    pub fn folds(&self, view: ViewId) -> &[std::ops::Range<usize>] {
        self.view_data
            .get(&view)
            .map_or(&[], |data| &data.folding.effective)
    }

    pub fn fold_candidate_at(&self, pos: usize) -> Option<&FoldCandidate> {
        let line = self.text().char_to_line(pos.min(self.text().len_chars()));
        self.fold_candidates().iter().rev().find(|fold| {
            self.text().char_to_line(fold.declaration) <= line
                && pos <= fold.function.end
                && line <= self.text().char_to_line(fold.function.end)
        })
    }

    /// Close one function, optionally relocating the primary cursor to its
    /// visible declaration. Other cursors must never become hidden.
    pub fn close_fold(
        &mut self,
        view: ViewId,
        candidate: FoldCandidate,
        relocate_primary: bool,
    ) -> bool {
        if !self.fold_candidates().contains(&candidate) {
            return false;
        }
        let text = self.text().slice(..);
        let selection = self.selection(view);
        let hidden = |pos| candidate.range.start < pos && pos < candidate.range.end;
        if selection.iter().enumerate().any(|(i, range)| {
            hidden(range.cursor(text)) && (!relocate_primary || i != selection.primary_index())
        }) {
            return false;
        }
        if hidden(selection.primary().cursor(text)) {
            let primary = selection.primary_index();
            let mut ranges = selection.ranges().to_vec();
            ranges[primary] = Range::point(candidate.range.start);
            self.set_selection(view, helix_core::Selection::new(ranges.into(), primary));
        }
        let state = &mut self.view_data_mut(view).folding;
        if state.closed.contains(&candidate) {
            return true;
        }
        // Candidates are nested/disjoint; reject stale crossing state defensively.
        state.closed.retain(|other| {
            other.range.end <= candidate.range.start
                || candidate.range.end <= other.range.start
                || (other.range.start <= candidate.range.start
                    && candidate.range.end <= other.range.end)
                || (candidate.range.start <= other.range.start
                    && other.range.end <= candidate.range.end)
        });
        state.closed.push(candidate);
        state.rebuild();
        self.folding_changed();
        true
    }

    /// Open the visible outer fold at the declaration or containing position.
    pub fn unfold_at(&mut self, view: ViewId, pos: usize) -> bool {
        let line = self.text().char_to_line(pos.min(self.text().len_chars()));
        let Some(state) = self.view_data.get(&view).map(|data| &data.folding) else {
            return false;
        };
        let candidate = state
            .closed
            .iter()
            .find(|fold| {
                state.effective.contains(&fold.range)
                    && self.text().char_to_line(fold.declaration) <= line
                    && pos <= fold.range.end
            })
            .cloned();
        let Some(candidate) = candidate else {
            return false;
        };
        let state = &mut self.view_data_mut(view).folding;
        state.closed.retain(|fold| fold.range != candidate.range);
        state.rebuild();
        self.folding_changed();
        true
    }

    pub fn fold_all(&mut self, view: ViewId) {
        for candidate in self.fold_candidates().to_vec() {
            self.close_fold(view, candidate, false);
        }
    }

    pub fn clear_folds(&mut self, view: ViewId) {
        if let Some(data) = self.view_data.get_mut(&view) {
            if data.folding.closed.is_empty() {
                return;
            }
            data.folding = FoldState::default();
            self.folding_changed();
        }
    }

    fn folding_changed(&mut self) {
        self.ai_completion = None;
        self.ai_completion_controller.cancel();
    }

    pub(super) fn reset_folding(&mut self) {
        self.fold_candidates.take();
        for data in self.view_data.values_mut() {
            data.folding = FoldState::default();
        }
        self.folding_changed();
    }

    pub(super) fn reveal_folded_cursors(&mut self, view: ViewId) {
        let Some(data) = self.view_data.get_mut(&view) else {
            return;
        };
        let Some(selection) = self.selections.get(&view) else {
            return;
        };
        let text = self.text.slice(..);
        let before = data.folding.closed.len();
        data.folding.closed.retain(|fold| {
            !selection.iter().any(|range| {
                let pos = range.cursor(text);
                fold.range.start < pos && pos < fold.range.end
            })
        });
        if before != data.folding.closed.len() {
            data.folding.rebuild();
            self.folding_changed();
        }
    }

    pub(super) fn map_folds(&mut self, changes: &ChangeSet) {
        self.fold_candidates.take();
        let edits: Vec<_> = changes
            .changes_iter()
            .map(|(start, end, _)| start..end)
            .collect();
        for data in self.view_data.values_mut() {
            data.folding.closed.retain_mut(|fold| {
                // Signature edits can change what the body means, even if the
                // hidden range itself was untouched. Boundary edits open it too.
                if edits
                    .iter()
                    .any(|edit| edit.start <= fold.function.end && fold.declaration <= edit.end)
                {
                    return false;
                }
                fold.declaration = changes.map_pos(fold.declaration, Assoc::After);
                fold.function.start = changes.map_pos(fold.function.start, Assoc::After);
                fold.function.end = changes.map_pos(fold.function.end, Assoc::Before);
                fold.range.start = changes.map_pos(fold.range.start, Assoc::After);
                fold.range.end = changes.map_pos(fold.range.end, Assoc::Before);
                fold.range.start < fold.range.end && fold.range.end <= self.text.len_chars()
            });
            data.folding.rebuild();
        }
        self.folding_changed();
    }

    pub(super) fn revalidate_folds(&mut self) {
        if self
            .view_data
            .values()
            .all(|data| data.folding.closed.is_empty())
        {
            return;
        }
        let candidates = self.fold_candidates().to_vec();
        for data in self.view_data.values_mut() {
            data.folding.closed.retain(|fold| {
                candidates
                    .binary_search_by_key(&fold.range.start, |candidate| candidate.range.start)
                    .is_ok_and(|i| candidates[i] == *fold)
            });
            data.folding.rebuild();
        }
    }
}
