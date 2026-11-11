//! Function-scoped reference and author annotations.

use std::{
    collections::HashSet,
    ops::Range,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use helix_core::{line_ending::line_end_char_index, text_annotations::InlineAnnotation, Rope};
use helix_event::{runtime_local, TaskController};
use helix_lsp::{
    lsp,
    util::{lsp_pos_to_pos, pos_to_lsp_pos},
    LanguageServerId, OffsetEncoding,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct FunctionHintsConfig {
    pub references: bool,
    pub git_author: bool,
}

impl FunctionHintsConfig {
    pub fn enabled(self) -> bool {
        self.references || self.git_author
    }
}

runtime_local! {
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    static INVALIDATED: tokio::sync::Notify = tokio::sync::Notify::const_new();
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

pub fn invalidate_generation() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
    INVALIDATED.notify_waiters();
}

pub async fn wait_for_invalidation(request_generation: u64) {
    let notified = INVALIDATED.notified();
    tokio::pin!(notified);
    // Register before checking the counter to avoid losing a cross-file edit
    // between checking the snapshot and waiting for the next notification.
    notified.as_mut().enable();
    if generation() == request_generation {
        notified.await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionHintsRequest {
    pub version: i32,
    pub generation: u64,
    pub visible: Range<usize>,
    pub path: PathBuf,
    pub symbol_server: LanguageServerId,
    pub reference_server: Option<LanguageServerId>,
    pub config: FunctionHintsConfig,
}

#[derive(Debug)]
pub struct FunctionHints {
    pub request: FunctionHintsRequest,
    pub requested_at: Instant,
    pub controller: TaskController,
    pub references: Vec<InlineAnnotation>,
    pub authors: Vec<InlineAnnotation>,
}

#[derive(Debug, Clone)]
pub struct FunctionTarget {
    pub char_idx: usize,
    pub selection: lsp::Range,
    /// Flat SymbolInformation does not guarantee a full definition range.
    pub lines: Option<Range<usize>>,
}

pub fn function_targets(
    text: &Rope,
    symbols: lsp::DocumentSymbolResponse,
    uri: &lsp::Url,
    encoding: OffsetEncoding,
    visible: Range<usize>,
) -> Vec<FunctionTarget> {
    let mut candidates = Vec::new();
    match symbols {
        lsp::DocumentSymbolResponse::Nested(symbols) => {
            // Use an explicit stack: servers can send arbitrarily nested symbols.
            let mut stack = symbols;
            while let Some(symbol) = stack.pop() {
                candidates.push((symbol.kind, symbol.range, symbol.selection_range, true));
                stack.extend(symbol.children.into_iter().flatten());
            }
        }
        lsp::DocumentSymbolResponse::Flat(symbols) => {
            candidates.extend(
                symbols
                    .into_iter()
                    .filter(|s| s.location.uri == *uri)
                    .filter_map(|s| {
                        let selection = flat_selection(text, s.location.range, &s.name, encoding)?;
                        Some((s.kind, s.location.range, selection, false))
                    }),
            );
        }
    }
    let mut targets: Vec<_> = candidates
        .into_iter()
        .filter_map(|(kind, range, selection, full_range)| {
            if !matches!(
                kind,
                lsp::SymbolKind::FUNCTION | lsp::SymbolKind::METHOD | lsp::SymbolKind::CONSTRUCTOR
            ) || !visible.contains(&(selection.start.line as usize))
                || range.start > selection.start
                || selection.start >= selection.end
                || selection.end > range.end
                || range.end.line as usize >= text.len_lines()
            {
                return None;
            }
            let start = lsp_pos_to_pos(text, selection.start, encoding)?;
            let end = lsp_pos_to_pos(text, selection.end, encoding)?;
            // The normal LSP converter clamps bad positions. Do not attach metadata
            // to a different function when a server sends an invalid symbol range.
            if pos_to_lsp_pos(text, start, encoding) != selection.start
                || pos_to_lsp_pos(text, end, encoding) != selection.end
            {
                return None;
            }
            let line = selection.start.line as usize;
            let end_line = range.end.line as usize + usize::from(range.end.character != 0);
            Some(FunctionTarget {
                char_idx: line_end_char_index(&text.slice(..), line),
                selection,
                lines: full_range.then_some(range.start.line as usize..end_line.max(line + 1)),
            })
        })
        .collect();
    targets.sort_by_key(|target| (target.char_idx, target.selection.start));
    targets.dedup_by_key(|target| target.selection);
    targets
}

fn flat_selection(
    text: &Rope,
    range: lsp::Range,
    name: &str,
    encoding: OffsetEncoding,
) -> Option<lsp::Range> {
    if name.is_empty() || range.start >= range.end || range.start.line as usize >= text.len_lines()
    {
        return None;
    }
    let start = lsp_pos_to_pos(text, range.start, encoding)?;
    let end = lsp_pos_to_pos(text, range.end, encoding)?;
    if start > end {
        return None;
    }
    // Some servers point at the whole declaration, others at its name. Resolve
    // the name before asking for references; querying a `def`/`fn` keyword would
    // otherwise misleadingly yield zero. Bound work for unusual server output.
    let declaration = text.slice(start..end.min(start + 4096)).to_string();
    let (byte, _) = declaration.match_indices(name).find(|(byte, _)| {
        !declaration[..*byte]
            .chars()
            .next_back()
            .is_some_and(helix_core::chars::char_is_word)
            && !declaration[*byte + name.len()..]
                .chars()
                .next()
                .is_some_and(helix_core::chars::char_is_word)
    })?;
    let name_start = start + declaration[..byte].chars().count();
    let name_end = name_start + name.chars().count();
    Some(lsp::Range::new(
        pos_to_lsp_pos(text, name_start, encoding),
        pos_to_lsp_pos(text, name_end, encoding),
    ))
}

pub fn reference_count(
    locations: &[lsp::Location],
    uri: &lsp::Url,
    declaration: lsp::Range,
) -> usize {
    locations
        .iter()
        .filter(|location| {
            location.uri != *uri
                || location.range.start < declaration.start
                || location.range.start >= declaration.end
        })
        .collect::<HashSet<_>>()
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        editor::{Config, GutterConfig},
        Document, View,
    };
    use arc_swap::ArcSwap;
    use helix_core::{
        doc_formatter::{DocumentFormatter, TextFormat},
        syntax, Transaction,
    };
    use std::sync::Arc;

    static GENERATION_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn invalidation_wakes_inflight_work_in_other_documents() {
        let _guard = GENERATION_TEST.lock().await;
        let pending = wait_for_invalidation(generation());
        tokio::pin!(pending);
        assert!(futures_util::poll!(&mut pending).is_pending());
        invalidate_generation();
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut pending)
            .await
            .expect("cross-file changes must cancel obsolete searches without waiting for idle");
    }

    fn rendered_hints(doc: &Document, view: &View) -> String {
        let annotations = view.text_annotations(doc, None);
        DocumentFormatter::new_at_prev_checkpoint(
            doc.text().slice(..),
            &TextFormat::default(),
            &annotations,
            0,
        )
        .filter(|grapheme| grapheme.is_virtual())
        .map(|grapheme| grapheme.raw.to_string())
        .collect()
    }

    #[tokio::test]
    async fn hints_are_virtual_independently_switchable_and_discarded_after_edit() {
        let _guard = GENERATION_TEST.lock().await;
        let config = Arc::new(ArcSwap::from_pointee(Config::default()));
        let mut enabled = Config::default();
        enabled.function_hints = FunctionHintsConfig {
            references: true,
            git_author: true,
        };
        config.store(Arc::new(enabled.clone()));
        let mut doc = Document::from(
            Rope::from_str("fn f() {}\n"),
            None,
            config.clone(),
            Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
        );
        let view = View::new(doc.id(), GutterConfig::default());
        doc.ensure_view_init(view.id);
        doc.function_hints.insert(
            view.id,
            FunctionHints {
                request: FunctionHintsRequest {
                    version: doc.version(),
                    generation: generation(),
                    visible: 0..2,
                    path: PathBuf::new(),
                    symbol_server: LanguageServerId::default(),
                    reference_server: None,
                    config: enabled.function_hints,
                },
                requested_at: Instant::now(),
                controller: TaskController::new(),
                references: vec![InlineAnnotation::new(9, "  2 usages")],
                authors: vec![InlineAnnotation::new(9, " · wangs")],
            },
        );
        assert_eq!(rendered_hints(&doc, &view), "  2 usages · wangs");
        assert_eq!(doc.text().to_string(), "fn f() {}\n");

        enabled.function_hints.references = false;
        config.store(Arc::new(enabled.clone()));
        assert_eq!(rendered_hints(&doc, &view), " · wangs");
        enabled.function_hints.references = true;
        enabled.function_hints.git_author = false;
        config.store(Arc::new(enabled.clone()));
        assert_eq!(rendered_hints(&doc, &view), "  2 usages");
        enabled.function_hints.git_author = true;
        config.store(Arc::new(enabled));
        invalidate_generation();
        assert_eq!(
            rendered_hints(&doc, &view),
            "",
            "a caller edit in another buffer invalidates displayed counts"
        );
        doc.function_hints
            .get_mut(&view.id)
            .unwrap()
            .request
            .generation = generation();
        let edit = Transaction::insert(doc.text(), doc.selection(view.id), "\n".into());
        doc.apply(&edit, view.id);
        assert_eq!(rendered_hints(&doc, &view), "");
    }

    fn range(start: (u32, u32), end: (u32, u32)) -> lsp::Range {
        lsp::Range::new(
            lsp::Position::new(start.0, start.1),
            lsp::Position::new(end.0, end.1),
        )
    }

    #[allow(deprecated)]
    fn symbol(
        kind: lsp::SymbolKind,
        lines: lsp::Range,
        selection: lsp::Range,
    ) -> lsp::DocumentSymbol {
        lsp::DocumentSymbol {
            name: "f".into(),
            detail: None,
            kind,
            tags: None,
            deprecated: None,
            range: lines,
            selection_range: selection,
            children: None,
        }
    }

    fn uri() -> lsp::Url {
        "file:///test.py".parse().unwrap()
    }

    #[test]
    fn nested_methods_are_visible_once_and_anchor_before_crlf() {
        let text = Rope::from_str(
            "class C:\r\n    def 方法(self):\r\n        pass\r\n\r\ndef hidden():\r\n    pass\r\n",
        );
        let method = symbol(
            lsp::SymbolKind::METHOD,
            range((1, 4), (3, 0)),
            range((1, 8), (1, 10)),
        );
        let mut class = symbol(
            lsp::SymbolKind::CLASS,
            range((0, 0), (3, 0)),
            range((0, 6), (0, 7)),
        );
        class.children = Some(vec![method.clone(), method]);
        let hidden = symbol(
            lsp::SymbolKind::FUNCTION,
            range((4, 0), (6, 0)),
            range((4, 4), (4, 10)),
        );
        let targets = function_targets(
            &text,
            lsp::DocumentSymbolResponse::Nested(vec![hidden, class]),
            &uri(),
            OffsetEncoding::Utf16,
            0..4,
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].char_idx, 27);
        assert_eq!(targets[0].selection.start, lsp::Position::new(1, 8));
        assert_eq!(targets[0].lines, Some(1..3));
    }

    #[test]
    fn utf16_name_positions_and_last_line_without_newline() {
        let text = Rope::from_str("/*😀*/ fn f() {}");
        let function = symbol(
            lsp::SymbolKind::FUNCTION,
            range((0, 0), (0, 16)),
            range((0, 10), (0, 11)),
        );
        let targets = function_targets(
            &text,
            lsp::DocumentSymbolResponse::Nested(vec![function]),
            &uri(),
            OffsetEncoding::Utf16,
            0..1,
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].char_idx, text.len_chars());
        assert_eq!(targets[0].lines, Some(0..1));
    }

    #[test]
    #[allow(deprecated)]
    fn flat_symbols_ignore_other_files_and_non_functions() {
        let text = Rope::from_str("def f():\n    pass\n");
        let function = lsp::SymbolInformation {
            name: "f".into(),
            kind: lsp::SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            location: lsp::Location::new(uri(), range((0, 4), (0, 5))),
            container_name: None,
        };
        let mut foreign = function.clone();
        foreign.location.uri = "file:///other.py".parse().unwrap();
        let mut variable = function.clone();
        variable.kind = lsp::SymbolKind::VARIABLE;
        let targets = function_targets(
            &text,
            lsp::DocumentSymbolResponse::Flat(vec![foreign, variable, function]),
            &uri(),
            OffsetEncoding::Utf8,
            0..2,
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].char_idx, 8);
    }

    #[test]
    #[allow(deprecated)]
    fn flat_definition_ranges_query_the_name_instead_of_the_keyword() {
        let text = Rope::from_str("def foo():\n    return foo()\n");
        let function = lsp::SymbolInformation {
            name: "foo".into(),
            kind: lsp::SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            location: lsp::Location::new(uri(), range((0, 0), (1, 16))),
            container_name: None,
        };
        let targets = function_targets(
            &text,
            lsp::DocumentSymbolResponse::Flat(vec![function]),
            &uri(),
            OffsetEncoding::Utf16,
            0..2,
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].selection, range((0, 4), (0, 7)));
        assert_eq!(
            targets[0].lines, None,
            "flat symbol locations cannot attribute full function history"
        );
    }

    #[test]
    fn malformed_symbols_are_ignored() {
        let text = Rope::from_str("fn f() {}\n");
        let symbols = vec![
            symbol(
                lsp::SymbolKind::FUNCTION,
                range((0, 0), (0, 9)),
                range((8, 0), (8, 1)),
            ),
            symbol(
                lsp::SymbolKind::FUNCTION,
                range((0, 0), (0, 9)),
                range((0, 90), (0, 91)),
            ),
            symbol(
                lsp::SymbolKind::FUNCTION,
                range((0, 8), (0, 1)),
                range((0, 3), (0, 4)),
            ),
        ];
        assert!(function_targets(
            &text,
            lsp::DocumentSymbolResponse::Nested(symbols),
            &uri(),
            OffsetEncoding::Utf16,
            0..100
        )
        .is_empty());
    }

    #[test]
    fn reference_count_excludes_declaration_and_duplicate_locations() {
        let declaration = range((0, 4), (0, 5));
        let call = lsp::Location::new(uri(), range((3, 0), (3, 1)));
        let other = lsp::Location::new("file:///other.py".parse().unwrap(), range((0, 4), (0, 5)));
        let locations = vec![
            lsp::Location::new(uri(), declaration),
            call.clone(),
            call,
            other,
        ];
        assert_eq!(reference_count(&locations, &uri(), declaration), 2);
        assert_eq!(reference_count(&[], &uri(), declaration), 0);
    }
}
