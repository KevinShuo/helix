//! Asynchronous compilation and compiler diagnostics.
pub(crate) mod parser;
mod runner;

use anyhow::{ensure, Context, Result};
use helix_view::{
    editor::Action,
    problems::{BuildRun, FileStamp, Problem, BUILD_LOG_LIMIT, BUILD_PROBLEM_LIMIT},
    Editor,
};
use std::{collections::BTreeMap, path::PathBuf, time::SystemTime};

pub(crate) fn start(editor: &mut Editor, command: String) -> Result<()> {
    let cwd = helix_stdx::env::current_working_dir().to_owned();
    start_at(editor, command, cwd)
}

fn start_at(editor: &mut Editor, command: String, cwd: PathBuf) -> Result<()> {
    ensure!(!command.trim().is_empty(), "Compilation command is empty");
    ensure!(
        !editor
            .problems
            .compilation
            .current
            .as_ref()
            .is_some_and(|r| r.running),
        "A compilation is already running; use :compile-cancel first"
    );
    let unsaved = editor
        .documents()
        .filter(|d| d.is_modified())
        .filter_map(|d| d.path().map(|p| p.to_owned()))
        .collect::<Vec<_>>();
    let started = SystemTime::now();
    let process = runner::spawn(&editor.config().shell, &command, &cwd)?;
    let state = &mut editor.problems.compilation;
    state.serial = state.serial.wrapping_add(1);
    let id = state.serial;
    let handle = state.controller.restart();
    state.current = Some(BuildRun {
        id,
        command,
        cwd: cwd.clone(),
        started,
        running: true,
        status: "Running".into(),
        log: String::new(),
        truncated: false,
        problems: vec![],
        snapshots: BTreeMap::new(),
        unsaved,
    });
    editor.problems.selected = None;
    editor.problems.anchor = None;
    refresh_log(editor);
    let warning = editor
        .problems
        .compilation
        .current
        .as_ref()
        .unwrap()
        .unsaved
        .len();
    editor.set_status(if warning == 0 {
        "Compilation started".into()
    } else {
        format!("Compilation started; {warning} unsaved buffer(s), using files on disk")
    });
    tokio::spawn(runner::run(process, cwd, id, handle));
    Ok(())
}

pub(crate) fn recompile(editor: &mut Editor) -> Result<()> {
    let run = editor
        .problems
        .compilation
        .current
        .as_ref()
        .context("No previous compilation")?;
    let (command, cwd) = (run.command.clone(), run.cwd.clone());
    start_at(editor, command, cwd)
}

pub(crate) fn cancel(editor: &mut Editor) {
    editor.problems.compilation.controller.cancel();
    if let Some(run) = &mut editor.problems.compilation.current {
        if run.running {
            run.running = false;
            run.status = "Canceled".into();
        }
    }
    refresh_log(editor);
    editor.set_status("Compilation canceled");
}

fn output(editor: &mut Editor, id: u64, text: String, problems: Vec<Problem>, truncated: bool) {
    let Some(run) = &mut editor.problems.compilation.current else {
        return;
    };
    if run.id != id || !run.running {
        return;
    }
    run.log.push_str(&text);
    run.truncated |= truncated;
    if run.log.len() > BUILD_LOG_LIMIT {
        let mut end = run.log.len() - BUILD_LOG_LIMIT;
        while !run.log.is_char_boundary(end) {
            end += 1;
        }
        run.log.drain(..end);
        run.truncated = true;
    }
    for p in problems {
        if run.problems.len() >= BUILD_PROBLEM_LIMIT {
            run.truncated = true;
            break;
        }
        run.snapshots
            .entry(p.path.clone())
            .or_insert_with(|| FileStamp::read(&p.path));
        run.problems.push(p);
    }
    refresh_log(editor);
}

fn finished(editor: &mut Editor, id: u64, status: String) {
    let Some(run) = &mut editor.problems.compilation.current else {
        return;
    };
    if run.id != id || !run.running {
        return;
    }
    run.running = false;
    run.status = status;
    let message = format!(
        "{}; {} located problem(s){}",
        run.status,
        run.problems.len(),
        if run.truncated {
            "; output truncated"
        } else {
            ""
        }
    );
    refresh_log(editor);
    editor.set_status(message);
}

fn log_text(editor: &Editor) -> Option<String> {
    let run = editor.problems.compilation.current.as_ref()?;
    Some(format!(
        "*compilation* — {}\n{}\n$ {}\n{}\n\n{}",
        run.status,
        run.cwd.display(),
        run.command,
        if run.truncated {
            "[output truncated: log/line/problem limit reached]"
        } else {
            ""
        },
        run.log
    ))
}

fn refresh_log(editor: &mut Editor) {
    let Some(id) = editor.problems.compilation.log_document else {
        return;
    };
    let view_id = editor
        .tree
        .views()
        .find(|(v, _)| v.doc == id)
        .map(|(v, _)| v.id)
        .unwrap_or(editor.tree.focus);
    let Some(text) = log_text(editor) else { return };
    let Some(doc) = editor.documents.get_mut(&id) else {
        return;
    };
    doc.ensure_view_init(view_id);
    doc.set_readonly_output("*compilation*");
    // Internal read-only output update: preserve focus and never touch source text.
    let transaction =
        helix_core::diff::compare_ropes(doc.text(), &helix_core::Rope::from_str(&text));
    doc.update_readonly_output(&transaction, view_id);
    let view = editor.tree.get_mut(view_id);
    doc.append_changes_to_history(view);
    // Compiler output is a bounded stream. Map every view's jumps to the new
    // revision zero before discarding the transient output history.
    for (view, _) in editor.tree.views_mut() {
        view.jumps.reset_document_history(doc);
    }
    doc.history.set(Default::default());
    doc.reset_modified();
    doc.readonly = true;
}

pub(crate) fn show_log(editor: &mut Editor) -> Result<()> {
    ensure!(
        editor.problems.compilation.current.is_some(),
        "No compilation log"
    );
    if let Some(id) = editor
        .problems
        .compilation
        .log_document
        .filter(|id| editor.documents.contains_key(id))
    {
        editor.switch(id, Action::HorizontalSplit);
    } else {
        let id = editor.new_file(Action::HorizontalSplit);
        editor.problems.compilation.log_document = Some(id);
    }
    refresh_log(editor);
    Ok(())
}
