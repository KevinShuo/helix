mod client;
pub(crate) mod format;
pub(crate) mod target;

use crate::{compositor::Compositor, job, ui};
use anyhow::{ensure, Context as _, Result};
use helix_core::Transaction;
use helix_event::{cancelable_future, register_hook};
use helix_view::{
    ai_doc::{Operation, Target},
    events::{ConfigDidChange, DocumentDidChange, DocumentFocusLost},
    Editor,
};

pub(crate) fn cancel(editor: &mut Editor) {
    if let Some(op) = editor.ai_doc.take() {
        if let Some(doc) = editor.document_mut(op.doc) {
            doc.ai_doc_controller.cancel();
        }
    }
}

pub(crate) fn start(editor: &mut Editor) -> Result<()> {
    ensure!(
        !editor.ai_commit_running,
        "AI 提交分析正在运行，请先完成或取消"
    );
    ensure!(
        editor.config().ai_doc.enabled,
        "请先在 [editor.ai-doc] 中启用 enabled = true"
    );
    ensure!(
        editor.macro_recording.is_none() && editor.macro_replaying.is_empty(),
        "宏录制或重放期间不支持 AI 文档生成"
    );
    cancel(editor);
    ensure!(
        !matches!(
            editor.last_completion,
            Some(helix_view::editor::CompleteAction::Selected { .. })
        ),
        "请先退出补全候选预览"
    );
    let (view, doc) = current!(editor);
    ensure!(!doc.readonly, "当前文档只读");
    ensure!(
        doc.selection(view.id).len() == 1 && doc.active_snippet.is_none(),
        "请使用单光标并退出 snippet 后生成注释"
    );
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let target = target::locate(
        doc.text().slice(..),
        doc.syntax().context("当前文档没有可用语法树")?,
        cursor,
        doc.language_name().unwrap_or(""),
    )?;
    launch(editor, target)
}

fn launch(editor: &mut Editor, target: Target) -> Result<()> {
    let model = editor.config().ai_model_config();
    let config = editor.config().ai_doc.clone();
    client::request(&model, &config, &target)?;
    // Manual generation has priority over automatic requests across all views.
    for doc in editor.documents.values_mut() {
        doc.ai_completion_controller.cancel();
        doc.ai_completion = None;
    }
    editor.ai_doc_serial = editor.ai_doc_serial.wrapping_add(1);
    let id = editor.ai_doc_serial;
    let (view, doc) = current!(editor);
    let op = Operation {
        id,
        doc: doc.id(),
        view: view.id,
        version: doc.version(),
        model: model.clone(),
        config: config.clone(),
        target: target.clone(),
        handle: doc.ai_doc_controller.restart(),
        text: None,
        preview: false,
    };
    let handle = op.handle.clone();
    editor.ai_doc = Some(op);
    editor.set_status("正在生成函数注释…（:ai-doc-cancel 可取消）");
    tokio::spawn(async move {
        let Some(result) =
            cancelable_future(client::generate(&model, &config, &target), &handle).await
        else {
            return;
        };
        job::dispatch(move |editor, _| {
            if !editor
                .ai_doc
                .as_ref()
                .is_some_and(|op| op.id == id && op.is_valid(editor))
            {
                return;
            }
            match result {
                Ok(text) => {
                    editor.ai_doc.as_mut().unwrap().text = Some(text);
                    editor.set_status("函数注释已生成，等待预览");
                }
                Err(error) => {
                    cancel(editor);
                    editor.set_error(format!("AI 注释生成失败：{error}"));
                }
            }
        })
        .await;
    });
    Ok(())
}

pub(crate) fn retry(editor: &mut Editor) -> Result<()> {
    let op = editor
        .ai_doc
        .as_ref()
        .filter(|op| op.is_valid(editor))
        .context("注释目标已变化，请重新执行 :ai-doc")?
        .clone();
    cancel(editor);
    launch(editor, op.target)
}

pub(crate) fn accept(editor: &mut Editor) -> Result<()> {
    ensure!(
        editor.ai_doc.as_ref().is_some_and(|op| op.is_valid(editor)),
        "注释目标已变化，请重新生成"
    );
    let op = editor.ai_doc.take().unwrap();
    let text = op.text.context("注释尚未生成完成")?;
    let (view, doc) = current!(editor);
    doc.ai_doc_controller.cancel();
    doc.append_changes_to_history(view);
    let transaction = Transaction::change(
        doc.text(),
        [(op.target.insert, op.target.insert, Some(text.into()))].into_iter(),
    );
    doc.apply(&transaction, view.id);
    doc.append_changes_to_history(view);
    // Reveal the insertion without changing the user's logical code position.
    // A later cursor motion may scroll back to that position as usual.
    while doc.unfold_at(view.id, op.target.insert) {}
    let anchor = doc
        .text()
        .line_to_char(doc.text().char_to_line(op.target.insert).saturating_sub(1));
    doc.set_view_offset(
        view.id,
        helix_view::view::ViewPosition {
            anchor,
            ..Default::default()
        },
    );
    editor.set_status("已添加函数注释（u 可撤销）");
    Ok(())
}

/// Called before compositor rendering, after command callbacks have finished.
pub(crate) fn refresh(editor: &mut Editor, compositor: &mut Compositor) {
    if editor
        .ai_doc
        .as_ref()
        .is_some_and(|op| !op.is_valid(editor))
    {
        cancel(editor);
    }
    if editor.ai_doc.is_none() {
        compositor.remove("ai-doc");
        return;
    }
    if compositor.editor_has_focus() {
        let op = editor.ai_doc.as_mut().unwrap();
        if let Some(text) = &op.text {
            if !op.preview {
                op.preview = true;
                compositor.push(Box::new(ui::ai_doc::Preview::new(op.id, &op.target, text)));
            }
        }
    }
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        event.doc.ai_doc_controller.cancel();
        Ok(())
    });
    register_hook!(move |event: &mut DocumentFocusLost<'_>| {
        cancel(event.editor);
        Ok(())
    });
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        cancel(event.editor);
        Ok(())
    });
}
