mod client;

use crate::{compositor::Compositor, job, ui};
use anyhow::{ensure, Context as _, Result};
use helix_core::{Rope, Transaction};
use helix_event::{cancelable_future, register_hook};
use helix_vcs::merge::MergeMode;
use helix_view::{
    ai_merge::{Operation, Target},
    events::{ConfigDidChange, DocumentDidChange, DocumentFocusLost},
    merge::MergeWriteStatus,
    Editor,
};
use std::ops::Range;

pub(crate) fn cancel(editor: &mut Editor) {
    editor.cancel_ai_merge();
}

pub(crate) fn start(editor: &mut Editor, block: usize, intent: String) -> Result<()> {
    ensure!(
        cfg!(feature = "git"),
        "AI Merge requires a build with Git support"
    );
    ensure!(
        editor.config().ai_merge.enabled,
        "请先在 [editor.ai-merge] 中启用 enabled = true"
    );
    ensure!(
        !editor.ai_commit_running && !editor.ai_doc.as_ref().is_some_and(|op| op.is_valid(editor)),
        "请先完成或取消其他 AI 生成"
    );
    ensure!(
        editor.macro_recording.is_none() && editor.macro_replaying.is_empty(),
        "宏录制或重放期间不支持 AI Merge"
    );
    ensure!(
        !matches!(
            editor.last_completion,
            Some(helix_view::editor::CompleteAction::Selected { .. })
        ),
        "请先退出补全候选预览"
    );
    let (_, doc) = helix_view::current_ref!(editor);
    let state = doc
        .merge()
        .context("请先打开 Merge 冲突文件（Space v r）")?;
    ensure!(
        editor.active_merge() == Some(state.session),
        "请先进入当前 Merge 会话"
    );
    ensure!(
        ui::git::trusted(editor, state.data.version.root()),
        "请先 :trust 信任当前工作区"
    );
    ensure!(
        !doc.readonly && !state.busy && matches!(state.write_status, MergeWriteStatus::Draft),
        "只允许在可写的冲突草稿中生成；已暂存或过期的结果需先刷新"
    );
    ensure!(doc.active_snippet.is_none(), "请先退出 snippet");
    ensure!(
        state.model.mode() == MergeMode::Blockwise,
        "当前为整文件人工复核模式，无法定位单个冲突块"
    );
    let selected = state
        .model
        .blocks()
        .get(block)
        .context("当前冲突块不存在")?;
    let sources = state.model.sources();
    let mut context = Vec::new();
    for (label, text, range) in [
        ("Draft", doc.text(), &selected.result),
        ("Current", sources.current(), &selected.current),
        ("Incoming", sources.incoming(), &selected.incoming),
        ("Base", sources.base(), &selected.base),
    ] {
        context.push(nearby(label, text, range));
    }
    for (label, text) in [
        ("Current", sources.current()),
        ("Incoming", sources.incoming()),
    ] {
        let end = text.line_to_char(40.min(text.len_lines()));
        context.push(format!(
            "{label} file header (declarations/imports):\n{}",
            text.slice(..end)
        ));
    }
    let target = Target {
        block,
        path: state.data.file.path.to_string_lossy().into_owned(),
        language: doc.language_name().unwrap_or("").to_owned(),
        base: sources.base().slice(selected.base.clone()).to_string(),
        current: sources
            .current()
            .slice(selected.current.clone())
            .to_string(),
        incoming: sources
            .incoming()
            .slice(selected.incoming.clone())
            .to_string(),
        result: doc.text().slice(selected.result.clone()).to_string(),
        context,
        intent,
        range: selected.result.clone(),
    };
    launch(editor, target)
}

fn nearby(label: &str, text: &Rope, range: &Range<usize>) -> String {
    let start = text.line_to_char(text.char_to_line(range.start).saturating_sub(12));
    let end = text.line_to_char((text.char_to_line(range.end) + 13).min(text.len_lines()));
    format!(
        "{label} nearby:\n{}[TARGET BLOCK]\n{}",
        text.slice(start..range.start),
        text.slice(range.end..end)
    )
}

fn launch(editor: &mut Editor, target: Target) -> Result<()> {
    let model = editor.config().ai_model_config();
    let config = editor.config().ai_merge.clone();
    client::request(&model, &config, &target)?;
    cancel(editor);
    crate::handlers::ai_doc::cancel(editor);
    for doc in editor.documents.values_mut() {
        doc.ai_completion_controller.cancel();
        doc.ai_completion = None;
    }
    editor.ai_merge_serial = editor.ai_merge_serial.wrapping_add(1);
    let id = editor.ai_merge_serial;
    let (view, doc) = helix_view::current!(editor);
    doc.merge_mut().unwrap().selected_block = target.block;
    let state = doc.merge().unwrap();
    let session = state.session;
    let source_version = state.data.version.clone();
    let op = Operation {
        id,
        doc: doc.id(),
        view: view.id,
        version: doc.version(),
        session,
        source_version,
        model: model.clone(),
        config: config.clone(),
        target: target.clone(),
        handle: doc.ai_merge_controller.restart(),
        suggestion: None,
        preview: false,
    };
    let handle = op.handle.clone();
    editor.ai_merge = Some(op);
    editor.set_status("正在生成当前冲突块的 AI 候选…（:ai-merge-cancel 可取消）");
    tokio::spawn(async move {
        let Some(result) =
            cancelable_future(client::generate(&model, &config, &target), &handle).await
        else {
            return;
        };
        job::dispatch(move |editor, _| {
            if !editor
                .ai_merge
                .as_ref()
                .is_some_and(|op| op.id == id && op.is_valid(editor))
            {
                return;
            }
            match result {
                Ok(suggestion) => {
                    editor.ai_merge.as_mut().unwrap().suggestion = Some(suggestion);
                    editor.set_status("AI 合并候选已生成，等待预览");
                }
                Err(error) => {
                    cancel(editor);
                    editor.set_error(format!("AI Merge 失败：{error}"));
                }
            }
        })
        .await;
    });
    Ok(())
}

pub(crate) fn retry(editor: &mut Editor) -> Result<()> {
    let target = editor
        .ai_merge
        .as_ref()
        .filter(|op| op.is_valid(editor))
        .context("冲突目标已变化，请重新执行 :ai-merge")?
        .target
        .clone();
    launch(editor, target)
}

pub(crate) fn accept(editor: &mut Editor) -> Result<()> {
    let op = editor
        .ai_merge
        .as_ref()
        .filter(|op| op.is_valid(editor))
        .context("冲突目标已变化，请重新生成")?;
    ensure!(
        ui::git::trusted(editor, op.source_version.root()),
        "请先 :trust 信任当前工作区"
    );
    let code = op
        .suggestion
        .as_ref()
        .and_then(|s| s.code.as_ref())
        .context("此冲突需要人工处理")?;
    let block = op.target.block;
    let (_, doc) = helix_view::current_ref!(editor);
    let tx = Transaction::change(
        doc.text(),
        [(
            op.target.range.start,
            op.target.range.end,
            Some(code.clone().into()),
        )]
        .into_iter(),
    );
    let mut after = doc.text().clone();
    ensure!(tx.apply(&mut after), "无法构造当前冲突块的修改");
    doc.merge()
        .unwrap()
        .data
        .format
        .context("不支持当前文件格式")?
        .encode(after.slice(..))?;
    cancel(editor);
    let (view, doc) = helix_view::current!(editor);
    doc.append_changes_to_history(view);
    doc.apply(&tx, view.id);
    doc.merge_mut().unwrap().model.require_review(block);
    doc.append_changes_to_history(view);
    editor.set_status("AI 候选已接受到 Result；请 Review 后再 Apply（u 可撤销）");
    Ok(())
}

pub(crate) fn refresh(editor: &mut Editor, compositor: &mut Compositor) {
    if editor.ai_merge.as_ref().is_some_and(|op| {
        !op.is_valid(editor) || !ui::git::trusted(editor, op.source_version.root())
    }) {
        cancel(editor);
    }
    let Some(op) = editor.ai_merge.as_ref() else {
        compositor.remove("ai-merge");
        return;
    };
    if compositor.component_has_focus("git-merge-editor") && !op.preview {
        if let Some(suggestion) = &op.suggestion {
            match ui::ai_merge::Preview::new(op.id, &op.target, suggestion) {
                Ok(preview) => {
                    editor.ai_merge.as_mut().unwrap().preview = true;
                    compositor.push(Box::new(preview));
                }
                Err(error) => {
                    cancel(editor);
                    editor.set_error(error.to_string());
                }
            }
        }
    }
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        event.doc.ai_merge_controller.cancel();
        Ok(())
    });
    register_hook!(move |event: &mut DocumentFocusLost<'_>| {
        if event
            .editor
            .ai_merge
            .as_ref()
            .is_some_and(|op| op.doc == event.doc)
        {
            cancel(event.editor);
        }
        Ok(())
    });
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        cancel(event.editor);
        Ok(())
    });
}
