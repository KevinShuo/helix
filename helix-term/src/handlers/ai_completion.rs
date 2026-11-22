mod client;
mod context;
mod prediction;
pub(crate) mod server;
mod validation;

use std::time::Instant;

use helix_core::text_annotations::InlineAnnotation;
use helix_event::{cancelable_future, register_hook};
use helix_view::{
    ai_completion::{AiCompletion, AnticipatedEdit, CachedAiCompletion},
    document::Mode,
    editor::CompleteAction,
    events::{ConfigDidChange, DocumentDidChange, DocumentFocusLost, SelectionDidChange},
    Document, Editor,
};

use crate::{events::OnModeSwitch, job};

fn clear_document(doc: &mut Document) {
    doc.ai_completion_controller.cancel();
    doc.ai_anticipated_edit = None;
    doc.ai_completion = None;
}

pub fn clear(editor: &mut Editor) {
    let (_, doc) = current!(editor);
    clear_document(doc);
}

pub fn trigger(editor: &mut Editor) {
    request(editor, false);
}

pub fn on_insert(editor: &mut Editor) {
    request(editor, true);
}

fn request(editor: &mut Editor, entering: bool) {
    let started = Instant::now();
    let config = editor.config().ai_completion.clone();
    if editor.ai_commit_running
        || super::ai::interactive_busy(editor)
        || editor.ai_merge.is_some()
        || super::ai_test::busy(editor)
        || editor.ai_doc.as_ref().is_some_and(|op| op.is_valid(editor))
        || !editor.config().ai.enabled
        || !config.enabled
        || editor.mode() != Mode::Insert
        || !editor.macro_replaying.is_empty()
        || matches!(
            editor.last_completion,
            Some(CompleteAction::Selected { .. })
        )
    {
        clear(editor);
        return;
    }
    let (view, doc) = current!(editor);
    // Entering insert mode does not break a typing pause.
    let previous_edit = if entering {
        None
    } else {
        doc.last_ai_completion_edit.replace(started)
    };
    let debounce = config.debounce_after(previous_edit.map(|last| started.duration_since(last)));
    if doc
        .ai_completion
        .as_ref()
        .is_some_and(|preview| preview.is_valid(doc, view.id))
    {
        return;
    }
    let model = match editor
        .config()
        .ai_provider(helix_view::ai::AiFeature::Completion)
    {
        Ok(model) => model,
        Err(error) => {
            clear(editor);
            log::debug!("AI completion: {error}");
            return;
        }
    };
    let (view, doc) = current!(editor);
    let compact_cpu =
        config.conservative && model.gpu_layers == 0 && model.is_local_completion_model();
    if entering && !compact_cpu {
        return;
    }
    if doc.readonly || doc.active_snippet.is_some() {
        clear_document(doc);
        return;
    }
    let Some(cursor) = helix_view::ai_completion::completion_cursor(doc, view.id) else {
        clear_document(doc);
        return;
    };
    let snapshot = AiCompletion {
        view: view.id,
        version: doc.version(),
        annotation: InlineAnnotation::new(cursor, ""),
        request_latency: None,
    };
    let doc_id = doc.id();
    if compact_cpu {
        if let Some(cached) = doc
            .ai_cached_completion
            .as_ref()
            .filter(|c| c.matches(doc, cursor))
        {
            doc.ai_completion_controller.cancel();
            doc.ai_anticipated_edit = None;
            doc.ai_completion = Some(AiCompletion {
                annotation: InlineAnnotation::new(cursor, cached.text.as_str()),
                request_latency: Some(started.elapsed()),
                ..snapshot
            });
            return;
        }
    }
    if doc.ai_completion_controller.is_running()
        && doc
            .ai_anticipated_edit
            .as_ref()
            .is_some_and(|edit| edit.is_valid(doc, view.id))
    {
        return;
    }
    clear_document(doc);
    let window = config.prefix_chars.min(64_000);
    let prefix_start = if compact_cpu {
        context::compact_prefix_start(
            doc.text().slice(..),
            doc.syntax(),
            cursor,
            window,
            config.suffix_chars,
        )
    } else {
        cursor.saturating_sub(window)
    };
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
        prefix: doc.text().slice(prefix_start..cursor).to_string(),
        suffix: doc
            .text()
            .slice(cursor..(cursor + config.suffix_chars.min(16_000)).min(doc.text().len_chars()))
            .to_string(),
    };
    let anticipated = compact_cpu
        .then(|| {
            prediction::anticipated_edit(
                &context,
                config.predict_next,
                entering,
                doc.line_ending.as_str(),
            )
        })
        .flatten()
        .filter(|edit| context.prefix.chars().count() + edit.chars().count() <= window);
    // A normal-mode cursor can land anywhere. Spend CPU on entry only when
    // we can carry this exact request across the anticipated edit.
    if entering && anticipated.is_none() {
        return;
    }
    if let Some(edit) = &anticipated {
        context.prefix.push_str(edit);
    }
    let blank_line = context
        .prefix
        .rsplit('\n')
        .next()
        .is_some_and(|s| s.trim().is_empty());
    let request_model = client::should_request(&config, &context);
    let variable = if compact_cpu && !context.suffix.starts_with(helix_core::chars::char_is_word) {
        context::variable_stem(
            &context.prefix,
            doc.syntax(),
            doc.text().char_to_byte(cursor) as u32,
        )
        .map(str::to_owned)
    } else {
        None
    };
    // A partial field before an existing semicolon may deserve an exact
    // language-server completion even when a model guess would be suppressed.
    if !request_model
        && variable.is_none()
        && !(model.kind == helix_view::ai::AiProviderType::LlamaCpp
            && context::member_stem(&context.prefix)
                .is_some_and(|stem| stem.chars().nth(1).is_some()))
    {
        if blank_line {
            log::debug!("AI next line: ineligible");
        }
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
    let completion_server = if variable.is_some()
        || config.conservative && context::member_stem(&context.prefix).is_some()
    {
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
    if !request_model && completion_server.is_none() {
        return;
    }
    let handle = doc.ai_completion_controller.restart();
    let cacheable = compact_cpu
        && variable.is_none()
        && completion_server.is_none()
        && doc.text().len_bytes() <= validation::MAX_SOURCE_BYTES
        && context::member_stem(&context.prefix).is_none();
    doc.ai_anticipated_edit = anticipated.as_ref().map(|edit| AnticipatedEdit {
        view: view.id,
        version: doc.version(),
        cursor,
        insertion: edit.clone(),
    });
    let debounce = if anticipated.is_some() {
        std::time::Duration::ZERO
    } else {
        debounce
    };
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
            let deadline = tokio::time::Instant::now() + debounce;
            let mut field_suffix = None;
            let mut completion_server = completion_server;
            if let Some(stem) = variable.as_deref() {
                if let Some((server, params)) = completion_server.take() {
                    tokio::time::sleep(debounce.min(std::time::Duration::from_millis(20))).await;
                    field_suffix = tokio::time::timeout(
                        std::time::Duration::from_millis(75),
                        context::fetch_variable(server, params, stem),
                    )
                    .await
                    .ok()
                    .flatten();
                }
            }
            if field_suffix.is_none() {
                tokio::time::sleep_until(deadline).await;
            }
            if let Some((server, params)) = completion_server {
                let budget = (config.prefix_chars.min(64_000) / 4).min(600).min(
                    context::MAX_SUPPORTING_CHARS
                        .saturating_sub(context.supporting.chars().count() + 2),
                );
                let stem = context::member_stem(&context.prefix).unwrap_or_default();
                if let Some(members) = context::fetch_members(server, params, stem, budget).await {
                    if model.kind == helix_view::ai::AiProviderType::LlamaCpp
                        && !context.suffix.starts_with(helix_core::chars::char_is_word)
                    {
                        field_suffix = members.field_suffix;
                    }
                    if !members.notes.is_empty() {
                        if !context.supporting.is_empty() {
                            context.supporting.push_str("\n\n");
                        }
                        context.supporting.push_str(&members.notes);
                    }
                    context.members = members.names;
                }
            }
            let text = match field_suffix {
                Some(text) => Some(text),
                None if !request_model => None,
                None => {
                    server::with_model(&model, async {
                        server::wait_ready(&model).await?;
                        client::complete(&config, &model, &context).await
                    })
                    .await?
                }
            };
            let Some(text) = text else {
                if blank_line {
                    log::debug!("AI next line: no candidate after model filtering");
                }
                return Ok(None);
            };
            if let Some((mut source, language)) = syntax_snapshot {
                let proposed = text.clone();
                let mut cursor = cursor;
                if let Some(edit) = &anticipated {
                    source.insert(cursor, edit);
                    cursor += edit.chars().count();
                }
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
                    if blank_line {
                        log::debug!("AI next line: rejected by syntax check");
                    }
                    return Ok(None);
                }
            }
            Ok::<_, anyhow::Error>(Some(text))
        };
        let Some(result) = cancelable_future(future, &handle).await else {
            if blank_line {
                log::debug!("AI next line: canceled");
            }
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
                || editor
                    .config()
                    .ai_provider(helix_view::ai::AiFeature::Completion)
                    .ok()
                    .as_ref()
                    != Some(&model)
                || matches!(
                    editor.last_completion,
                    Some(CompleteAction::Selected { .. })
                )
            {
                return;
            }
            let (view, doc) = current!(editor);
            if doc.id() != doc_id || view.id != snapshot.view {
                return;
            }
            let mut text = text
                .replace("\r\n", "\n")
                .replace('\n', doc.line_ending.as_str());
            let (cursor, latency) = if anticipated.is_some() {
                let Some(edit) = doc
                    .ai_anticipated_edit
                    .take()
                    .filter(|e| e.is_valid(doc, view.id))
                else {
                    return;
                };
                let elapsed = if edit.insertion.is_empty() {
                    doc.last_ai_completion_edit.unwrap_or(started).elapsed()
                } else {
                    text.insert_str(0, &edit.insertion);
                    started.elapsed()
                };
                (edit.cursor, elapsed)
            } else if snapshot.is_valid(doc, view.id) {
                (cursor, started.elapsed())
            } else {
                return;
            };
            if cacheable {
                doc.ai_cached_completion = Some(CachedAiCompletion {
                    source: doc.text().clone(),
                    cursor,
                    path: doc.path().map(ToOwned::to_owned),
                    language: doc.language_name().map(ToOwned::to_owned),
                    indent: doc.indent_style.as_str(),
                    text: text.clone(),
                });
            }
            doc.ai_completion = Some(AiCompletion {
                annotation: InlineAnnotation::new(cursor, text),
                request_latency: Some(latency),
                version: doc.version(),
                view: view.id,
            });
            if blank_line {
                log::debug!("AI next line: preview ready in {} ms", latency.as_millis());
            }
        })
        .await;
    });
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        let version = event.doc.version();
        if event.ghost_transaction
            || !event
                .doc
                .ai_anticipated_edit
                .as_mut()
                .is_some_and(|e| e.consume(event.changes, event.view, version))
        {
            event.doc.ai_completion_controller.cancel();
            event.doc.ai_anticipated_edit = None;
        }
        event.doc.ai_completion = event.doc.ai_completion.take().and_then(|preview| {
            if event.ghost_transaction {
                return None;
            }
            let next = preview.after_insertion(event.changes, event.view, version)?;
            if event
                .doc
                .ai_cached_completion
                .as_ref()
                .is_some_and(|entry| {
                    entry.cursor == preview.annotation.char_idx
                        && entry.text.as_str() == preview.annotation.text.as_str()
                        && &entry.source == event.old_text
                })
            {
                let source = event.doc.text().clone();
                if let Some(entry) = event.doc.ai_cached_completion.as_mut() {
                    entry.source = source;
                    entry.cursor = next.annotation.char_idx;
                    entry.text = next.annotation.text.to_string();
                }
            }
            Some(next)
        });
        Ok(())
    });
    register_hook!(move |event: &mut SelectionDidChange<'_>| {
        if !event
            .doc
            .ai_anticipated_edit
            .as_ref()
            .is_some_and(|e| e.is_valid(event.doc, event.view))
            && !event
                .doc
                .ai_completion
                .as_ref()
                .is_some_and(|preview| preview.is_valid(event.doc, event.view))
        {
            clear_document(event.doc);
        }
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
            doc.ai_cached_completion = None;
            clear_document(doc);
        }
        Ok(())
    });
}
