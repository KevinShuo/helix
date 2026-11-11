# Local AI Inline Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve Helix completion and add automatic local-model ghost text accepted in full with Shift+Right.

**Architecture:** Keep the suggestion and cancellation controller on the document, scoped to one view and document version. An asynchronous HTTP task collects bounded prefix/suffix context; virtual annotations display validated results without editing the buffer. Intercept the mapped accept command before the ordinary completion menu.

**Tech Stack:** Rust, Tokio, reqwest, serde_json, existing Helix annotation and event APIs.

**Spec:** `docs/superpowers/specs/2026-11-11-ai-inline-completion-design.md`

## Global Constraints

- Preserve the existing LSP, path, word completion and Tab/Enter behavior.
- Automatically show grey suggestions; `S-right` accepts the entire visible suggestion.
- Default off; debounce 350ms; use configurable local OpenAI-compatible chat endpoint.
- Single caret only; suspend in selections and active snippets.
- Read credentials from environment; never include them in source or logs.
- No push, remote PR, or remote mutation. User fork: https://github.com/KevinShuo/helix (reference only).
- Work in the suitable current checkout on local branch `codex/ai-inline-completion`.

## Review Focus

- Temporary popup previews must never become AI context or accidentally be accepted with the AI suggestion (Task 3).
- A response for an obsolete caret/view/version must never be displayed or inserted (Tasks 1, 2, 3).
- Virtual multiline/Unicode text must preserve caret-before-ghost and mouse mapping (Task 1).
- Custom keybindings, dot repeat and macros must not initiate fresh inference on replay (Task 3).
- A failing or unreachable model must leave ordinary editing usable and never leak credentials (Tasks 2, 4).

### Task 1: Suggestion state, configuration, and virtual text

**Files:** create `helix-view/src/ai_completion.rs`; modify `helix-view/src/lib.rs`, `editor.rs`, `document.rs`, `view.rs`; extend `helix-core/src/text_annotations.rs`, `position.rs`; update rendering in `helix-term/src/ui/document.rs`.

**Interfaces:** `AiCompletionConfig` (enabled, endpoint, model, api_key_env, debounce_ms, timeout_ms, max_tokens, prefix_chars, suffix_chars); `AiCompletion { view, version, annotation }`; `AiCompletion::is_valid(&Document, ViewId) -> bool`. `Document` owns suggestion and a `TaskController`. `TextAnnotations` exposes a ghost anchor so visual-position calculations stop before ghost text.

- [ ] Write failing config and virtual-position tests: non-default configuration deserializes; inserting grey text leaves real text unchanged; caret stays before multiline Unicode suggestions; clicks after suggestions map to real suffix.
- [ ] Run targeted `cargo test -p helix-view ai_completion` / `cargo test -p helix-core ghost`; observe missing behavior.
- [ ] Implement state/config, virtual text with dedicated grey fallback, and consistent coordinate mapping.
- [ ] Run targeted tests and `cargo check -p helix-term`; expected all pass.
- [ ] Commit this testable layer locally.

### Task 2: Bounded asynchronous local-model requests

**Files:** create `helix-term/src/handlers/ai_completion.rs` and `ai_completion/client.rs`; update `helix-term/Cargo.toml`, `handlers.rs`, lockfile.

**Interfaces:** `trigger(&mut Editor)`, `clear(&mut Editor)`, `register_hooks()`; client `complete(&AiCompletionConfig, &CompletionContext) -> Result<Option<String>>`; context contains filename, language, prefix, suffix.

- [ ] Write failing HTTP-boundary tests using a local TCP fixture: request has prefix/suffix and thinking disabled; preserve indentation; strip enclosing fences and duplicate closing suffix; reject tool calls/oversized/error responses.
- [ ] Run `cargo test -p helix-term ai_completion`; observe expected failures.
- [ ] Add cancellable debounce/request with timeout, bounded response size, no redirects, no model-owned tools, and sanitized diagnostics. Invalidation covers document/selection/focus/mode/config changes; commit responses only when handle and snapshot are still valid.
- [ ] Run HTTP tests including cancellation, timeout and superseded requests; expected all pass.
- [ ] Commit locally.

### Task 3: Acceptance and existing completion coexistence

**Files:** modify `helix-term/src/commands.rs`, `keymap/default.rs`, `ui/editor.rs`; add `helix-term/tests/test/ai_completion.rs`, register it in `tests/integration.rs`.

**Interfaces:** `ai_completion_accept(&mut commands::Context)`; `EditorView` handles the mapped accept command before popup events and records accepted text as an edit for repeat.

- [ ] Write editor integration tests for automatic ghost display without buffer mutation, `S-right` whole multiline accept, undo, original menu coexistence, no-ghost acceptance no-op, custom remap, stale response rejection and repeat.
- [ ] Run `cargo integration-test ai_completion`; verify failures before implementation.
- [ ] Wire command/default mapping and pre-menu dispatch; acceptance uses one transaction and history boundary, cancels generation and clears popup without accepting its selected candidate.
- [ ] Ensure repeat/macro paths reproduce literal accepted text without model requests; test the actual replay behavior.
- [ ] Run targeted integration tests plus existing insertion/completion tests; expected all pass.
- [ ] Commit locally.

### Task 4: Local setup, validation and documentation

**Files:** update `book/src/configuration.md`, `book/src/themes.md`, generated keymap/command docs as needed; add `contrib/helix-junie.ps1` and example AI config if useful.

- [ ] Document configurable provider, grey style, acceptance/remapping and supported scope; provide a launcher that reads existing Junie credentials into this process only and runs the locally built editor.
- [ ] Start existing local Junie service for a bounded synthetic completion; inspect model routes and confirm per-request thinking override and latency. Do not change Junie's persistent inference settings.
- [ ] Run `cargo fmt --check`, relevant unit suites, integration suites and a release build; report every failure or unavailable environment check accurately.
- [ ] Run a fresh whole-change code review, fix important findings with regression coverage, and verify again where changes require it.
- [ ] Deliver local binary/config/launcher instructions and actual validation results. Do not push.

## Execution notes

The user approved the design and explicitly requested implementation. Continue inline without requesting another plan approval. Native Windows PowerShell bookkeeping substitutes for Bash-only skill helper scripts when needed; keep evidence in the plan-specific ignored ledger.
