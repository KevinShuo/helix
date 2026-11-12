//! Function and class candidates use source coordinates, independent of view state.

use std::ops::Range;

use crate::{
    syntax::{Loader, QueryIterEvent, Syntax},
    RopeSlice,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldCandidate {
    /// Start of the declaration (including a decorator/template wrapper).
    pub declaration: usize,
    /// Source characters replaced by a single `…`, excluding closing braces.
    pub range: Range<usize>,
    pub function: Range<usize>,
}

impl FoldCandidate {
    /// The definition's first line, excluding decorator/template wrappers.
    pub fn header_line(&self, text: RopeSlice) -> usize {
        text.char_to_line(self.function.start)
    }
}

pub fn function_candidates(
    text: RopeSlice,
    syntax: &Syntax,
    loader: &Loader,
) -> Vec<FoldCandidate> {
    let mut candidates = Vec::new();
    let mut query = syntax.query_iter::<_, (), _>(text, |lang| loader.fold_query(lang), ..);
    while let Some(event) = query.next() {
        let QueryIterEvent::Match(mat) = event else {
            continue;
        };
        let Some(fold_query) = loader.fold_query(query.current_language()) else {
            continue;
        };
        if !["fold.function", "fold.class"]
            .into_iter()
            .any(|capture| fold_query.get_capture(capture) == Some(mat.capture))
        {
            continue;
        }
        let node = mat.node;
        if node.is_missing()
            || node
                .children()
                .any(|child| child.is_missing() || child.kind() == "ERROR")
        {
            continue;
        }
        let Some(body) = node.children().find(|child| {
            matches!(
                child.kind(),
                "block" | "compound_statement" | "field_declaration_list"
            )
        }) else {
            continue;
        };
        let (start, end) = if body.kind() == "block" {
            let Some(colon) = node.children().find(|child| child.kind() == ":") else {
                continue;
            };
            (colon.end_byte(), body.end_byte())
        } else {
            let mut children = body.children();
            let Some(open) = children.next().filter(|child| child.kind() == "{") else {
                continue;
            };
            let Some(close) = children
                .last()
                .filter(|child| child.kind() == "}" && !child.is_missing())
            else {
                continue;
            };
            (open.end_byte(), close.start_byte())
        };
        let start = text.byte_to_char(start as usize);
        let end = text.byte_to_char(end as usize);
        if start >= end
            || text.char_to_line(start) == text.char_to_line(end)
            || text.slice(start..end).chars().all(char::is_whitespace)
        {
            continue;
        }
        let function = text.byte_to_char(node.start_byte() as usize)
            ..text.byte_to_char(node.end_byte() as usize);
        let declaration = node
            .parent()
            .filter(|parent| {
                matches!(
                    parent.kind(),
                    "decorated_definition" | "template_declaration"
                )
            })
            .map_or(function.start, |parent| {
                text.byte_to_char(parent.start_byte() as usize)
            });
        candidates.push(FoldCandidate {
            declaration,
            range: start..end,
            function,
        });
    }
    candidates.sort_by_key(|candidate| {
        (
            candidate.range.start,
            std::cmp::Reverse(candidate.range.end),
        )
    });
    candidates.dedup_by(|a, b| a.range == b.range);
    // A query extension may produce crossing captures. Never hand them to layout.
    let mut ends = Vec::new();
    candidates.retain(|candidate| {
        while ends.last().is_some_and(|end| *end <= candidate.range.start) {
            ends.pop();
        }
        if ends.last().is_some_and(|end| *end < candidate.range.end) {
            return false;
        }
        ends.push(candidate.range.end);
        true
    });
    candidates
}
