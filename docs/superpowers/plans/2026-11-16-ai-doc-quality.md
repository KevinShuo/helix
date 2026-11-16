# AI Doc Quality Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement task by task.

**Goal:** Improve factual documentation while retaining one model call on the successful path.

**Architecture:** Preserve complete in-memory target source. Extract callable facts and bounded, relevant same-file declarations; request evidence for every emitted description. Validate source quotes, required returns, and existing structural constraints before preview. Repair a complete but invalid response once under one total timeout and one model lease.

**Tech Stack:** Existing Rust, tree-house, serde, tokio and mock HTTP integration tests; no new dependencies.

**Spec:** User requested highest practical quality, selected balanced speed/accuracy, and authorized direct implementation without design questions on 2026-11-16.

## Global Constraints

- Normal success uses one request; at most one repair, no retry for transport failure, tool calls or truncated completion.
- Use current buffer source; context contains complete blocks, respects scope and excludes unrelated functions. Do not read additional project files.
- Source remains untrusted data; model never controls edits. Preserve preview, cancellation, undo, provider selection and shared-model lifecycle.
- Evidence proves source presence, not natural-language correctness; do not claim guaranteed accuracy.
- Reuse current checkout as required by desktop workspace guidance; preserve existing untracked files.

## Review Focus

- Async generators, misleading None annotations, nested callables and parenthesized None.
- Relevant helpers/constants after the target; same-named members in unrelated classes.
- Evidence injection, absent/extra evidence, quotations copied only from model output.
- Repair context overflow, repeated invalid output, cancellation and combined timeout.
- Existing ai-test target extraction must keep its context and identity behavior.

### Task 1: Source facts and relevant context

**Files:** helix-view/src/ai_doc.rs; helix-term/src/handlers/ai_doc/target.rs; source_context.rs; target_tests.rs.

**Interfaces:** Target gains callable-kind and value-return facts plus separate related-source blocks, leaving supporting declarations consumed by ai-test unchanged.

- [x] Add failing tests for generator/None, parenthesized None, and relevant same-file constants/helpers/member context with unrelated-scope exclusion.
- [x] Run ai_doc tests and observe the new assertions fail.
- [x] Implement AST facts and bounded scope-aware context selection; rerun tests.

### Task 2: Grounded output and bounded repair

**Files:** helix-term/src/handlers/ai_doc/{client.rs,format.rs}; client/tests.rs; format/tests.rs; helix-term/tests/test/ai_doc.rs.

**Interfaces:** Production render requires evidence entries {field, quote} for summary/details/returns/param:name/template:name/raises:name, each checked against source sent in that request. Request JSON includes complete source, callable facts and complete context blocks selected within the combined input/output budget.

- [x] Add failing evidence and missing-return tests; HTTP regressions for repair success, retry cap, truncation, and cancellation.
- [x] Observe RED, implement grounded render and JSON prompts; preserve all existing safety checks.
- [x] Implement one repair with diagnostic and prior JSON, one model lease, one total generation timeout, and no target truncation.
- [x] Run targeted tests then package suite and integration AI tests.

### Task 3: Quality evaluation and delivery

**Files:** client/evaluation.rs; book/src/editor.md; docs/superpowers/reports/2026-11-16-ai-doc-quality-validation.md.

- [x] Expand opt-in fixtures beyond six simple cases: boundary ordering, caught errors, generators, mutations, helpers/constants, opaque dependencies, prompt injection and misleading names.
- [x] Run production pipeline on an available existing local model using synthetic inputs; record results, latency, scope and limitations without extrapolating to overall accuracy.
- [x] Review final diff; run formatting, package/integration checks, and document exact results.

## Progress

- Baseline: 21 ai_doc tests passed, 2 explicit live-model tests ignored.
- Execution: user authorized direct implementation; no further workflow approvals needed.
- 2026-11-17 checkpoint: user requested saving and pushing work for continuation tomorrow. Implementation and automated checks are saved; further semantic quality tuning and release installation remain. See `../reports/2026-11-16-ai-doc-quality-validation.md` for exact results and the next steps.
