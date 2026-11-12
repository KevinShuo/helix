//! Debounced, cooperative source overview construction. Syntax borrows never leave a job.
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use helix_core::syntax::HighlightEvent;
use helix_event::register_hook;
use helix_view::{
    events::{ConfigDidChange, DocumentDidChange},
    minimap::{
        source_lines, source_sample_count, MinimapSummary, Projection, ProjectionKey, SourceKey,
        SourceRow,
    },
    Document, DocumentId, Editor,
};

use crate::job;

fn key(doc: &Document) -> SourceKey {
    SourceKey {
        version: doc.version(),
        epoch: doc.minimap.epoch,
        tab_width: doc.tab_width(),
    }
}

struct Build {
    doc: DocumentId,
    serial: u64,
    key: SourceKey,
    lines: usize,
    degraded: bool,
    source: Vec<usize>,
    rows: Vec<(usize, SourceRow)>,
    summary: Option<Arc<MinimapSummary>>,
    projections: Vec<Projection>,
    cursor: usize,
}

/// Called after layout, so invisible splits do no work. At most 32MiB of source summaries.
pub(crate) fn request(editor: &mut Editor) {
    let mut visible: HashMap<DocumentId, Vec<ProjectionKey>> = HashMap::new();
    for (view, _) in editor.tree.views() {
        let doc = &editor.documents[&view.doc];
        if let Some(content) = view.minimap_layout(doc).content {
            let k = ProjectionKey {
                width: content.width,
                height: content.height,
                render: editor.config().minimap.render,
            };
            let keys = visible.entry(view.doc).or_default();
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    let budget = (32 * 1024 * 1024 / visible.len().max(1)).min(8 * 1024 * 1024);
    for doc in editor.documents.values_mut() {
        let Some(keys) = visible.remove(&doc.id()) else {
            if doc.minimap.summary.is_some() || doc.minimap.pending {
                doc.minimap.invalidate();
            }
            continue;
        };
        let current = key(doc);
        if doc.minimap.source_budget != budget
            || doc
                .minimap
                .summary
                .as_ref()
                .is_some_and(|s| s.key != current)
        {
            doc.minimap.invalidate();
        }
        doc.minimap.source_budget = budget;
        if doc.minimap.requested == keys
            && (doc.minimap.pending || !doc.minimap.projections.is_empty())
        {
            continue;
        }
        doc.minimap.serial = doc.minimap.serial.wrapping_add(1);
        doc.minimap.pending = true;
        doc.minimap.requested = keys.clone();
        let lines = doc.text().len_lines();
        let degraded = lines > 50_000
            || doc.text().len_bytes() > 4 * 1024 * 1024
            || lines.saturating_mul(std::mem::size_of::<(usize, SourceRow)>()) > budget;
        let height = keys.iter().map(|k| k.height).max().unwrap_or(1).min(
            (budget / (32 * std::mem::size_of::<(usize, SourceRow)>())).clamp(1, u16::MAX as usize)
                as u16,
        );
        let samples = source_sample_count(lines, degraded, height);
        let summary = doc
            .minimap
            .summary
            .clone()
            .filter(|s| s.degraded == degraded && s.rows.len() >= samples);
        let source = if summary.is_none() {
            source_lines(lines, degraded, height)
        } else {
            Vec::new()
        };
        let rows = Vec::with_capacity(source.len());
        let build = Build {
            doc: doc.id(),
            serial: doc.minimap.serial,
            key: key(doc),
            lines,
            degraded,
            source,
            rows,
            summary,
            cursor: 0,
            projections: keys
                .into_iter()
                .map(|k| Projection::new(lines, k))
                .collect(),
        };
        let handle = doc.minimap.controller.restart();
        tokio::spawn(async move {
            helix_event::cancelable_future(run(build), handle).await;
        });
    }
}

async fn run(mut build: Build) {
    tokio::time::sleep(Duration::from_millis(150)).await;
    loop {
        let (tx, rx) = tokio::sync::oneshot::channel();
        job::dispatch(move |editor, _| {
            let needed = editor.tree.views().any(|(view, _)| {
                view.doc == build.doc
                    && view
                        .minimap_layout(&editor.documents[&view.doc])
                        .map
                        .is_some()
            });
            let Some(doc) = editor.document(build.doc) else {
                return;
            };
            if !needed || doc.minimap.serial != build.serial || key(doc) != build.key {
                return;
            }
            let started = Instant::now();
            if build.summary.is_none() {
                let loader = editor.syn_loader.load();
                let text = doc.text().slice(..);
                let mut bytes = 0;
                let limit = (build.cursor + 128).min(build.source.len());
                let first_byte = text.line_to_byte(build.source[build.cursor]);
                let mut end = limit;
                let mut last_byte = first_byte;
                let mut colorable = !build.degraded;
                if colorable {
                    // One bounded query per slice instead of seeking the syntax tree per line.
                    end = build.cursor;
                    for index in build.cursor..limit {
                        let line = build.source[index];
                        let start = text.line_to_byte(line);
                        let len = text.line(line).len_bytes();
                        if len > 8192 || start + len - first_byte > 32 * 1024 {
                            if index == build.cursor {
                                end += 1;
                                colorable = false;
                            }
                            break;
                        }
                        end = index + 1;
                        last_byte = start + len;
                    }
                }
                let mut highlighter = (colorable && first_byte < last_byte)
                    .then(|| {
                        doc.syntax().map(|syntax| {
                            syntax.highlighter(text, &loader, first_byte as u32..last_byte as u32)
                        })
                    })
                    .flatten();
                let mut color = None;
                while build.cursor < end {
                    let line = build.source[build.cursor];
                    let source = text.line(line);
                    let byte_start = text.line_to_byte(line);
                    let row = SourceRow::from_line(source, build.key.tab_width, |offset| {
                        if let Some(h) = &mut highlighter {
                            while h.next_event_offset() != u32::MAX
                                && h.next_event_offset() as usize <= byte_start + offset
                            {
                                let (event, highlights) = h.advance();
                                if event == HighlightEvent::Refresh {
                                    color = None;
                                }
                                for highlight in highlights {
                                    color = Some(highlight);
                                }
                            }
                        }
                        color
                    });
                    bytes += source.len_bytes().min(16 * 1024);
                    build.rows.push((line, row));
                    build.cursor += 1;
                    if bytes >= 32 * 1024 || started.elapsed() >= Duration::from_millis(3) {
                        break;
                    }
                }
                if build.cursor == build.source.len() {
                    build.summary = Some(Arc::new(MinimapSummary {
                        key: build.key,
                        lines: build.lines,
                        degraded: build.degraded,
                        rows: std::mem::take(&mut build.rows),
                    }));
                    build.cursor = 0;
                }
            } else {
                let summary = build.summary.as_ref().unwrap();
                let end = (build.cursor + 128).min(summary.rows.len());
                while build.cursor < end {
                    let (line, row) = &summary.rows[build.cursor];
                    for projection in &mut build.projections {
                        projection.add_row(*line, row);
                    }
                    build.cursor += 1;
                    if started.elapsed() >= Duration::from_millis(3) {
                        break;
                    }
                }
                if build.cursor == summary.rows.len() {
                    let state = &mut editor.document_mut(build.doc).unwrap().minimap;
                    state.summary = build.summary;
                    state.projections = build.projections;
                    state.pending = false;
                    helix_event::request_redraw();
                    return;
                }
            }
            let _ = tx.send(build);
        })
        .await;
        match rx.await {
            Ok(next) => build = next,
            Err(_) => break,
        }
        // Give terminal input a chance even when many documents need a summary.
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

pub(super) fn register_hooks() {
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        event.doc.minimap.invalidate();
        Ok(())
    });
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        for doc in event.editor.documents.values_mut() {
            doc.minimap.invalidate();
        }
        Ok(())
    });
}
