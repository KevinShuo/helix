# Problem Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Navigate live diagnostics across files, then navigate errors and warnings from asynchronous builds.

**Architecture:** A shared problem model stores persistent filters and navigation identity. Live LSP entries are collected per command, while a cancellable runner streams logs and parser results into separate build state through editor callbacks.

**Tech Stack:** Rust, existing Helix picker, Tokio, serde_json, existing regex support.

**Spec:** `docs/superpowers/specs/2026-11-15-problem-navigation-design.md`

## Global Constraints

- Preserve existing diagnostics commands and the user's uncommitted changes.
- Use current checkout `I:/DEV/helix`, branch `codex/ai-test-generation`; do not commit mixed pre-existing work or install/publish the editor.
- Default filters: all sources, errors and warnings, workspace. No automatic navigation.
- No new dependencies. All child processes belong to the build operation.
- Retained log 1 MiB, line input 64 KiB, located entries 10,000.

## Review Focus

- Identical start positions and removed current diagnostics must still permit progress.
- Current document character ranges and LSP UTF-16 ranges must not be confused.
- Windows paths with drive colons and spaces must survive parsing and shell quoting.
- Output fragments and simultaneous stdout/stderr must not corrupt parser state.
- Late callbacks after cancellation/recompile must not overwrite a new run.

### Task 1: Cross-file navigation and filters

**Files:** Create `helix-view/src/problems.rs`, `helix-term/src/commands/problems.rs`,
`helix-term/tests/test/problems.rs`; modify editor/lib registration, commands,
typed commands, keymap, integration registration and `book/src/keymap.md`.

**Interfaces:** Produce `Problem` (with LSP range + explicit encoding), `ProblemSource`,
`ProblemFilter`, `ProblemState` in helix-view; `collect`, `navigate`, `picker` in
commands/problems. Build state is consumed by task 3. Store identity plus cursor
anchor so repeated commands can reach multiple messages at one location.

- [x] Write editor integration tests for next/previous across files and severity/source/scope selection; run and observe missing-command failures.
- [x] Implement the model, collection using document diagnostics and raw unopened-file LSP diagnostics, picker and navigation commands/keymaps.
- [x] Add same-position/update/removal/UTF-16/fold/missing-file tests and verify behavior.
- [x] Run `cargo test -p helix-term --features integration --test integration test::problems -j 1 -- --test-threads=1`; expect all passing.

### Task 2: Compiler output parsing

**Files:** Create `helix-term/src/handlers/compilation/parser.rs` and its tests;
register compilation handler module. Extend `helix-view/src/problems.rs` for build entries.

**Interfaces:** `Parser::new(cwd: PathBuf)`, `Parser::push(&mut self, line: &str) -> Vec<Problem>`;
parse a completed line independently per pipe and keep Rust pending-message context.
Emit absolute paths and explicit column encoding for navigation.

- [x] Write parser tests for GCC/Clang, MSVC, Cargo JSON, Rust multiline diagnostics, Windows/Unicode paths, ANSI and malformed locations; run before implementation.
- [x] Implement parser with validated positive line/column coordinates and bounded messages.
- [x] Run `cargo test -p helix-term --lib handlers::compilation::parser -j 1`; expect all passing.

### Task 3: Async compilation, log and navigation

**Files:** Create `helix-term/src/handlers/compilation.rs`, `compilation/runner.rs`;
modify typed commands and problem model/commands. Document usage in `book/src/configuration.md`.

**Interfaces:** Handler `start(editor, command)`, `recompile(editor)`,
`cancel(editor)`, `show_log(editor)`; runner streams tagged batches through
`job::dispatch`. Build state owns run ID, command, cwd, log, problems, status,
TaskController and log document ID. Navigation consumes build entries alongside LSP.

- [x] Write fixture-process/editor tests for streaming and failure, location jump, cancellation/recompile, bounded output and log focus; observe missing behavior.
- [x] Implement cancellable child ownership, concurrent bounded pipe reading, parser callbacks and a read-only log split.
- [x] Validate build snapshots on navigation; retain logs for unlocated failures; preserve problem filters and cursor focus during output.
- [x] Run focused problem and compilation suites with `-j 1`; expect all passing.

### Task 4: Verification and review

- [x] Run rustfmt on touched Rust files and `git diff --check`.
- [x] Run relevant library and integration suites, `cargo check -p helix-term -j 1`.
- [x] Independent reviewer reads new files and focused diff against spec; address important findings with reproducing tests.
- [x] Record exact results and any limitations in `docs/superpowers/2026-11-15-problem-navigation-validation.md` and provide command examples.

## Execution ledger

User approved approach and ordered implementation. Proceed inline without another
approval round. Existing uncommitted feature work makes a fresh checkout omit
required interfaces; preserve the suitable current checkout. No commits containing
mixed existing changes. Record verification results here as tasks progress.

Completed 2026-11-15: 9/9 focused integration tests, 157 terminal unit tests
(12 existing opt-in tests ignored), 88 view unit tests, cargo check, rustfmt check
and git diff --check passed. Evidence and review fixes are recorded in
`docs/superpowers/2026-11-15-problem-navigation-validation.md`.

Ruling: retain the suitable checkout and leave changes uncommitted because it
contains the user's existing AI work. No installed executable or remote changed.
Ruling: retain existing e-key bindings and use q/Space E; use conservative line
navigation for ambiguous text compiler columns, documented explicitly.
