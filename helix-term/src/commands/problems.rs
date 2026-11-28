//! Navigation shared by live language-server diagnostics and compiler output.
use super::{align_view, push_jump, Align, Context};
use crate::{
    job,
    ui::{overlay::overlaid, Picker, PickerColumn},
};
use anyhow::{bail, Context as _, Result};
use helix_core::{diagnostic::Severity, syntax::config::LanguageServerFeature, Selection};
use helix_lsp::{lsp, util::lsp_range_to_range, OffsetEncoding};
use helix_view::{
    editor::Action,
    problems::{Problem, ProblemSource},
    Editor,
};

fn severity(value: Option<lsp::DiagnosticSeverity>) -> Severity {
    match value {
        Some(lsp::DiagnosticSeverity::ERROR) => Severity::Error,
        Some(lsp::DiagnosticSeverity::INFORMATION) => Severity::Info,
        Some(lsp::DiagnosticSeverity::HINT) => Severity::Hint,
        _ => Severity::Warning,
    }
}

pub(crate) fn collect(editor: &Editor) -> Vec<Problem> {
    let mut entries = Vec::new();
    let filter = editor.problems.filter;
    let current_path = current_ref!(editor).1.path();
    if filter.source != Some(ProblemSource::Build) {
        for doc in editor.documents() {
            let Some(path) = doc.path() else { continue };
            for diag in doc.diagnostics() {
                let text = doc.text();
                if diag.range.start > diag.range.end || diag.range.end > text.len_chars() {
                    continue;
                }
                let position = |offset| {
                    let line = text.char_to_line(offset);
                    lsp::Position::new(line as u32, (offset - text.line_to_char(line)) as u32)
                };
                entries.push(Problem {
                    path: path.to_owned(),
                    range: lsp::Range::new(position(diag.range.start), position(diag.range.end)),
                    encoding: OffsetEncoding::Utf32,
                    severity: diag.severity(),
                    source: ProblemSource::Lsp,
                    origin: format!(
                        "{:?}/{}",
                        diag.provider,
                        diag.source.as_deref().unwrap_or("")
                    ),
                    message: diag.message.clone(),
                });
            }
        }
        let loader = editor.syn_loader.load();
        for (uri, diagnostics) in &editor.diagnostics {
            let Some(path) = uri.as_path() else { continue };
            if editor.document_by_path(path).is_some() {
                continue;
            }
            for (diag, provider) in diagnostics {
                let Some(ls) = provider
                    .language_server_id()
                    .and_then(|id| editor.language_server_by_id(id))
                else {
                    continue;
                };
                let Some(language) = loader.language_for_filename(path) else {
                    continue;
                };
                let config = loader.language(language).config();
                if !config.language_servers.iter().any(|s| {
                    s.name == ls.name() && s.has_feature(LanguageServerFeature::Diagnostics)
                }) || severity(diag.severity) < config.diagnostic_severity
                {
                    continue;
                }
                entries.push(Problem {
                    path: helix_stdx::path::canonicalize(path),
                    range: diag.range,
                    encoding: ls.offset_encoding(),
                    severity: severity(diag.severity),
                    source: ProblemSource::Lsp,
                    origin: format!("{provider:?}/{}", diag.source.as_deref().unwrap_or("")),
                    message: diag.message.clone(),
                });
            }
        }
    }
    if filter.source != Some(ProblemSource::Lsp) {
        if let Some(run) = &editor.problems.compilation.current {
            entries.extend(run.problems.iter().cloned());
        }
    }
    entries.retain(|p| {
        filter.minimum_severity.is_none_or(|s| p.severity >= s)
            && (!filter.document || Some(p.path.as_path()) == current_path)
    });
    entries.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    entries.dedup();
    entries
}

pub(crate) fn jump(
    editor: &mut Editor,
    problem: &Problem,
    action: Action,
    index: usize,
    total: usize,
) -> Result<()> {
    if editor.document_by_path(&problem.path).is_none() && !problem.path.is_file() {
        bail!("Problem file does not exist: {}", problem.path.display());
    }
    let text = if let Some(doc) = editor.document_by_path(&problem.path) {
        doc.text().clone()
    } else {
        helix_core::Rope::from_str(
            &helix_view::document::read_to_string(
                &mut std::io::BufReader::new(std::fs::File::open(&problem.path)?),
                None,
            )?
            .0,
        )
    };
    let mut location = problem.range;
    let mut notes = Vec::new();
    if problem.source == ProblemSource::Build {
        // Text compiler formats do not identify byte/display/character columns.
        // ASCII without tabs is unambiguous; otherwise guarantee the line only.
        if problem.origin == "compiler" {
            if text
                .get_line(location.start.line as usize)
                .is_some_and(|l| l.chars().any(|c| !c.is_ascii() || c == '\t'))
            {
                location.start.character = 0;
                location.end = location.start;
                notes.push("line only; compiler column encoding is ambiguous");
            }
        }
        for pos in [location.start, location.end] {
            if pos.line as usize >= text.len_lines() {
                bail!("Build position is out of bounds; rebuild before navigating");
            }
            let slice = text.slice(..);
            let line = helix_core::line_ending::line_without_line_ending(&slice, pos.line as usize);
            let units = match problem.encoding {
                OffsetEncoding::Utf8 => line.len_bytes(),
                OffsetEncoding::Utf16 => line.chars().map(char::len_utf16).sum(),
                OffsetEncoding::Utf32 => line.len_chars(),
            };
            if pos.character as usize > units {
                bail!("Build column is out of bounds; rebuild before navigating");
            }
        }
        let run = editor.problems.compilation.current.as_ref();
        let dirty = editor
            .document_by_path(&problem.path)
            .is_some_and(|d| d.is_modified());
        let stale = run.is_some_and(|r| {
            r.unsaved.contains(&problem.path)
                || r.snapshots.get(&problem.path).is_some_and(|stamp| {
                    *stamp != helix_view::problems::FileStamp::read(&problem.path)
                })
                || r.snapshots
                    .get(&problem.path)
                    .and_then(|s| s.as_ref())
                    .and_then(|s| s.modified)
                    .is_some_and(|t| t > r.started)
        });
        if dirty || stale {
            notes.push("stale build location; save and rebuild");
        }
    }
    let range = lsp_range_to_range(&text, location, problem.encoding)
        .context("Problem position is out of bounds; refresh diagnostics or rebuild")?;
    let (view, doc) = current!(editor);
    push_jump(view, doc);
    let id = editor.open(&problem.path, action)?;
    let (view, doc) = current!(editor);
    debug_assert_eq!(id, doc.id());
    doc.set_selection(view.id, Selection::single(range.head, range.anchor));
    align_view(doc, view, Align::Center);
    view.diagnostics_handler
        .immediately_show_diagnostic(doc, view.id);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    editor.problems.anchor = Some((problem.path.clone(), cursor));
    editor.problems.selected = Some(problem.clone());
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!(" ({})", notes.join("; "))
    };
    editor.set_status(format!(
        "[{}/{}] {} {:?}: {}{}",
        index + 1,
        total,
        problem.source.label(),
        problem.severity,
        problem.message,
        notes
    ));
    Ok(())
}

pub(crate) fn navigate(editor: &mut Editor, forward: bool, count: usize) {
    let entries = collect(editor);
    let (view, doc) = current_ref!(editor);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let path = doc.path();
    let line = doc.text().char_to_line(cursor);
    let column = cursor - doc.text().line_to_char(line);
    let continued = editor
        .problems
        .anchor
        .as_ref()
        .is_some_and(|(p, pos)| Some(p.as_path()) == path && *pos == cursor);
    let selected = continued
        .then_some(editor.problems.selected.as_ref())
        .flatten();
    let exact = selected.and_then(|old| {
        entries.iter().position(|p| p == old).or_else(|| {
            let mut matches = entries
                .iter()
                .enumerate()
                .filter(|(_, p)| p.same_identity(old));
            let first = matches.next()?;
            matches.next().is_none().then_some(first.0)
        })
    });
    let count = count.max(1);
    let target = if let Some(index) = exact {
        if forward {
            (index + 1 < entries.len()).then(|| index.saturating_add(count).min(entries.len() - 1))
        } else {
            index.checked_sub(1).map(|_| index.saturating_sub(count))
        }
    } else {
        let indices = entries
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                if let Some(old) = selected {
                    return if forward {
                        p.sort_key() > old.sort_key()
                    } else {
                        p.sort_key() < old.sort_key()
                    };
                }
                let Some(path) = path else { return true };
                let location = (
                    p.path.as_path(),
                    p.range.start.line as usize,
                    p.range.start.character as usize,
                );
                let col = match p.encoding {
                    OffsetEncoding::Utf32 => column,
                    OffsetEncoding::Utf8 => {
                        doc.text().char_to_byte(cursor) - doc.text().line_to_byte(line)
                    }
                    OffsetEncoding::Utf16 => doc
                        .text()
                        .slice(doc.text().line_to_char(line)..cursor)
                        .chars()
                        .map(char::len_utf16)
                        .sum(),
                };
                if forward {
                    location >= (path, line, col)
                } else {
                    location <= (path, line, col)
                }
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let offset = (count - 1).min(indices.len().saturating_sub(1));
        if forward {
            indices.get(offset).copied()
        } else {
            indices.iter().rev().nth(offset).copied()
        }
    };
    match target {
        Some(index) => {
            if let Err(err) = jump(
                editor,
                &entries[index],
                Action::Replace,
                index,
                entries.len(),
            ) {
                editor.set_error(err.to_string());
            }
        }
        None => editor.set_status(if forward {
            "No next problem"
        } else {
            "No previous problem"
        }),
    }
}

pub(crate) fn picker(editor: &mut Editor, compositor: &mut crate::compositor::Compositor) {
    let entries = collect(editor);
    if entries.is_empty() {
        editor.set_status("No problems match the current filters");
        return;
    }
    let picker = Picker::new(
        vec![
            PickerColumn::new("severity", |p: &Problem, _: &()| {
                format!("{:?}", p.severity).into()
            }),
            PickerColumn::new("source", |p: &Problem, _: &()| p.source.label().into()),
            PickerColumn::new("path", |p: &Problem, _: &()| {
                format!(
                    "{}:{}:{}",
                    helix_stdx::path::get_relative_path(&p.path).display(),
                    p.range.start.line + 1,
                    p.range.start.character + 1
                )
                .into()
            }),
            PickerColumn::new("message", |p: &Problem, _: &()| p.message.clone().into()),
        ],
        3,
        entries,
        (),
        move |cx, problem, action| {
            let refreshed = collect(cx.editor);
            let entry = refreshed
                .iter()
                .enumerate()
                .find(|(_, p)| *p == problem)
                .or_else(|| {
                    refreshed
                        .iter()
                        .enumerate()
                        .find(|(_, p)| p.same_identity(problem))
                });
            match entry {
                Some((index, p)) => {
                    if let Err(err) = jump(cx.editor, p, action, index, refreshed.len()) {
                        cx.editor.set_error(err.to_string());
                    }
                }
                None => cx
                    .editor
                    .set_status("Problem is no longer available; refresh the list"),
            }
        },
    )
    .with_preview(|_, p| {
        Some((
            p.path.as_path().into(),
            Some((p.range.start.line as usize, p.range.end.line as usize)),
        ))
    });
    compositor.push(Box::new(overlaid(picker)));
}

pub(super) fn next_problem(cx: &mut Context) {
    let count = cx.count();
    navigate(cx.editor, true, count);
}
pub(super) fn previous_problem(cx: &mut Context) {
    let count = cx.count();
    navigate(cx.editor, false, count);
}
pub(super) fn problems_picker(_cx: &mut Context) {
    job::dispatch_blocking(picker);
}

/// Jump to the compiler error at the cursor's log line in a `*compilation*` buffer.
/// Mirrors Emacs `M-x compile` / `<ret>` on a compiler output line.
pub(super) fn open_problem_at_cursor(cx: &mut Context) {
    let editor = &mut cx.editor;

    // Verify we're in the compilation log buffer.
    let log_doc_id = editor.problems.compilation.log_document;
    let (view, doc) = current_ref!(editor);
    if log_doc_id != Some(doc.id()) {
        editor.set_error("Not in compilation log; use :compile-log to open it");
        return;
    }

    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    let buffer_line = doc.text().char_to_line(cursor);

    // The log buffer header occupies lines 0-4; raw compiler output starts at line 5.
    // Format (from compilation::log_text):
    //   line 0  "*compilation* — {status}"
    //   line 1  "{cwd}"
    //   line 2  "$ {command}"
    //   line 3  "[output truncated…]" or ""
    //   line 4  ""  (blank separator)
    //   line 5+ raw compiler output
    const HEADER_LINES: usize = 5;
    if buffer_line < HEADER_LINES {
        editor.set_error("Cursor is in the log header, not on a compiler output line");
        return;
    }
    let log_line_idx = buffer_line - HEADER_LINES;

    let run = match &editor.problems.compilation.current {
        Some(r) => r,
        None => {
            editor.set_error("No compilation log available");
            return;
        }
    };

    let log_lines: Vec<&str> = run.log.lines().collect();
    let Some(&current_text) = log_lines.get(log_line_idx) else {
        editor.set_error("Log line out of range");
        return;
    };
    let prev_text = log_line_idx
        .checked_sub(1)
        .and_then(|i| log_lines.get(i))
        .copied();
    let next_text = log_lines.get(log_line_idx + 1).copied();
    let cwd = run.cwd.clone();

    // Pass previous line first so the Rust 2-line pattern (error header → arrow line)
    // works when the cursor is on the arrow (`  --> file:line:col`) line.
    let problems = {
        use crate::handlers::compilation::parser::Parser;
        let mut p = Parser::new(cwd.clone());
        if let Some(prev) = prev_text {
            p.push(prev);
        }
        p.push(current_text)
    };

    // Lookahead fallback: cursor is on the Rust error header line; the location
    // is on the following arrow line.
    let problems = if problems.is_empty() {
        if let Some(next) = next_text {
            use crate::handlers::compilation::parser::Parser;
            let mut p = Parser::new(cwd.clone());
            p.push(current_text);
            p.push(next)
        } else {
            problems
        }
    } else {
        problems
    };

    // Python `  File "..."` fallback: cursor is directly on a frame line.
    // The multi-line parser above only emits on the exception line, which may be
    // several lines away; this lets the user jump to any individual frame with <ret>.
    let problems = if problems.is_empty() {
        use crate::handlers::compilation::parser::parse_single_line;
        parse_single_line(&cwd, current_text)
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        problems
    };

    if problems.is_empty() {
        editor.set_error("No error location found on this line");
        return;
    }

    let first = problems.into_iter().next().unwrap();

    // Locate the problem in the run's list for accurate [n/total] status display.
    let (index, problem, total) = {
        let run = editor.problems.compilation.current.as_ref().unwrap();
        let total = run.problems.len().max(1);
        match run
            .problems
            .iter()
            .enumerate()
            .find(|(_, p)| p.path == first.path && p.range.start == first.range.start)
        {
            Some((i, p)) => (i, p.clone(), total),
            None => (0, first, 1),
        }
    };

    if let Err(err) = jump(editor, &problem, Action::Replace, index, total) {
        editor.set_error(err.to_string());
    }
}
