//! Explicit test execution, independent of AI/model availability.
mod cmake;
mod python;

use crate::{compositor::Compositor, job, ui};
use anyhow::{ensure, Context, Result};
use helix_view::{
    current_ref,
    test::{discover, Outcome, RunRequest, Scope, TestCase},
    Editor,
};
use std::path::Path;

pub(crate) async fn update(
    id: u64,
    f: impl FnOnce(&mut helix_view::test::TestState) + Send + 'static,
) {
    job::dispatch(move |editor, _| {
        if editor.tests.id == id && editor.tests.running {
            f(&mut editor.tests);
        }
    })
    .await;
}
pub(crate) async fn phase(id: u64, phase: &str) {
    let phase = phase.to_owned();
    update(id, move |s| s.phase = phase).await;
}
pub(crate) async fn publish(id: u64, case: TestCase) {
    update(id, move |s| {
        if let Some(old) = s.cases.iter_mut().find(|c| c.id == case.id) {
            *old = case;
        } else {
            s.cases.push(case);
        }
    })
    .await;
}
pub(crate) async fn log(id: u64, text: String) {
    update(id, move |s| {
        s.log.push_str(&text);
        if s.log.len() > 131_072 {
            let mut at = s.log.len() - 131_072;
            while !s.log.is_char_boundary(at) {
                at += 1;
            }
            s.log.drain(..at);
        }
    })
    .await;
}

pub(crate) fn nearest(editor: &mut Editor) -> Result<()> {
    let (view, doc) = current_ref!(editor);
    let path = doc.path().context("请先保存测试文件")?.to_path_buf();
    let line = doc.text().char_to_line(
        doc.selection(view.id)
            .primary()
            .cursor(doc.text().slice(..)),
    );
    let declaration = discover(doc)
        .into_iter()
        .filter(|d| d.start_line <= line && d.end_line >= line)
        .min_by_key(|d| d.end_line - d.line)
        .context("光标不在已识别的测试中；可使用 :test-file")?;
    start(editor, Scope::Declaration { path, declaration })
}
pub(crate) fn start(editor: &mut Editor, scope: Scope) -> Result<()> {
    let (_, doc) = current_ref!(editor);
    let path = doc.path().context("请先保存测试文件")?;
    let language = doc.language_name().context("无法识别测试语言")?.to_owned();
    ensure!(
        matches!(language.as_str(), "python" | "cpp" | "c"),
        "首版支持 Python 和 C/C++ 测试"
    );
    let request = RunRequest {
        root: super::ai_test::project::root_for(path, &language),
        language,
        config: editor.config().test.effective(&editor.config().ai_test),
        scope,
    };
    launch(editor, request)
}
pub(crate) fn rerun(editor: &mut Editor, failed: bool, ids: Option<Vec<String>>) -> Result<()> {
    let mut request = editor.tests.request.clone().context("尚无测试运行记录")?;
    if failed || ids.is_some() {
        let ids = ids.unwrap_or_else(|| editor.tests.failed_ids());
        ensure!(!ids.is_empty(), "没有可重跑的失败测试");
        let ids = if request.language == "python" {
            ids.into_iter()
                .map(|id| {
                    editor
                        .tests
                        .cases
                        .iter()
                        .find(|c| c.id == id)
                        .and_then(|c| c.selector.clone())
                        .unwrap_or(id)
                })
                .collect()
        } else {
            ids
        };
        request.scope = Scope::Cases(ids);
    }
    launch(editor, request)
}
fn launch(editor: &mut Editor, request: RunRequest) -> Result<()> {
    ensure!(!editor.tests.running, "测试正在运行；请先 :test-stop");
    ensure!(
        !editor
            .documents()
            .any(|d| d.is_modified() && d.path().is_some_and(|p| p.starts_with(&request.root))),
        "请先保存项目中的修改（:wa），再运行测试"
    );
    editor.tests.id = editor.tests.id.wrapping_add(1);
    let id = editor.tests.id;
    editor.tests.running = true;
    editor.tests.stale = false;
    editor.tests.phase = "准备测试".into();
    editor.tests.log.clear();
    editor.tests.cases.clear();
    editor.tests.panel_visible = true;
    editor.tests.focus_requested = true;
    editor.tests.versions = editor
        .documents()
        .filter_map(|d| {
            d.path()
                .filter(|p| p.starts_with(&request.root))
                .map(|p| (p.to_path_buf(), (d.id(), d.version())))
        })
        .collect();
    editor.tests.request = Some(request.clone());
    let task = tokio::spawn(async move {
        let result = if request.language == "python" {
            python::run(id, &request).await
        } else {
            cmake::run(id, &request).await
        };
        job::dispatch(move |editor, _| {
            let state = &mut editor.tests;
            if state.id != id || !state.running {
                return;
            }
            state.running = false;
            state.task = None;
            let completed = result.is_ok();
            match result {
                Ok(()) if state.cases.is_empty() => state.phase = "错误：没有发现测试".into(),
                Ok(()) => {
                    state.phase = if state.cases.iter().any(|c| c.outcome.failed()) {
                        "测试失败"
                    } else if state.cases.iter().all(|c| c.outcome == Outcome::Skipped) {
                        "全部跳过"
                    } else {
                        "运行完成"
                    }
                    .into()
                }
                Err(error) => {
                    state.phase = format!("运行错误：{error}");
                    state.log.push_str(&format!("\n{error:#}\n"));
                }
            }
            for c in &mut state.cases {
                if matches!(c.outcome, Outcome::Queued | Outcome::Running) {
                    // pytest -x/--maxfail can finish without executing all items.
                    c.outcome = if completed {
                        Outcome::Cancelled
                    } else {
                        Outcome::Error
                    };
                }
            }
        })
        .await;
    });
    editor.tests.task = Some(task.abort_handle());
    Ok(())
}
pub(crate) fn stop(editor: &mut Editor) {
    if let Some(task) = editor.tests.task.take() {
        task.abort();
    }
    if editor.tests.running {
        editor.tests.id = editor.tests.id.wrapping_add(1);
        editor.tests.running = false;
        editor.tests.phase = "已停止".into();
        for c in &mut editor.tests.cases {
            if matches!(c.outcome, Outcome::Queued | Outcome::Running) {
                c.outcome = Outcome::Cancelled;
            }
        }
    }
}

pub(crate) fn refresh(editor: &mut Editor, compositor: &mut Compositor) {
    let changed: Vec<_> = editor
        .documents()
        .filter_map(|doc| {
            let path = doc.path()?;
            if editor.tests.declarations.get(path).is_some_and(|cache| {
                cache.document == doc.id()
                    && cache.version == doc.version()
                    && cache.language == doc.language_name().unwrap_or_default()
            }) {
                return None;
            }
            Some((
                path.to_path_buf(),
                helix_view::test::DeclarationCache {
                    document: doc.id(),
                    version: doc.version(),
                    language: doc.language_name().unwrap_or_default().into(),
                    declarations: discover(doc),
                },
            ))
        })
        .collect();
    let open: std::collections::HashSet<_> = editor
        .documents()
        .filter_map(|d| d.path().map(Path::to_path_buf))
        .collect();
    editor.tests.declarations.retain(|p, _| open.contains(p));
    for (path, cache) in changed {
        editor.tests.declarations.insert(path, cache);
    }
    if editor.tests.request.as_ref().is_some_and(|r| {
        editor.documents().any(|d| {
            d.path().is_some_and(|p| {
                p.starts_with(&r.root)
                    && (d.is_modified()
                        || editor
                            .tests
                            .versions
                            .get(p)
                            .is_some_and(|(id, version)| *id != d.id() || *version != d.version()))
            })
        })
    }) {
        editor.tests.stale = true;
    }
    if editor.tests.panel_visible && compositor.find::<ui::test::TestPanel>().is_none() {
        compositor.remove("git-tools");
        compositor.push(Box::new(ui::test::TestPanel::new()));
    }
    if editor.tests.focus_requested {
        if let Some(panel) = compositor.find::<ui::test::TestPanel>() {
            panel.focus();
            editor.tests.focus_requested = false;
        }
    }
}

pub(crate) fn normalized(path: impl AsRef<Path>) -> std::path::PathBuf {
    helix_stdx::path::canonicalize(path.as_ref().to_owned())
}
