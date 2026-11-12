//! Idle-time function metadata. Nothing in this module edits document text.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::{stream, StreamExt};
use helix_core::{syntax::config::LanguageServerFeature, text_annotations::InlineAnnotation, Rope};
use helix_event::{cancelable_future, register_hook, TaskController, TaskHandle};
use helix_loader::workspace_trust::TrustQuery;
use helix_lsp::{
    lsp,
    util::{lsp_pos_to_pos, pos_to_lsp_pos},
    Client,
};
use helix_view::{
    document::Mode,
    events::{
        ConfigDidChange, DocumentDidChange, DocumentDidClose, DocumentDidOpen,
        LanguageServerExited, LanguageServerInitialized,
    },
    function_hints::{
        function_targets, generation, invalidate_generation, reference_count,
        wait_for_invalidation, FunctionHints, FunctionHintsRequest, FunctionTarget,
    },
    Document, DocumentId, Editor, ViewId,
};

use crate::job;

const CACHE_TTL: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_VISIBLE_FUNCTIONS: usize = 64;

enum HintResult {
    References(Vec<InlineAnnotation>),
    Authors(Vec<InlineAnnotation>),
}

fn attach(
    doc: &mut Document,
    view: ViewId,
    request: &FunctionHintsRequest,
    generation: u64,
    result: HintResult,
) {
    if request.version != doc.version()
        || request.folds != doc.folds(view)
        || request.generation != generation
        || doc.path() != Some(request.path.as_path())
        || request.config != doc.config.load().function_hints
    {
        return;
    }
    let Some(hints) = doc
        .function_hints
        .get_mut(&view)
        .filter(|hints| hints.request == *request)
    else {
        return;
    };
    match result {
        HintResult::References(annotations) => hints.references = annotations,
        HintResult::Authors(annotations) => hints.authors = annotations,
    }
}

async fn publish(
    doc_id: DocumentId,
    view_id: ViewId,
    request: FunctionHintsRequest,
    handle: TaskHandle,
    result: HintResult,
) {
    job::dispatch(move |editor, _| {
        if handle.is_canceled()
            || !editor
                .tree
                .try_get(view_id)
                .is_some_and(|view| view.doc == doc_id)
        {
            return;
        }
        if let Some(doc) = editor.document_mut(doc_id) {
            attach(doc, view_id, &request, generation(), result);
        }
    })
    .await;
}

struct PreparedRequest {
    key: FunctionHintsRequest,
    symbols: Arc<Client>,
    references: Option<Arc<Client>>,
    identifier: lsp::TextDocumentIdentifier,
    text: Rope,
    trust_git: bool,
}

fn prepare(editor: &Editor, view_id: ViewId) -> Option<PreparedRequest> {
    let view = editor.tree.try_get(view_id)?;
    let doc = editor.document(view.doc)?;
    let path = doc.path()?.to_path_buf();
    let symbols = doc
        .language_servers_with_feature(LanguageServerFeature::DocumentSymbols)
        .next()?;
    let references = doc
        .language_servers_with_feature(LanguageServerFeature::GotoReference)
        .find(|server| server.id() == symbols.id())
        .or_else(|| {
            doc.language_servers_with_feature(LanguageServerFeature::GotoReference)
                .next()
        });
    let start = doc
        .text()
        .char_to_line(doc.view_offset(view_id).anchor.min(doc.text().len_chars()));
    Some(PreparedRequest {
        key: FunctionHintsRequest {
            version: doc.version(),
            generation: generation(),
            visible: start..view.estimate_last_doc_line(doc) + 1,
            folds: doc.folds(view_id).to_vec(),
            path,
            symbol_server: symbols.id(),
            reference_server: references.map(Client::id),
            config: editor.config().function_hints,
        },
        symbols: editor.language_servers.get_by_id(symbols.id())?.clone(),
        references: references
            .and_then(|server| editor.language_servers.get_by_id(server.id()))
            .cloned(),
        identifier: doc.identifier(),
        text: doc.text().clone(),
        trust_git: editor
            .workspace_trust
            .query(doc.workspace_root(), TrustQuery::Git)
            .is_trusted(),
    })
}

pub(crate) fn request_visible(editor: &mut Editor) {
    if !editor.config().function_hints.enabled() || editor.mode() == Mode::Insert {
        return;
    }
    let views: Vec<_> = editor
        .tree
        .views()
        .map(|(view, _)| (view.id, view.doc))
        .collect();
    let mut refresh = false;
    for (view_id, doc_id) in views {
        let Some(prepared) = prepare(editor, view_id) else {
            if let Some(doc) = editor.document_mut(doc_id) {
                doc.function_hints.remove(&view_id);
            }
            continue;
        };
        refresh = true;
        let doc = editor.document_mut(doc_id).unwrap();
        if doc.function_hints.get(&view_id).is_some_and(|hints| {
            hints.request == prepared.key
                && (hints.controller.is_running() || hints.requested_at.elapsed() < CACHE_TTL)
        }) {
            continue;
        }
        let mut controller = TaskController::new();
        let handle = controller.restart();
        doc.function_hints.insert(
            view_id,
            FunctionHints {
                request: prepared.key.clone(),
                requested_at: Instant::now(),
                controller,
                references: Vec::new(),
                authors: Vec::new(),
            },
        );
        tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = wait_for_invalidation(prepared.key.generation) => {},
                _ = cancelable_future(request(prepared, doc_id, view_id, &handle), &handle) => {},
            }
        });
    }
    if refresh {
        // The application's idle event disarms this timer before calling us.
        // Rearm at the cache interval so external commits also appear without a keypress.
        editor
            .idle_timer
            .as_mut()
            .reset(tokio::time::Instant::now() + CACHE_TTL);
    }
}

async fn request(prepared: PreparedRequest, doc: DocumentId, view: ViewId, handle: &TaskHandle) {
    let future = prepared
        .symbols
        .call_cancellable::<lsp::request::DocumentSymbolRequest>(lsp::DocumentSymbolParams {
            text_document: prepared.identifier.clone(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });
    let response = match tokio::time::timeout(REQUEST_TIMEOUT, future).await {
        Ok(Ok(Some(response))) => response,
        Ok(Ok(None)) => return,
        error => {
            log::debug!("Function hints: document symbols unavailable: {error:?}");
            return;
        }
    };
    let mut targets = function_targets(
        &prepared.text,
        response,
        &prepared.identifier.uri,
        prepared.symbols.offset_encoding(),
        prepared.key.visible.clone(),
    );
    targets.retain(|target| {
        lsp_pos_to_pos(
            &prepared.text,
            target.selection.start,
            prepared.symbols.offset_encoding(),
        )
        .is_some_and(|pos| !prepared.key.folds.iter().any(|fold| fold.contains(&pos)))
    });
    targets.truncate(MAX_VISIBLE_FUNCTIONS);
    if targets.is_empty() {
        return;
    }
    let references = async {
        if !prepared.key.config.references {
            return;
        }
        let Some(server) = &prepared.references else {
            return;
        };
        let annotations = stream::iter(targets.clone())
            .map(|target| {
                let prepared = &prepared;
                async move { reference_annotation(prepared, server, &target).await }
            })
            .buffered(4)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .flatten()
            .collect();
        publish(
            doc,
            view,
            prepared.key.clone(),
            handle.clone(),
            HintResult::References(annotations),
        )
        .await;
    };
    let authors = async {
        if !prepared.key.config.git_author || !prepared.trust_git {
            return;
        }
        let git_targets: Vec<_> = targets
            .iter()
            .filter(|target| target.lines.is_some())
            .collect();
        if git_targets.is_empty() {
            return;
        }
        let ranges = git_targets
            .iter()
            .filter_map(|target| target.lines.clone())
            .collect();
        let authors = match helix_vcs::function_authors(
            prepared.key.path.clone(),
            prepared.text.to_string(),
            ranges,
            prepared.trust_git,
        )
        .await
        {
            Ok(authors) => authors,
            Err(error) => {
                log::debug!("Function hints: Git author unavailable: {error}");
                return;
            }
        };
        let annotations = git_targets
            .iter()
            .zip(authors)
            .filter_map(|(target, author)| {
                let author = author?;
                // Commit metadata is arbitrary text: keep it on one display line.
                let name: String = author
                    .author
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(80)
                    .collect();
                if name.trim().is_empty() {
                    return None;
                }
                let modified = if author.modified { "*" } else { "" };
                Some(InlineAnnotation::new(
                    target.char_idx,
                    format!("  {}{modified}", name.trim()),
                ))
            })
            .collect();
        publish(
            doc,
            view,
            prepared.key.clone(),
            handle.clone(),
            HintResult::Authors(annotations),
        )
        .await;
    };
    tokio::join!(references, authors);
}

async fn reference_annotation(
    prepared: &PreparedRequest,
    server: &Client,
    target: &FunctionTarget,
) -> Option<InlineAnnotation> {
    // Symbols and references can come from different servers/offset encodings.
    let convert = |position| {
        lsp_pos_to_pos(&prepared.text, position, prepared.symbols.offset_encoding())
            .map(|pos| pos_to_lsp_pos(&prepared.text, pos, server.offset_encoding()))
    };
    let declaration = lsp::Range::new(
        convert(target.selection.start)?,
        convert(target.selection.end)?,
    );
    let future = server.call_cancellable::<lsp::request::References>(lsp::ReferenceParams {
        text_document_position: lsp::TextDocumentPositionParams {
            text_document: prepared.identifier.clone(),
            position: declaration.start,
        },
        context: lsp::ReferenceContext {
            include_declaration: false,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });
    let locations = match tokio::time::timeout(REQUEST_TIMEOUT, future).await {
        Ok(Ok(locations)) => locations.unwrap_or_default(),
        error => {
            log::debug!("Function hints: references unavailable: {error:?}");
            return None;
        }
    };
    let count = reference_count(&locations, &prepared.identifier.uri, declaration);
    let label = if count == 1 { "usage" } else { "usages" };
    Some(InlineAnnotation::new(
        target.char_idx,
        format!("  {count} {label}"),
    ))
}

fn invalidate(editor: &mut Editor) {
    invalidate_generation();
    for doc in editor.documents.values_mut() {
        doc.function_hints.clear();
    }
    editor.reset_idle_timer();
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        event.doc.function_hints.clear();
        if !event.ghost_transaction {
            invalidate_generation();
        }
        Ok(())
    });
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        invalidate(event.editor);
        Ok(())
    });
    register_hook!(move |event: &mut LanguageServerInitialized<'_>| {
        invalidate(event.editor);
        Ok(())
    });
    register_hook!(move |event: &mut LanguageServerExited<'_>| {
        invalidate(event.editor);
        Ok(())
    });
    register_hook!(move |_event: &mut DocumentDidOpen<'_>| {
        invalidate_generation();
        Ok(())
    });
    register_hook!(move |_event: &mut DocumentDidClose<'_>| {
        invalidate_generation();
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use arc_swap::ArcSwap;
    use helix_core::{syntax, Rope, Transaction};
    use helix_event::TaskController;
    use helix_lsp::LanguageServerId;
    use helix_view::{
        editor::{Config, GutterConfig},
        function_hints::{FunctionHints, FunctionHintsConfig},
        View,
    };
    use std::{sync::Arc, time::Instant};

    fn setup() -> (Document, View, FunctionHintsRequest) {
        let mut config = Config::default();
        config.function_hints = FunctionHintsConfig {
            references: true,
            git_author: true,
        };
        let mut doc = Document::from(
            Rope::from_str("fn f() {}\n"),
            None,
            Arc::new(ArcSwap::from_pointee(config)),
            Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
        );
        doc.set_path(Some(&std::env::temp_dir().join("helix-function-hints.rs")));
        let view = View::new(doc.id(), GutterConfig::default());
        doc.ensure_view_init(view.id);
        let request = FunctionHintsRequest {
            version: doc.version(),
            generation: 7,
            visible: 0..2,
            folds: Vec::new(),
            path: doc.path().unwrap().to_path_buf(),
            symbol_server: LanguageServerId::default(),
            reference_server: None,
            config: doc.config.load().function_hints,
        };
        doc.function_hints.insert(
            view.id,
            FunctionHints {
                request: request.clone(),
                requested_at: Instant::now(),
                controller: TaskController::new(),
                references: vec![],
                authors: vec![],
            },
        );
        (doc, view, request)
    }

    fn references() -> HintResult {
        HintResult::References(vec![InlineAnnotation::new(9, "  2 usages")])
    }

    #[test]
    fn reference_and_author_results_arrive_independently() {
        let (mut doc, view, request) = setup();
        attach(&mut doc, view.id, &request, 7, references());
        assert_eq!(
            doc.function_hints[&view.id].references[0].text.as_str(),
            "  2 usages"
        );
        attach(
            &mut doc,
            view.id,
            &request,
            7,
            HintResult::Authors(vec![InlineAnnotation::new(9, "  wangs*")]),
        );
        assert_eq!(
            doc.function_hints[&view.id].authors[0].text.as_str(),
            "  wangs*"
        );
        assert_eq!(doc.function_hints[&view.id].references.len(), 1);
        assert_eq!(doc.text().to_string(), "fn f() {}\n");
    }

    #[test]
    fn late_results_are_rejected_after_cross_file_change_or_scroll() {
        let (mut doc, view, request) = setup();
        attach(&mut doc, view.id, &request, 8, references());
        assert!(doc.function_hints[&view.id].references.is_empty());
        doc.function_hints
            .get_mut(&view.id)
            .unwrap()
            .request
            .visible = 10..20;
        attach(&mut doc, view.id, &request, 7, references());
        assert!(doc.function_hints[&view.id].references.is_empty());
    }

    #[test]
    fn late_results_cannot_revive_hints_after_edit_or_view_close() {
        let (mut doc, view, request) = setup();
        let edit = Transaction::insert(doc.text(), doc.selection(view.id), "\n".into());
        doc.apply(&edit, view.id);
        attach(&mut doc, view.id, &request, 7, references());
        assert!(doc
            .function_hints
            .get(&view.id)
            .is_none_or(|h| h.references.is_empty()));
        doc.remove_view(view.id);
        attach(&mut doc, view.id, &request, 7, references());
        assert!(!doc.function_hints.contains_key(&view.id));
    }

    #[test]
    fn late_results_cannot_attach_to_renamed_document() {
        let (mut doc, view, request) = setup();
        doc.set_path(Some(
            &std::env::temp_dir().join("renamed-function-hints.rs"),
        ));
        attach(&mut doc, view.id, &request, 7, references());
        assert!(!doc.function_hints.contains_key(&view.id));
    }
}
