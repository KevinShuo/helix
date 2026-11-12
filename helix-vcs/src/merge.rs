//! Pure text model for an editable three-way merge.

use std::ops::Range;

use anyhow::{ensure, Context, Result};
use helix_core::{Assoc, ChangeSet, Rope, RopeSlice, Transaction};
use imara_diff::{Algorithm, Diff, InternedInput};

use crate::git_tools::{MergeTextFormat, MAX_MERGE_BYTES, MAX_MERGE_LINES};

#[cfg(not(feature = "git"))]
impl crate::git_tools::GitRepository {
    pub async fn merge_candidate(
        &self,
        _data: &crate::git_tools::GitConflictData,
    ) -> Result<MergeCandidate> {
        anyhow::bail!("Git support was not compiled in")
    }
}

impl MergeTextFormat {
    /// Decode without loss, removing the BOM and normalizing CRLF for the draft.
    pub fn decode(&self, bytes: &[u8]) -> Result<Rope> {
        ensure!(Self::detect(bytes)? == *self, "Merge text format changed");
        let bytes = if self.bom { &bytes[3..] } else { bytes };
        Ok(Rope::from(
            std::str::from_utf8(bytes)?.replace("\r\n", "\n"),
        ))
    }

    /// Encode the LF-only draft, preserving BOM, line endings and missing EOF newline.
    /// Limits apply to encoded bytes, including an expanded CRLF or BOM.
    pub fn encode(&self, text: RopeSlice<'_>) -> Result<Vec<u8>> {
        ensure!(text.len_bytes() <= MAX_MERGE_BYTES, "Result exceeds 2 MiB");
        let text = text.to_string();
        ensure!(
            !text.contains('\r'),
            "Merge drafts must use LF line endings"
        );
        ensure!(!text.contains('\0'), "Binary merge results are unsupported");
        ensure!(
            text.lines().count() <= MAX_MERGE_LINES,
            "Result exceeds 50,000 lines"
        );
        let encoded = if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        };
        let mut bytes = Vec::with_capacity(encoded.len() + if self.bom { 3 } else { 0 });
        if self.bom {
            bytes.extend_from_slice(b"\xef\xbb\xbf");
        }
        bytes.extend_from_slice(encoded.as_bytes());
        ensure!(
            bytes.len() <= MAX_MERGE_BYTES,
            "Encoded result exceeds 2 MiB"
        );
        Ok(bytes)
    }
}

#[derive(Clone, Debug)]
pub struct MergeSources {
    base: Rope,
    current: Rope,
    incoming: Rope,
}

impl MergeSources {
    pub fn new(base: Rope, current: Rope, incoming: Rope) -> Self {
        Self {
            base,
            current,
            incoming,
        }
    }
    pub fn base(&self) -> &Rope {
        &self.base
    }
    pub fn current(&self) -> &Rope {
        &self.current
    }
    pub fn incoming(&self) -> &Rope {
        &self.incoming
    }

    /// Recompute once per document version, then cache for linked scrolling.
    pub fn line_maps(&self, result: RopeSlice<'_>) -> MergeLineMaps {
        MergeLineMaps {
            base: rope_line_map(result, self.base.slice(..)),
            current: rope_line_map(result, self.current.slice(..)),
            incoming: rope_line_map(result, self.incoming.slice(..)),
        }
    }
}

/// Zero-based source line for each result line, with a final EOF boundary entry.
/// Within changed regions the correspondence is approximate and monotonic.
#[derive(Clone, Debug)]
pub struct MergeLineMaps {
    pub base: Vec<usize>,
    pub current: Vec<usize>,
    pub incoming: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockDecision {
    Unresolved,
    Reviewed,
    NeedsReview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    Current,
    Incoming,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMode {
    Blockwise,
    WholeFileReview,
}

/// All ranges are character offsets; sources never change with result edits.
#[derive(Clone, Debug)]
pub struct ConflictBlock {
    pub base: Range<usize>,
    pub current: Range<usize>,
    pub incoming: Range<usize>,
    pub result: Range<usize>,
    pub decision: BlockDecision,
}

#[derive(Clone, Debug)]
pub struct MergeCandidate {
    pub result: Rope,
    pub sources: MergeSources,
    pub blocks: Vec<ConflictBlock>,
    pub matches_working: bool,
    working: Option<Rope>,
}

/// Owns immutable initialization and review metadata. The editor Document owns
/// the only mutable result; every edit (including undo/redo) must call on_change.
#[derive(Clone, Debug)]
pub struct MergeModel {
    initial_result: Rope,
    sources: MergeSources,
    blocks: Vec<ConflictBlock>,
    mode: MergeMode,
    whole_reviewed: bool,
    result_len: usize,
}

impl MergeModel {
    /// A mismatching working file is preserved automatically and requires whole-file review.
    pub fn from_candidate(candidate: MergeCandidate) -> Self {
        if !candidate.matches_working {
            return Self::preserve_working(
                candidate.working.unwrap_or(candidate.result),
                candidate.sources,
            );
        }
        Self {
            result_len: candidate.result.len_chars(),
            initial_result: candidate.result,
            sources: candidate.sources,
            blocks: candidate.blocks,
            mode: MergeMode::Blockwise,
            whole_reviewed: false,
        }
    }

    pub fn preserve_working(text: Rope, sources: MergeSources) -> Self {
        Self {
            result_len: text.len_chars(),
            initial_result: text,
            sources,
            blocks: Vec::new(),
            mode: MergeMode::WholeFileReview,
            whole_reviewed: false,
        }
    }

    pub fn initial_result(&self) -> &Rope {
        &self.initial_result
    }
    pub fn sources(&self) -> &MergeSources {
        &self.sources
    }
    pub fn blocks(&self) -> &[ConflictBlock] {
        &self.blocks
    }
    pub fn mode(&self) -> MergeMode {
        self.mode
    }
    pub fn line_maps(&self, result: RopeSlice<'_>) -> MergeLineMaps {
        self.sources.line_maps(result)
    }

    /// Apply the returned transaction to the Document, call on_change, then
    /// mark_reviewed(Some(block)). Merely requesting a choice changes no state.
    pub fn choice(
        &self,
        block: usize,
        choice: ConflictChoice,
        result: RopeSlice<'_>,
    ) -> Result<Transaction> {
        ensure!(
            self.mode == MergeMode::Blockwise,
            "Whole-file review has no block replacement actions"
        );
        ensure!(
            result.len_chars() == self.result_len,
            "Merge ranges are stale"
        );
        let block = self.blocks.get(block).context("Unknown conflict block")?;
        let current = || {
            self.sources
                .current
                .slice(block.current.clone())
                .to_string()
        };
        let incoming = || {
            self.sources
                .incoming
                .slice(block.incoming.clone())
                .to_string()
        };
        let replacement = match choice {
            ConflictChoice::Current => current(),
            ConflictChoice::Incoming => incoming(),
            ConflictChoice::Both => {
                let mut text = current();
                let right = incoming();
                if !text.is_empty() && !right.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&right);
                text
            }
        };
        let doc = Rope::from(result);
        let tx = Transaction::change(
            &doc,
            [(
                block.result.start,
                block.result.end,
                Some(replacement.into()),
            )]
            .into_iter(),
        );
        let mut after = doc;
        ensure!(tx.apply(&mut after), "Could not construct merge choice");
        MergeTextFormat {
            bom: false,
            crlf: false,
        }
        .encode(after.slice(..))?;
        Ok(tx)
    }

    pub fn on_change(&mut self, changes: &ChangeSet) {
        if changes.is_empty() {
            return;
        }
        self.whole_reviewed = false;
        let mut touched = vec![false; self.blocks.len()];
        let mut ambiguous = false;
        for (from, to, _) in changes.changes_iter() {
            let mut count = 0;
            for (index, block) in self.blocks.iter().enumerate() {
                let range = &block.result;
                let intersects = if from == to {
                    from >= range.start && from <= range.end
                } else {
                    from < range.end && to > range.start
                        || range.is_empty() && from <= range.start && to >= range.end
                };
                if intersects {
                    count += 1;
                    touched[index] = true;
                    if from < range.start || to > range.end {
                        ambiguous = true;
                    }
                }
            }
            ambiguous |= count > 1;
        }
        if ambiguous {
            self.mode = MergeMode::WholeFileReview;
            self.blocks.clear();
        } else {
            for (index, block) in self.blocks.iter_mut().enumerate() {
                block.result = changes.map_pos(block.result.start, Assoc::Before)
                    ..changes.map_pos(block.result.end, Assoc::After);
                if touched[index] {
                    block.decision = BlockDecision::NeedsReview;
                }
            }
        }
        self.result_len = changes.map_pos(self.result_len, Assoc::After);
    }

    /// None reviews the complete file in WholeFileReview mode only.
    pub fn mark_reviewed(&mut self, block: Option<usize>) {
        match (self.mode, block) {
            (MergeMode::WholeFileReview, None) => self.whole_reviewed = true,
            (MergeMode::Blockwise, Some(index)) => {
                if let Some(block) = self.blocks.get_mut(index) {
                    block.decision = BlockDecision::Reviewed;
                }
            }
            _ => {}
        }
    }

    pub fn ready_to_apply(&self) -> bool {
        match self.mode {
            MergeMode::WholeFileReview => self.whole_reviewed,
            MergeMode::Blockwise => self
                .blocks
                .iter()
                .all(|block| block.decision == BlockDecision::Reviewed),
        }
    }
}

fn rope_line_map(before: RopeSlice<'_>, after: RopeSlice<'_>) -> Vec<usize> {
    let mut input = InternedInput {
        before: Vec::new(),
        after: Vec::new(),
        interner: imara_diff::Interner::new(before.len_lines() + after.len_lines()),
    };
    input.update_before(before.lines());
    input.update_after(after.lines());
    let mut map = map_input(&input);
    for line in &mut map[..before.len_lines()] {
        *line = (*line).min(after.len_lines() - 1);
    }
    map
}

fn map_input<T>(input: &InternedInput<T>) -> Vec<usize> {
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let mut map = vec![0; input.before.len() + 1];
    let (mut left, mut right) = (0, 0);
    for hunk in diff.hunks() {
        let start = hunk.before.start as usize;
        let end = hunk.before.end as usize;
        while left < start {
            map[left] = right;
            left += 1;
            right += 1;
        }
        let new_start = hunk.after.start as usize;
        let new_end = hunk.after.end as usize;
        for (offset, line) in map[start..end].iter_mut().enumerate() {
            *line = new_start + offset.min(new_end.saturating_sub(new_start));
        }
        left = end;
        right = new_end;
    }
    while left < map.len() {
        map[left] = right;
        left += 1;
        right += 1;
    }
    map
}

#[cfg(feature = "git")]
pub(crate) fn marker_size(sources: &MergeSources) -> usize {
    [&sources.base, &sources.current, &sources.incoming]
        .into_iter()
        .flat_map(|source| source.lines())
        .map(|line| {
            let mut chars = line.chars();
            let Some(first @ ('<' | '|' | '=' | '>')) = chars.next() else {
                return 0;
            };
            1 + chars.take_while(|c| *c == first).count()
        })
        .max()
        .unwrap_or(0)
        .max(7)
        + 1
}

#[cfg(any(feature = "git", test))]
#[derive(Debug)]
enum Segment<'a> {
    Clean(&'a str),
    Conflict([&'a str; 3]),
}

#[cfg(any(feature = "git", test))]
fn parse_generated(text: &str, size: usize) -> Result<Vec<Segment<'_>>> {
    let open = format!("{} helix-current\n", "<".repeat(size));
    let base = format!("{} helix-base\n", "|".repeat(size));
    let middle = format!("{}\n", "=".repeat(size));
    let end = format!("{} helix-incoming\n", ">".repeat(size));
    let mut segments = Vec::new();
    let mut remaining = text;
    while let Some(start) = find_line(remaining, &open) {
        segments.push(Segment::Clean(&remaining[..start]));
        remaining = &remaining[start + open.len()..];
        let base_start = find_line(remaining, &base).context("Missing generated base marker")?;
        let current = &remaining[..base_start];
        remaining = &remaining[base_start + base.len()..];
        let middle_start = find_line(remaining, &middle).context("Missing generated separator")?;
        let ancestor = &remaining[..middle_start];
        remaining = &remaining[middle_start + middle.len()..];
        let end_start = find_line(remaining, &end).context("Missing generated end marker")?;
        let incoming = &remaining[..end_start];
        remaining = &remaining[end_start + end.len()..];
        segments.push(Segment::Conflict([ancestor, current, incoming]));
    }
    segments.push(Segment::Clean(remaining));
    Ok(segments)
}

#[cfg(any(feature = "git", test))]
fn find_line(text: &str, needle: &str) -> Option<usize> {
    text.match_indices(needle)
        .find_map(|(at, _)| (at == 0 || text.as_bytes()[at - 1] == b'\n').then_some(at))
}

/// Consume only the expected marker positions. Literal marker lines in source
/// bodies are compared byte-for-byte, never interpreted as conflict structure.
#[cfg(any(feature = "git", test))]
fn marker(text: &mut &str, character: char, size: Option<usize>, label: bool) -> Option<usize> {
    let (line, rest) = text.split_once('\n')?;
    let count = line.chars().take_while(|c| *c == character).count();
    if count < 7 || size.is_some_and(|size| size != count) {
        return None;
    }
    let suffix = &line[count..];
    if !(suffix.is_empty() || label && suffix.starts_with(' ')) {
        return None;
    }
    *text = rest;
    Some(count)
}

#[cfg(any(feature = "git", test))]
fn equivalent_working(segments: &[Segment<'_>], mut working: &str) -> bool {
    for segment in segments {
        match segment {
            Segment::Clean(text) => {
                let Some(rest) = working.strip_prefix(text) else {
                    return false;
                };
                working = rest;
            }
            Segment::Conflict([base, current, incoming]) => {
                let Some(size) = marker(&mut working, '<', None, true) else {
                    return false;
                };
                let Some(rest) = working.strip_prefix(current) else {
                    return false;
                };
                working = rest;
                if marker(&mut working, '|', Some(size), true).is_some() {
                    let Some(rest) = working.strip_prefix(base) else {
                        return false;
                    };
                    working = rest;
                }
                if marker(&mut working, '=', Some(size), false).is_none() {
                    return false;
                }
                let Some(rest) = working.strip_prefix(incoming) else {
                    return false;
                };
                working = rest;
                if marker(&mut working, '>', Some(size), true).is_none() {
                    return false;
                }
            }
        }
    }
    working.is_empty()
}

/// Git's line boundaries always use LF, even with Helix's unicode-lines feature.
#[cfg(any(feature = "git", test))]
fn lf_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries = vec![0];
    boundaries.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    if !text.is_empty() && !text.ends_with('\n') {
        boundaries.push(text.len());
    }
    boundaries
}

#[cfg(any(feature = "git", test))]
pub(crate) fn candidate_from_output(
    sources: MergeSources,
    text: &str,
    size: usize,
    working: Option<&str>,
) -> Result<MergeCandidate> {
    let segments = parse_generated(text, size)?;
    let mut projections = [String::new(), String::new(), String::new()];
    let mut projected_ranges = Vec::new();
    for segment in &segments {
        match segment {
            Segment::Clean(clean) => {
                for projection in &mut projections {
                    projection.push_str(clean);
                }
            }
            Segment::Conflict(parts) => {
                let mut ranges = [0..0, 0..0, 0..0];
                for side in 0..3 {
                    let start = projections[side].len();
                    projections[side].push_str(parts[side]);
                    ranges[side] = start..projections[side].len();
                }
                projected_ranges.push(ranges);
            }
        }
    }
    let ropes = [&sources.base, &sources.current, &sources.incoming];
    let source_text = ropes.map(|rope| rope.to_string());
    let maps: [_; 3] = std::array::from_fn(|side| {
        map_input(&InternedInput::new(
            projections[side].as_str(),
            source_text[side].as_str(),
        ))
    });
    let projection_boundaries = projections.each_ref().map(|text| lf_boundaries(text));
    let source_boundaries = source_text.each_ref().map(|text| lf_boundaries(text));
    let mut ranges = Vec::new();
    let mut mapped = true;
    for projected in &projected_ranges {
        let mut block_ranges = [0..0, 0..0, 0..0];
        for side in 0..3 {
            let range = &projected[side];
            let start_line = projection_boundaries[side]
                .binary_search(&range.start)
                .ok()
                .context("Invalid projected conflict start")?;
            let end_line = projection_boundaries[side]
                .binary_search(&range.end)
                .ok()
                .context("Invalid projected conflict end")?;
            let start_byte = source_boundaries[side]
                [maps[side][start_line].min(source_boundaries[side].len() - 1)];
            let end_byte = source_boundaries[side]
                [maps[side][end_line].min(source_boundaries[side].len() - 1)];
            let start = ropes[side].byte_to_char(start_byte);
            let end = ropes[side].byte_to_char(end_byte);
            let body = &projections[side][range.clone()];
            let actual = ropes[side].slice(start..end).to_string();
            // Git inserts a newline before EOF markers even if that source had none.
            if actual != body
                && !(end == ropes[side].len_chars()
                    && !actual.ends_with('\n')
                    && body.strip_suffix('\n') == Some(actual.as_str()))
            {
                mapped = false;
            }
            block_ranges[side] = start..end;
        }
        ranges.push(block_ranges);
    }
    let mut result = String::new();
    let mut result_chars = 0;
    let mut blocks = Vec::new();
    let mut conflict = 0;
    for segment in &segments {
        match segment {
            Segment::Clean(clean) => {
                result.push_str(clean);
                result_chars += clean.chars().count();
            }
            Segment::Conflict(parts) => {
                let [base, current, incoming] = ranges[conflict].clone();
                conflict += 1;
                let start = result_chars;
                if mapped {
                    let text = sources.base.slice(base.clone());
                    result.push_str(&text.to_string());
                    result_chars += text.len_chars();
                } else {
                    result.push_str(parts[0]);
                    result_chars += parts[0].chars().count();
                }
                blocks.push(ConflictBlock {
                    base,
                    current,
                    incoming,
                    result: start..result_chars,
                    decision: BlockDecision::Unresolved,
                });
            }
        }
    }
    let result = Rope::from(result);
    MergeTextFormat {
        bom: false,
        crlf: false,
    }
    .encode(result.slice(..))?;
    Ok(MergeCandidate {
        matches_working: mapped
            && working.is_some_and(|working| equivalent_working(&segments, working)),
        result,
        sources,
        blocks,
        working: working.map(Rope::from),
    })
}

#[cfg(test)]
mod test;
