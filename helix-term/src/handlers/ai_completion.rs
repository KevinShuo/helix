mod client;
mod context;
mod prediction;
pub(crate) mod server;
mod validation;

use std::time::Duration;

use helix_core::text_annotations::InlineAnnotation;
use helix_event::{cancelable_future, register_hook};
use helix_view::{
    ai_completion::AiCompletion,
    document::Mode,
    editor::CompleteAction,
    events::{ConfigDidChange, DocumentDidChange, DocumentFocusLost, SelectionDidChange},
    Document, Editor,
};

use crate::{events::OnModeSwitch, job};

fn clear_document(doc: &mut Document) {
    doc.ai_completion_controller.cancel();
    doc.ai_completion = None;
}

pub fn clear(editor: &mut Editor) {
    let (_, doc) = current!(editor);
    clear_document(doc);
}

pub fn trigger(editor: &mut Editor) {
    clear(editor);
    let config = editor.config().ai_completion.clone();
    if !config.enabled
        || editor.mode() != Mode::Insert
        || !editor.macro_replaying.is_empty()
        || matches!(
            editor.last_completion,
            Some(CompleteAction::Selected { .. })
        )
    {
        return;
    }
    let (view, doc) = current!(editor);
    if doc.readonly || doc.active_snippet.is_some() {
        return;
    }
    let Some(cursor) = helix_view::ai_completion::completion_cursor(doc, view.id) else {
        return;
    };
    let snapshot = AiCompletion {
        view: view.id,
        version: doc.version(),
        annotation: InlineAnnotation::new(cursor, ""),
    };
    let doc_id = doc.id();
    let mut context = client::CompletionContext {
        filename: doc
            .path()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        language: doc.language_name().unwrap_or("").into(),
        indent_unit: doc.indent_style.as_str(),
        members: None,
        supporting: if config.conservative {
            context::source_notes(
                doc.text().slice(..),
                doc.syntax(),
                cursor,
                config.prefix_chars,
            )
        } else {
            String::new()
        },
        prefix: doc
            .text()
            .slice(cursor.saturating_sub(config.prefix_chars.min(64_000))..cursor)
            .to_string(),
        suffix: doc
            .text()
            .slice(cursor..(cursor + config.suffix_chars.min(16_000)).min(doc.text().len_chars()))
            .to_string(),
    };
    if !client::should_request(&config, &context) {
        return;
    }
    let predicting = config.predict_next && prediction::eligible(&context);
    let syntax_snapshot = (config.conservative || predicting)
        .then(|| {
            doc.syntax()
                .map(|syntax| (doc.text().clone(), syntax.root_language()))
        })
        .flatten()
        .filter(|(source, _)| source.len_bytes() <= validation::MAX_SOURCE_BYTES);
    let completion_server =
        if config.conservative && context::member_stem(&context.prefix).is_some() {
            doc.language_servers_with_feature(
                helix_core::syntax::config::LanguageServerFeature::Completion,
            )
            .next()
            .map(|server| {
                (
                    server.id(),
                    helix_lsp::lsp::CompletionParams {
                        text_document_position: helix_lsp::lsp::TextDocumentPositionParams {
                            text_document: doc.identifier(),
                            position: helix_lsp::util::pos_to_lsp_pos(
                                doc.text(),
                                cursor,
                                server.offset_encoding(),
                            ),
                        },
                        context: Some(helix_lsp::lsp::CompletionContext {
                            trigger_kind: helix_lsp::lsp::CompletionTriggerKind::INVOKED,
                            trigger_character: None,
                        }),
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    },
                )
            })
        } else {
            None
        };
    let handle = doc.ai_completion_controller.restart();
    let syntax_loader = editor.syn_loader.load_full();
    let completion_server = completion_server.and_then(|(id, params)| {
        editor
            .language_servers
            .get_by_id(id)
            .cloned()
            .map(|server| (server, params))
    });
    tokio::spawn(async move {
        let future = async {
            tokio::time::sleep(Duration::from_millis(config.debounce_ms.min(5000))).await;
            if let Some((server, params)) = completion_server {
                let budget = (config.prefix_chars.min(64_000) / 4).min(600).min(
                    context::MAX_SUPPORTING_CHARS
                        .saturating_sub(context.supporting.chars().count() + 2),
                );
                let stem = context::member_stem(&context.prefix).unwrap_or_default();
                if let Some(members) = context::fetch_members(server, params, stem, budget).await {
                    if !members.notes.is_empty() {
                        if !context.supporting.is_empty() {
                            context.supporting.push_str("\n\n");
                        }
                        context.supporting.push_str(&members.notes);
                    }
                    context.members = members.names;
                }
            }
            server::wait_ready(&config).await?;
            let Some(text) = client::complete(&config, &context).await? else {
                return Ok(None);
            };
            if let Some((source, language)) = syntax_snapshot {
                let proposed = text.clone();
                let valid = tokio::task::spawn_blocking(move || {
                    validation::check(
                        source,
                        cursor,
                        &proposed,
                        language,
                        &syntax_loader,
                        predicting,
                    )
                })
                .await
                .unwrap_or(false);
                if !valid {
                    return Ok(None);
                }
            }
            Ok::<_, anyhow::Error>(Some(text))
        };
        let Some(result) = cancelable_future(future, &handle).await else {
            return;
        };
        let text = match result {
            Ok(Some(text)) => text,
            Ok(None) => return,
            Err(error) => {
                // Automatic requests must not interrupt editing or include response bodies/keys.
                log::debug!("AI completion: {error}");
                return;
            }
        };
        job::dispatch(move |editor, _| {
            if handle.is_canceled()
                || editor.mode() != Mode::Insert
                || !editor.config().ai_completion.enabled
                || matches!(
                    editor.last_completion,
                    Some(CompleteAction::Selected { .. })
                )
            {
                return;
            }
            let (view, doc) = current!(editor);
            if doc.id() != doc_id || view.id != snapshot.view || !snapshot.is_valid(doc, view.id) {
                return;
            }
            let text = text
                .replace("\r\n", "\n")
                .replace('\n', doc.line_ending.as_str());
            doc.ai_completion = Some(AiCompletion {
                annotation: InlineAnnotation::new(snapshot.annotation.char_idx, text),
                ..snapshot
            });
        })
        .await;
    });
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        clear_document(event.doc);
        Ok(())
    });
    register_hook!(move |event: &mut SelectionDidChange<'_>| {
        clear_document(event.doc);
        Ok(())
    });
    register_hook!(move |event: &mut DocumentFocusLost<'_>| {
        if let Some(doc) = event.editor.document_mut(event.doc) {
            clear_document(doc);
        }
        Ok(())
    });
    register_hook!(move |event: &mut OnModeSwitch<'_, '_>| {
        if event.old_mode == Mode::Insert {
            clear(event.cx.editor);
        }
        Ok(())
    });
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        for doc in event.editor.documents.values_mut() {
            clear_document(doc);
        }
        Ok(())
    });
}
