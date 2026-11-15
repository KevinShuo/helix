# AI Merge Conflict Block Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Generate, preview and accept an AI suggestion for the currently selected native merge conflict block.

**Architecture:** Reuse the shared AI transport/provider and native merge result Document. A version-bound Operation owns one pending request/candidate; acceptance creates one target-range Transaction without bypassing Review / Apply.

**Tech Stack:** Existing Rust, Tokio, serde_json, Helix Document/Transaction and TUI; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-15-ai-merge-conflict-block-design.md`

## Global Constraints

- Current conflict block only; full required source input or a budget error.
- Independent `[editor.ai-merge]`: enabled=false, timeout-ms=60000, max-tokens=1536, max-input-bytes=16384.
- Reuse model connection, lifecycle and bounded HTTP transport; no new dependencies.
- Accept changes only the target draft range and requires Review; existing Apply owns write/stage and original BOM/newlines.
- Work on the current checkout's feature branch; preserve unrelated untracked files and installed release executable.

## Review Focus

- Empty code is a legitimate deletion; status/JSON validation must not confuse it with no candidate (Task 1).
- Neighbouring and manually edited conflict ranges remain untouched, including Unicode offsets (Task 2).
- Delayed replies after block/session/config changes cannot reopen a preview or mutate a draft (Task 2).
- Generated candidates must not bypass manual Review or write the index/worktree before Apply (Task 2).
- Narrow/tiny terminals keep acceptance, cancellation and uncertainty visible (Task 3).

---

### Task 1: Bounded model protocol and independent configuration

**Files:** Create `helix-view/src/ai_merge.rs`, `helix-term/src/handlers/ai_merge.rs`, `helix-term/src/handlers/ai_merge/client.rs` and client tests; modify module registries and Editor config/default/provider enablement.

**Interfaces:** Produce `AiMergeConfig`; `Target { block, path, language, base, current, incoming, result, context, intent, range }`; `Suggestion { code: Option<String>, explanation: String }`; `client::request(model, config, target) -> Result<Value>`, `client::parse(response, block) -> Result<Suggestion>`, `client::generate(...) -> Result<Suggestion>`.

- [x] Write failing tests: complete required texts survive JSON input; oversize required input is refused; optional context is omitted before required text; strict response ID/status/fields, finish reason and tool-call validation; empty deletion accepted; CRLF normalized; controls/markers rejected.
- [x] Run `cargo test --profile integration --locked -p helix-term --lib ai_merge` and observe missing implementation failure.
- [x] Implement config and pure request/parse helpers; shared server readiness and transport for generation. Defaults and shared provider activation covered by `helix-view` tests.
- [x] Run protocol tests and `cargo test --profile integration --locked -p helix-view --lib ai_merge`; expected zero failures.
- [x] Commit task changes and record results.

### Task 2: Version-bound native command, preview, and acceptance

**Files:** Modify handler/view AI merge modules; create `helix-term/src/ui/ai_merge.rs` and `helix-term/tests/test/ai_merge.rs`; modify Editor/Document/handlers/application/UI registries, typed commands, git merge actions/menu/render and shared AI scheduling guards.

**Interfaces:** Consume Task 1 protocol; produce `Operation::is_valid(&Editor)`, `start(editor, block, intent)`, `cancel(editor)`, `retry(editor)`, `accept(editor)`, `refresh(editor, compositor)`. Merge entry captures the selected block when the command is submitted. Cancellation hooks invalidate pending requests.

- [x] Write a failing real-repository/local-HTTP integration test for `:ai-merge`, unchanged draft before Enter, target-only acceptance, NeedsReview, undo/redo and unchanged disk/index.
- [x] Run `cargo integration-test ai_merge -- --test-threads=1`, expected unknown command/no HTTP request before implementation.
- [x] Implement Operation snapshot, cancellation and validity guards; budgeted complete target and optional nearby/header context; modal diff preview; Enter/retry/cancel; one Transaction acceptance; toolbar/menu/typed command entries and model scheduling guards.
- [x] Add delayed-request tests for editing, switching block/file/session, config invalidation and explicit cancel; refusal/invalid output and staged/whole-file/trust guards; real Review/Apply and CRLF/BOM tests. Run each new test while the corresponding behavior is absent before finishing it.
- [x] Run `cargo integration-test ai_merge -- --test-threads=1` and existing `ai_doc`, `ai_commit`, `git_conflicts` suites; expected zero failures.
- [x] Commit task changes and record results.

### Task 3: Terminal presentation, documentation and whole-branch verification

**Files:** Test/adjust `helix-term/src/ui/ai_merge.rs`; update `book/src/configuration.md` and command docs; create `docs/superpowers/2026-11-15-ai-merge-conflict-block-validation.md`.

**Interfaces:** Consume Task 2 UI and command contract; no new runtime dependencies or Git operations.

- [x] Add rendering tests at 160×40, 80×24, and tiny areas, including needs-manual with no acceptance. Observe the intended presentation failure before any adjustment.
- [x] Document enabled config, model sharing, exact actions, required Review and cancellation/size boundaries.
- [x] Run formatting, workspace tests, full integration tests, and no-default-features build check; name and investigate any baseline failures separately.
- [x] Run native terminal smoke using a development binary and isolated fixtures. If an existing local model is available, exercise representative merge scenarios without logging credentials.
- [x] Request one fresh whole-branch review, address important findings with reproducing tests, write final evidence and commit. Keep feature branch available for user review.

### Task 4: Model routing and requested deployment

**Authorization:** The user selected 27B for Merge and 9B for completion, and requested merging/pushing master and updating the local installation.

**Compatibility:** The installed executable is `1c5ac7fbe` from `codex/ai-test-generation`, which contains the provider registry, on-demand model leases and native test workbench. Integrate that existing branch before deployment so the update retains installed capabilities and accepts the current user configuration.

- [x] Resolve the prerequisite merge while retaining both feature sets.
- [x] Add Merge to the per-feature provider registry with optional `provider` / `local-provider`, legacy fallback and independent activation. Verify 9B remains resident and 27B is on demand.
- [x] Route Merge through the existing cancellable lease and shared transport; retain local delete/modify refusal before model loading and account for configured reasoning tokens.
- [x] Test configuration, target invalidation, generation exclusion and model restoration; run workspace/integration checks and obtain a focused review.

**Authorized release procedure** (results recorded alongside the private installation backup):

- Integrate into latest master, push origin/master, build and install the exact revision with runtime files. Back up the existing executable, runtime and user configuration first.
- Configure local Merge to `local-27b-direct` and completion to `local`; verify the installed executable and native 27B request, preserving active user processes and unrelated configuration.
