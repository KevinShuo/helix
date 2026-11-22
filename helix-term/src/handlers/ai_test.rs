mod client;
pub(crate) mod project;
pub(crate) mod runner;

use crate::{compositor::Compositor, job, ui};
use anyhow::{ensure, Context, Result};
use helix_core::Transaction;
use helix_event::{cancelable_future, register_hook};
use helix_view::{
    ai::AiFeature,
    ai_test::{Operation, Project},
    editor::Action,
    events::ConfigDidChange,
    Editor,
};

pub(crate) fn busy(editor: &Editor) -> bool {
    editor
        .ai_test
        .as_ref()
        .is_some_and(|op| !op.applied || op.running)
}

pub(crate) fn cancel(editor: &mut Editor) {
    editor.ai_test_controller.cancel();
    editor.ai_test = None;
}

fn preflight(
    editor: &Editor,
    project: &Project,
    applied: Option<&[helix_view::ai_test::FileChange]>,
) -> Result<()> {
    for snap in &project.snapshots {
        project::confined(&project.root, &snap.path)?;
        let expected = applied
            .and_then(|changes| changes.iter().find(|c| c.before.path == snap.path))
            .map(|c| Some(c.after.clone()))
            .unwrap_or_else(|| snap.text.clone());
        ensure!(
            project::snapshot(&snap.path)?.text == expected,
            "文件已变化，请重新生成：{}",
            snap.path.display()
        );
        if let Some(doc) = editor.document_by_path(&snap.path) {
            ensure!(
                !doc.is_modified(),
                "请先保存缓冲区：{}",
                snap.path.display()
            );
            if let Some(text) = &expected {
                ensure!(
                    doc.text().to_string() == *text,
                    "缓冲区与磁盘内容不一致，请重新加载：{}",
                    snap.path.display()
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn start(editor: &mut Editor) -> Result<()> {
    editor.config().check_ai_enabled()?;
    ensure!(
        editor.config().ai_test.enabled,
        "请先在 [editor.ai-test] 中启用 enabled = true"
    );
    ensure!(
        !super::ai::interactive_busy(editor)
            && !editor.ai_commit_running
            && editor.ai_doc.is_none()
            && editor.ai_merge.is_none(),
        "请先完成或取消其他 AI 生成任务"
    );
    ensure!(
        editor.macro_recording.is_none() && editor.macro_replaying.is_empty(),
        "宏录制或重放期间不支持测试生成"
    );
    ensure!(
        !matches!(
            editor.last_completion,
            Some(helix_view::editor::CompleteAction::Selected { .. })
        ),
        "请先退出补全候选预览"
    );
    let (view, doc) = current!(editor);
    ensure!(
        !doc.readonly && !doc.is_modified(),
        "请先保存被测源文件，并确认文件可编辑"
    );
    ensure!(
        doc.selection(view.id).len() == 1 && doc.active_snippet.is_none(),
        "请使用单光标并退出 snippet"
    );
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let target = super::ai_doc::target::locate_for_test(
        doc.text().slice(..),
        doc.syntax().context("当前文档没有可用语法树")?,
        cursor,
        doc.language_name().unwrap_or(""),
    )?;
    let path = doc.path().context("请先保存被测源文件")?.to_owned();
    let (id, version) = (doc.id(), doc.version());
    let config = editor.config().ai_test.clone();
    let project = project::discover(&path, &target, &config)?;
    preflight(editor, &project, None)?;
    cancel(editor);
    editor.ai_test_serial = editor.ai_test_serial.wrapping_add(1);
    let model = editor.config().ai_provider(AiFeature::Test)?;
    let op = Operation {
        id: editor.ai_test_serial,
        doc: id,
        version,
        model,
        config,
        target,
        project,
        handle: editor.ai_test_controller.restart(),
        changes: None,
        assumptions: Vec::new(),
        preview: false,
        applied: false,
        running: false,
        repairs: 0,
        report: None,
    };
    launch(editor, op, None)
}

fn launch(editor: &mut Editor, mut op: Operation, repair: Option<(String, String)>) -> Result<()> {
    client::request(
        &op.model,
        &op.config,
        &op.target,
        &op.project,
        repair.as_ref().map(|(c, d)| (c.as_str(), d.as_str())),
    )?;
    for doc in editor.documents_mut() {
        doc.ai_completion_controller.cancel();
        doc.ai_completion = None;
    }
    op.changes = None;
    op.preview = false;
    op.applied = false;
    op.running = false;
    let (id, model, config, target, project, handle) = (
        op.id,
        op.model.clone(),
        op.config.clone(),
        op.target.clone(),
        op.project.clone(),
        op.handle.clone(),
    );
    editor.ai_test = Some(op);
    editor.set_status(if repair.is_some() {
        "正在修复测试的编译或导入错误…（:ai-test-cancel 可取消）"
    } else {
        "正在生成单元测试…（:ai-test-cancel 可取消）"
    });
    tokio::spawn(async move {
        let Some(result) = cancelable_future(
            client::generate(
                &model,
                &config,
                &target,
                &project,
                repair.as_ref().map(|(c, d)| (c.as_str(), d.as_str())),
            ),
            &handle,
        )
        .await
        else {
            return;
        };
        job::dispatch(move |editor, _| {
            if !editor
                .ai_test
                .as_ref()
                .is_some_and(|op| op.id == id && op.source_valid(editor))
            {
                return;
            }
            let result = result.and_then(|generated| {
                preflight(editor, &project, None)?;
                let changes = project::changes(&project, &generated.code)?;
                Ok((generated, changes))
            });
            match result {
                Ok((generated, changes)) => {
                    let op = editor.ai_test.as_mut().unwrap();
                    op.changes = Some(changes);
                    op.assumptions = generated.assumptions;
                    editor.set_status("单元测试已生成，等待预览");
                }
                Err(error) => {
                    if editor
                        .ai_test
                        .as_ref()
                        .is_some_and(|op| op.repairs > 0 && op.report.is_some())
                    {
                        let op = editor.ai_test.as_mut().unwrap();
                        op.applied = true;
                        op.running = false;
                        op.preview = false;
                        op.report.as_mut().unwrap().log.push_str(&format!(
                            "\n自动修复失败：{error}\n原测试和运行诊断已保留。"
                        ));
                        editor.set_error(format!("AI 测试修复失败：{error}；原诊断已保留"));
                    } else {
                        cancel(editor);
                        editor.set_error(format!("AI 测试生成失败：{error}"));
                    }
                }
            }
        })
        .await;
    });
    Ok(())
}

pub(crate) fn retry(editor: &mut Editor) -> Result<()> {
    let mut op = editor
        .ai_test
        .as_ref()
        .filter(|op| op.source_valid(editor) && !op.applied && !op.running)
        .context("测试目标已变化，请重新执行 :ai-test")?
        .clone();
    preflight(editor, &op.project, None)?;
    op.handle = editor.ai_test_controller.restart();
    editor.ai_test_serial = editor.ai_test_serial.wrapping_add(1);
    op.id = editor.ai_test_serial;
    launch(editor, op, None)
}

pub(crate) fn accept(editor: &mut Editor) -> Result<()> {
    let op = editor
        .ai_test
        .as_ref()
        .filter(|op| op.source_valid(editor) && !op.applied)
        .context("测试目标已变化，请重新生成")?
        .clone();
    let changes = op.changes.as_ref().context("测试尚未生成完成")?;
    preflight(editor, &op.project, None)?;
    // Load every file before changing any buffer. Action::Load keeps source focus.
    let mut ids = Vec::new();
    for change in changes {
        let id = editor.open(&change.before.path, Action::Load)?;
        let doc = editor.document(id).unwrap();
        ensure!(
            !doc.readonly && !doc.is_modified(),
            "目标文件只读或有未保存修改：{}",
            change.before.path.display()
        );
        ids.push(id);
    }
    preflight(editor, &op.project, None)?;
    let view = editor.tree.get_mut(editor.tree.focus);
    for (change, id) in changes.iter().zip(ids) {
        let doc = editor.documents.get_mut(&id).unwrap();
        doc.append_changes_to_history(view);
        let transaction = Transaction::change(
            doc.text(),
            [(0, doc.text().len_chars(), Some(change.after.clone().into()))].into_iter(),
        );
        doc.apply(&transaction, view.id);
        doc.append_changes_to_history(view);
    }
    editor.ai_test.as_mut().unwrap().applied = true;
    editor.ai_test.as_mut().unwrap().preview = true;
    editor
        .set_status("测试与构建修改已放入缓冲区；:wa! 保存后执行 :ai-test-run（各文件可 u 撤销）");
    Ok(())
}

pub(crate) fn run_last(editor: &mut Editor) -> Result<()> {
    ensure!(
        !super::ai::interactive_busy(editor)
            && !editor.ai_commit_running
            && editor.ai_doc.is_none()
            && editor.ai_merge.is_none(),
        "请先完成其他 AI 生成任务"
    );
    let mut op = editor
        .ai_test
        .as_ref()
        .filter(|op| op.applied && !op.running && op.source_valid(editor))
        .context("请先生成并接受测试修改")?
        .clone();
    preflight(editor, &op.project, op.changes.as_deref())?;
    op.running = true;
    op.report = None;
    op.preview = false;
    op.handle = editor.ai_test_controller.restart();
    let (id, handle, project, config) = (
        op.id,
        op.handle.clone(),
        op.project.clone(),
        op.config.clone(),
    );
    editor.ai_test = Some(op);
    editor.set_status("正在运行本次生成的测试…（:ai-test-cancel 可取消）");
    tokio::spawn(async move {
        let Some(report) = cancelable_future(runner::run(&project, &config), &handle).await else {
            return;
        };
        job::dispatch(move |editor, _| {
            if !editor
                .ai_test
                .as_ref()
                .is_some_and(|op| op.id == id && op.source_valid(editor))
            {
                return;
            }
            let valid = {
                let op = editor.ai_test.as_ref().unwrap();
                preflight(editor, &op.project, op.changes.as_deref())
            };
            if let Err(error) = valid {
                cancel(editor);
                editor.set_error(format!("测试运行期间输入发生变化，结果已丢弃：{error}"));
                return;
            }
            let repair = report.repairable && editor.ai_test.as_ref().unwrap().repairs == 0;
            let success = report.success;
            let op = editor.ai_test.as_mut().unwrap();
            op.running = false;
            op.report = Some(report);
            op.preview = false;
            if repair {
                if let Err(error) = repair_once(editor) {
                    editor.set_error(format!(
                        "测试失败，无法自动修复：{error}；:ai-test-show 查看诊断"
                    ));
                }
            } else {
                editor.set_status(if success {
                    "本次生成的测试已通过；这不代表测试覆盖充分"
                } else {
                    "测试运行失败，已保留测试和诊断"
                });
            }
        })
        .await;
    });
    Ok(())
}

fn repair_once(editor: &mut Editor) -> Result<()> {
    let mut op = editor.ai_test.as_ref().context("没有测试任务")?.clone();
    let report = op.report.as_ref().context("没有失败诊断")?;
    ensure!(
        report.repairable && op.repairs == 0,
        "本次任务不能继续自动修复"
    );
    let code = std::fs::read_to_string(&op.project.test_file)?;
    let diagnostic = report.log.clone();
    for snap in &mut op.project.snapshots {
        *snap = project::snapshot(&snap.path)?;
    }
    op.project.build_changes.clear();
    op.repairs += 1;
    op.handle = editor.ai_test_controller.restart();
    editor.ai_test_serial = editor.ai_test_serial.wrapping_add(1);
    op.id = editor.ai_test_serial;
    launch(editor, op, Some((code, diagnostic)))
}

pub(crate) fn show(editor: &mut Editor) -> Result<()> {
    let op = editor.ai_test.as_mut().context("没有测试结果")?;
    ensure!(
        op.report.is_some() || op.changes.is_some(),
        "测试尚未生成完成"
    );
    op.preview = false;
    Ok(())
}

pub(crate) fn refresh(editor: &mut Editor, compositor: &mut Compositor) {
    if editor
        .ai_test
        .as_ref()
        .is_some_and(|op| !op.source_valid(editor))
    {
        cancel(editor);
    }
    if editor.ai_test.is_none() {
        compositor.remove("ai-test");
        return;
    }
    if compositor.editor_has_focus() {
        let op = editor.ai_test.as_mut().unwrap();
        if !op.preview
            && !op.running
            && (op.changes.is_some() || (op.applied && op.report.is_some()))
        {
            op.preview = true;
            compositor.push(Box::new(ui::ai_test::Preview::new(op)));
        }
    }
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        cancel(event.editor);
        Ok(())
    });
}
