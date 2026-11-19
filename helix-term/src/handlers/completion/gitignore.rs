use std::{borrow::Cow, ffi::OsStr, sync::Arc};

use helix_core::{
    completion::{CompletionProvider, IgnoreRuleCompletion},
    line_ending::line_without_line_ending,
    RopeSlice, Selection, Transaction,
};
use helix_event::TaskHandle;
use helix_loader::workspace_trust::{TrustQuery, WorkspaceTrust};
use helix_view::{
    document::SavePoint, handlers::completion::ResponseContext, Document, DocumentId, Editor,
    ViewId,
};

use super::{CompletionItem, CompletionItems, CompletionResponse};
use crate::gitignore::suggest::{suggest, Context};

pub(crate) fn enabled(doc: &Document) -> bool {
    doc.path().and_then(|p| p.file_name()) == Some(OsStr::new(".gitignore"))
}

pub(super) fn comment(text: RopeSlice, cursor: usize) -> bool {
    text.line(text.char_to_line(cursor)).get_char(0) == Some('#')
}

pub(super) fn trigger_char(prefix: RopeSlice) -> bool {
    if prefix == "!" {
        return true;
    }
    let mut chars = prefix.chars_at(prefix.len_chars()).reversed();
    match chars.next() {
        Some('/') => chars.take_while(|&ch| ch == '\\').count() % 2 == 0,
        Some('.') if chars.next() == Some('*') => {
            matches!(chars.next(), None | Some('/')) || prefix == "!*."
        }
        _ => false,
    }
}

pub(crate) fn filter(doc: &Document, cursor: usize) -> Option<String> {
    if !enabled(doc) {
        return None;
    }
    Some(context(doc.text().slice(..), cursor)?.filter)
}

fn context(text: RopeSlice, cursor: usize) -> Option<Context> {
    let line = text.char_to_line(cursor);
    let offset = cursor - text.line_to_char(line);
    Context::parse(&Cow::from(line_without_line_ending(&text, line)), offset)
}

struct Segments {
    context: Context,
    ranges: Vec<(usize, usize)>,
}

impl Segments {
    fn at(text: RopeSlice, selection: &Selection) -> Option<Self> {
        let cursor = selection.primary().cursor(text);
        let ctx = context(text, cursor)?;
        let mut ranges = Vec::with_capacity(selection.len());
        for range in selection {
            let pos = range.cursor(text);
            if pos != cursor && context(text, pos).as_ref() != Some(&ctx) {
                return None;
            }
            let offset = text.line_to_char(text.char_to_line(pos));
            ranges.push((offset + ctx.start, offset + ctx.end));
        }
        if ranges
            .windows(2)
            .any(|pair| pair[0] == pair[1] || pair[0].1 > pair[1].0)
        {
            return None;
        }
        Some(Self {
            context: ctx,
            ranges,
        })
    }

    fn replace(&self, text: &helix_core::Rope, replacement: &str) -> Transaction {
        Transaction::change(
            text,
            self.ranges
                .iter()
                .map(|&(start, end)| (start, end, Some(replacement.into()))),
        )
    }
}

pub(crate) fn repeat(editor: &mut Editor, replacement: &str) {
    let (view, doc) = current!(editor);
    if !enabled(doc) {
        return;
    }
    let text = doc.text();
    if let Some(segments) = Segments::at(text.slice(..), doc.selection(view.id)) {
        let replacement = segments.context.replay_segment(replacement);
        let transaction = segments.replace(text, replacement);
        doc.apply(&transaction, view.id);
    }
}

/// Offsets and rule effects refer to one exact buffer revision and cursor set.
#[derive(Clone)]
pub(super) struct Snapshot {
    doc: DocumentId,
    view: ViewId,
    version: i32,
    selection: Selection,
}

impl Snapshot {
    pub(super) fn valid(&self, doc: &Document, view: ViewId) -> bool {
        self.doc == doc.id()
            && self.view == view
            && self.version == doc.version()
            && &self.selection == doc.selection(view)
            && enabled(doc)
    }
}

pub(super) fn refresh(
    editor: &mut Editor,
    handle: TaskHandle,
) -> Option<(Snapshot, impl FnOnce() -> CompletionResponse)> {
    let (view, doc) = current!(editor);
    if !enabled(doc)
        || comment(
            doc.text().slice(..),
            doc.selection(view.id)
                .primary()
                .cursor(doc.text().slice(..)),
        )
    {
        return None;
    }
    let savepoint = doc.savepoint(view);
    request(
        doc,
        view.id,
        false,
        handle,
        editor.workspace_trust.clone(),
        savepoint,
    )
}

pub(super) fn request(
    doc: &Document,
    view: ViewId,
    manual: bool,
    handle: TaskHandle,
    trust: WorkspaceTrust,
    savepoint: Arc<SavePoint>,
) -> Option<(Snapshot, impl FnOnce() -> CompletionResponse)> {
    let base = doc.path()?.parent()?.to_owned();
    let text = doc.text().clone();
    let selection = doc.selection(view).clone();
    let snapshot = Snapshot {
        doc: doc.id(),
        view,
        version: doc.version(),
        selection: selection.clone(),
    };
    let paths = doc.path_completion_enabled();
    let compute = move || {
        let slice = text.slice(..);
        let cursor = selection.primary().cursor(slice);
        let line = slice.char_to_line(cursor);
        let start = slice.line_to_char(line);
        let end = start + slice.line(line).len_chars();
        let compute_items = || {
            if handle.is_canceled() {
                return None;
            }
            let segments = Segments::at(slice, &selection)?;
            let before = Cow::from(slice.slice(..start));
            let after = Cow::from(slice.slice(end..));
            let workspace = helix_loader::find_workspace_in(&base).0;
            let trust_full = trust.query(&workspace, TrustQuery::Git).is_trusted();
            let candidates = suggest(
                &base,
                &segments.context,
                &before,
                &after,
                paths,
                manual,
                trust_full,
                || handle.is_canceled(),
            );
            let items = candidates
                .into_iter()
                .map(|candidate| {
                    let transaction = segments.replace(&text, &candidate.replacement);
                    CompletionItem::Other(helix_core::CompletionItem {
                        transaction,
                        label: crate::ui::gitignore::display_text(&candidate.label).into(),
                        kind: "ignore".into(),
                        documentation: Some(crate::ui::gitignore::display_text(&candidate.detail)),
                        provider: CompletionProvider::Gitignore,
                        ignore_rule: Some(IgnoreRuleCompletion {
                            text: candidate.filter,
                            rank: candidate.rank,
                            repeat: segments.context.repeat_pattern(&candidate.replacement),
                        }),
                    })
                })
                .collect();
            Some(items)
        };
        CompletionResponse {
            items: CompletionItems::Other(compute_items().unwrap_or_default()),
            provider: CompletionProvider::Gitignore,
            context: ResponseContext {
                is_incomplete: false,
                priority: 1,
                savepoint,
            },
        }
    };
    Some((snapshot, compute))
}
