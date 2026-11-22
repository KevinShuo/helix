use super::ai;
use crate::{
    compositor::{Compositor, Context},
    events::OnModeSwitch,
    job, ui,
};
use anyhow::{ensure, Context as _, Result};
use helix_core::{text_annotations::InlineAnnotation, Selection, Transaction};
use helix_event::{cancelable_future, register_hook, TaskController, TaskHandle};
use helix_view::{
    ai::{AiFeature, AiProviderConfig},
    ai_generate::Operation,
    events::{ConfigDidChange, DocumentDidChange, DocumentFocusLost, SelectionDidChange},
    Document, Editor,
};
use serde_json::{json, Value};

pub(crate) fn active(editor: &Editor) -> bool {
    if editor.should_close() {
        return false;
    }
    let (view, doc) = current_ref!(editor);
    doc.ai_generate
        .as_ref()
        .is_some_and(|op| op.mode == editor.mode() && op.matches(doc, view.id))
}

pub(crate) fn cancel(editor: &mut Editor) {
    if !editor.should_close() {
        doc_mut!(editor).ai_generate = None;
    }
}

pub(crate) fn open(cx: &mut Context, instruction: String) -> Result<()> {
    let editor = &mut *cx.editor;
    editor.config().check_ai_enabled()?;
    ensure!(
        editor.config().ai_generate.enabled,
        "请先在 [editor.ai-generate] 中启用 enabled = true"
    );
    let model = editor.config().ai_provider(AiFeature::Generate)?;
    let mode = editor.mode();
    ensure!(
        editor.macro_recording.is_none() && editor.macro_replaying.is_empty(),
        "宏录制或重放期间无法打开生成输入框"
    );
    ensure!(
        !super::ai_chat::busy(editor)
            && !editor.ai_commit_running
            && editor.ai_merge.is_none()
            && editor.ai_doc.is_none()
            && !super::ai_test::busy(editor),
        "请先完成或取消当前 AI 任务"
    );
    let (view, doc) = current!(editor);
    ensure!(!doc.readonly, "当前文档只读");
    ensure!(
        doc.selection(view.id).len() == 1 && doc.active_snippet.is_none(),
        "请使用单光标并退出 snippet 后生成代码"
    );
    let selection = doc.selection(view.id).primary();
    let cursor = selection.cursor(doc.text().slice(..));
    let old = doc
        .ai_generate
        .take()
        .filter(|op| op.mode == mode && op.matches(doc, view.id));
    let refining = old.as_ref().is_some_and(|op| !op.instruction.is_empty());
    let (previous, previous_draft, failed) = old
        .map(|op| {
            let draft = op
                .annotation
                .map(|a| a.text.to_string())
                .or(op.previous_draft);
            (op.instruction, draft, op.failed)
        })
        .unwrap_or_default();
    doc.ai_completion = None;
    doc.ai_anticipated_edit = None;
    doc.ai_completion_controller.cancel();
    let mut controller = TaskController::new();
    let handle = controller.restart();
    let (previous, initial_input) = if failed {
        (String::new(), previous)
    } else {
        (previous, String::new())
    };
    doc.ai_generate = Some(Operation {
        view: view.id,
        version: doc.version(),
        mode,
        selection,
        cursor,
        instruction: previous,
        previous_draft,
        annotation: None,
        failed: false,
        controller,
    });
    if !instruction.trim().is_empty() {
        return submit(editor, &instruction, model, handle);
    }
    cx.jobs
        .after_event
        .push(Box::new(move |editor, compositor| {
            if !handle.is_canceled() && active(editor) {
                compositor.replace_or_push(
                    ui::ai_generate::ID,
                    ui::ai_generate::Dialog::new(model, handle, refining, initial_input, editor),
                );
            }
        }));
    Ok(())
}

pub(crate) fn submit(
    editor: &mut Editor,
    instruction: &str,
    model: AiProviderConfig,
    handle: TaskHandle,
) -> Result<()> {
    ensure!(
        !handle.is_canceled() && active(editor),
        "光标或文档已变化，请重新生成"
    );
    let instruction = instruction.trim();
    ensure!(!instruction.is_empty(), "请输入生成需求");
    let (view, doc) = current!(editor);
    let op = doc.ai_generate.as_mut().unwrap();
    if !op.instruction.is_empty() {
        op.instruction.push('\n');
    }
    op.instruction.push_str(instruction);
    let messages = match messages(doc, &model) {
        Ok(messages) => messages,
        Err(error) => {
            restore_draft(doc);
            return Err(error);
        }
    };
    let doc_id = doc.id();
    let view_id = view.id;
    editor.set_status("正在生成代码…  Esc 取消");
    tokio::spawn(async move {
        let Some(result) = cancelable_future(ai::interactive(&model, messages), &handle).await
        else {
            return;
        };
        job::dispatch(move |editor, _| {
            if handle.is_canceled() || !active(editor) {
                return;
            }
            let (view, doc) = current!(editor);
            if doc.id() != doc_id || view.id != view_id {
                return;
            }
            match result.and_then(|text| insertion(&text, doc.line_ending.as_str())) {
                Ok(text) => {
                    let op = doc.ai_generate.as_mut().unwrap();
                    op.previous_draft = None;
                    op.annotation = Some(InlineAnnotation::new(op.cursor, text));
                    editor.set_status("Shift+Right 插入代码 · Esc 放弃 · Ctrl+\\ 补充需求");
                }
                Err(error) => {
                    restore_draft(doc);
                    editor.set_error(format!("AI 代码生成失败：{error}（Ctrl+\\ 修改或重试）"));
                }
            }
        })
        .await;
    });
    Ok(())
}

fn restore_draft(doc: &mut Document) {
    if let Some(op) = &mut doc.ai_generate {
        op.failed = true;
        op.annotation = op
            .previous_draft
            .take()
            .map(|text| InlineAnnotation::new(op.cursor, text));
    }
}

fn messages(doc: &Document, model: &AiProviderConfig) -> Result<Vec<Value>> {
    let op = doc.ai_generate.as_ref().unwrap();
    let system = "Return only code to insert between prefix and suffix. Follow requirements and revise any draft. Preserve whitespace.";
    let text = doc.text();
    let mut window = 4096.min(model.budget_context());
    loop {
        let start = op.cursor.saturating_sub(window);
        let end = (op.cursor + window / 2).min(text.len_chars());
        let user = json!({
            "language": doc.language_name(),
            "prefix": text.slice(start..op.cursor).to_string(),
            "suffix": text.slice(op.cursor..end).to_string(),
            "requirements": op.instruction,
            "previous_draft": op.previous_draft,
        });
        let messages = vec![
            json!({"role": "system", "content": system}),
            json!({"role": "user", "content": user.to_string()}),
        ];
        if ai::interactive_fits(model, &messages) {
            return Ok(messages);
        }
        ensure!(
            window > 0,
            "需求或上一版代码超出了模型上下文，请缩短需求或使用更大上下文的模型"
        );
        window /= 2;
    }
}

fn insertion(raw: &str, newline: &str) -> Result<String> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    let text = if trimmed.starts_with("```") {
        let (_, body) = trimmed
            .split_once('\n')
            .context("模型返回了不完整的代码块")?;
        body.strip_suffix("\n```")
            .context("模型返回了不完整的代码块")?
    } else {
        normalized.as_str()
    };
    ensure!(
        !text.trim().is_empty()
            && !text
                .chars()
                .any(|c| c.is_control() && c != '\t' && c != '\n'),
        "模型返回的代码为空或含有控制字符"
    );
    Ok(text.replace('\n', newline))
}

pub(crate) fn accept(editor: &mut Editor) -> Option<String> {
    if !active(editor) {
        return None;
    }
    let (view, doc) = current!(editor);
    let op = doc.ai_generate.as_mut()?;
    let text = op.annotation.take()?.text.to_string();
    let cursor = op.cursor;
    doc.ai_generate = None;
    doc.append_changes_to_history(view);
    let transaction = Transaction::change(
        doc.text(),
        [(cursor, cursor, Some(text.as_str().into()))].into_iter(),
    )
    .with_selection(Selection::point(cursor + text.chars().count()));
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    editor.set_status("已插入生成的代码");
    Some(text)
}

pub(crate) fn refresh(editor: &mut Editor, compositor: &mut Compositor) {
    if !active(editor) {
        cancel(editor);
        compositor.remove(ui::ai_generate::ID);
    }
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        event.doc.ai_generate = None;
        Ok(())
    });
    register_hook!(move |event: &mut SelectionDidChange<'_>| {
        if event
            .doc
            .ai_generate
            .as_ref()
            .is_some_and(|op| op.view == event.view && !op.matches(event.doc, event.view))
        {
            event.doc.ai_generate = None;
        }
        Ok(())
    });
    register_hook!(move |event: &mut DocumentFocusLost<'_>| {
        if let Some(doc) = event.editor.document_mut(event.doc) {
            doc.ai_generate = None;
        }
        Ok(())
    });
    register_hook!(move |event: &mut OnModeSwitch<'_, '_>| {
        cancel(event.cx.editor);
        Ok(())
    });
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        cancel(event.editor);
        Ok(())
    });
}
