# AI Function Documentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add native `:ai-doc` / `:ai-doc-cancel` for Chinese reST function documentation in C, C++ and Python.

**Architecture:** Use the host syntax tree to construct a fixed insertion target, a bounded complete-function request, and editor-owned reST formatting from validated semantic JSON. Keep one cancelable editor operation and a modal preview; share HTTP transport and local model lifecycle with existing completion.

**Tech Stack:** Rust 1.90, Tokio, reqwest, serde, tree-sitter, Helix compositor/Transaction.

**Spec:** `docs/superpowers/specs/2026-11-14-ai-function-doc-design.md` (approved by user on 2026-11-14).

## Global Constraints

- C/C++ function definitions and multiline Python definitions; Chinese reST by default.
- Existing documentation skips generation. No model-chosen edits or cross-file reads.
- Preview is nonediting; acceptance is one undoable insertion. Any document edit invalidates results.
- Preserve automatic and ordinary completion behavior; allow documentation while AI completion is disabled.
- Keep the current checkout and work on `codex/ai-function-doc`; leave `.idea/` alone.

## Review Focus

- Host syntax vs injected strings, EOF and unsupported anonymous callable nesting.
- Decorators, receiver parameters, multiline templates and return declarators.
- Source delimiters, JSON truncation, duplicate/missing fields and malformed model responses.
- Focus/config/version changes, modal result deferral and preview key isolation.
- One shared local process, disabled completion, cancellation and request resource release.

## Task 1: Configuration and shared model transport

**Files:** `helix-view/src/ai_doc.rs`, `helix-view/src/{lib,editor}.rs`, `helix-term/src/handlers/ai.rs`, existing AI client/server and application.
**Interfaces:** `AiDocConfig` (disabled, zh-CN/rest, 30000ms, 768 tokens); `Config::ai_model_config()` returns existing model config with enabled OR; shared `chat(config, body, timeout)` returns content after envelope validation.
- [x] Add configuration tests asserting defaults, independent enablement and unknown styles rejected; run RED.
- [x] Add config/effective model settings and extract transport preserving completion response semantics.
- [x] Run config and existing AI completion unit tests GREEN; commit scoped changes.

## Task 2: Function target and reST formatting

**Files:** `helix-term/src/handlers/ai_doc/{target,format}.rs` and unit tests.
**Interfaces:** `target::locate(source: RopeSlice, syntax: &Syntax, cursor: usize, language: &str) -> Result<Target>`; owned `Target` includes source, range, insert char, indent/newline, parameter/type metadata and return/template/exception facts. `format::render(target: &Target, json: &str) -> Result<String>` creates only insertable documentation.
- [x] Add parse-based tests for all three languages, nested/decorated/async/templates/constructors, existing docs, EOF, injection and unsupported forms; run RED.
- [x] Implement host-tree target extraction and structural comment detection; implement strict semantic JSON and Chinese reST formatter.
- [x] Test escaping, malformed/duplicate/unknown fields, parameter order and insertion source preservation; run GREEN and commit.

## Task 3: Complete-function context and model protocol

**Files:** `helix-term/src/handlers/ai_doc/client.rs`, common transport tests.
**Interfaces:** `generate(model: &AiCompletionConfig, config: &AiDocConfig, target: &Target) -> Result<String>` returns formatted documentation; request keeps the complete target and drops optional supporting context first.
- [x] Add fake HTTP tests for complete source, Chinese JSON request, output caps, failure/truncation and over-budget refusal; run RED.
- [x] Implement independent prompt/schema, token budget with conservative byte bound, readiness and shared transport.
- [x] Run protocol/format tests GREEN and commit.

## Task 4: Editor operation, native commands and preview

**Files:** `helix-view/src/ai_doc.rs`, editor state, `helix-term/src/handlers/ai_doc.rs`, `helix-term/src/ui/ai_doc.rs`, compositor/application and typed command registration.
**Interfaces:** `start(cx)`, `cancel(editor)`, `show_ready(compositor, cx)`, operation snapshot validated by document/view/version/config and cancellation handle. Modal preview applies one Transaction and maps selections.
- [x] Add integration tests for command generation/preview/accept/undo/redo/cancel and no preview mutation; run RED.
- [x] Implement operation state, event invalidation, pause automatic AI, safe modal deferral and explicit preview key consumption.
- [x] Add tests for movement, edits/focus/config/repeated requests, ordinary prompt interaction, retry and disabled automatic completion; run GREEN and commit.

## Task 5: Validation, documentation and review

**Files:** user guide/config command references and `docs/superpowers/2026-11-14-ai-function-doc-validation.md`.
- [x] Run focused and existing completion regressions, full relevant crate tests, workspace check and formatting.
- [x] Probe currently configured local model when available with two functions per language; record content, latency and limitations without logging credentials.
- [x] Build and exercise native terminal preview; use independent final code review and fix important findings with regression tests.
- [x] Record verification evidence, final usage/config and remaining limitations; commit reviewed feature.

## Execution ledger

Ruling: User explicitly approved implementation; execute the plan inline without another approval round. Developer checkout preference takes precedence over creating a new worktree. The current checkout is reused on a feature branch.

Pre-flight: Tasks 2/3/4 share owned Target metadata and formatted insertion text; Task 1's effective model configuration is used by both service lifecycle and both request types. Editor owns operation/cancellation state; formatter has no editor/network dependencies.

Validation evidence and baseline failures: `docs/superpowers/2026-11-14-ai-function-doc-validation.md`. The tasks are delivered in one reviewed feature commit because the shared editor state/model interfaces span the original task boundaries.
