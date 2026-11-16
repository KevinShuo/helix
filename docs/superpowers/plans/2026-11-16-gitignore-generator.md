# Gitignore Generator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Provide offline project detection, selectable built-in ignore templates, and an editable unsaved `.gitignore` draft.

**Architecture:** A pure catalog/detection/generation module feeds a dedicated asynchronous modal. Both command entry points share document reuse and draft insertion logic; ordinary save remains responsible for disk writes.

**Tech Stack:** Rust 1.90, existing Tokio, Helix compositor, tui and integration harness; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-16-gitignore-generator-design.md` (approved 2026-11-16).

## Global Constraints

- Root-only detection; 4096-entry cap for extension matching; fixed-name checks independent of enumeration.
- Built-in templates through `include_str!`; no network, AI, project code execution or Git executable in production.
- `:gitignore`, static `gitignore`, `Space v i`; no required Git feature.
- Existing buffers/files preserved; no writes before ordinary save; stable ordered blocks and no line deduplication.
- Reuse this existing `codex/` checkout and preserve unrelated local edits; commit only feature paths/hunks.

## Review Focus

- Same-path unsaved/empty buffers must win over generation; integration reuse tests in Task 2.
- A target created while selecting must keep its contents; race test in Task 2.
- Cancellation followed by a late callback must not reopen or change documents; session test in Task 2.
- Nested Git/worktree markers and non-Git roots must resolve without cwd confusion; filesystem tests in Task 1.
- Filtered-out selections and zero-result searches must keep the complete selected set; modal tests in Task 2.

### Task 1: Catalog, detection and ordered generation

**Files:** Create `helix-term/src/gitignore.rs`, `helix-term/src/gitignore/{catalog,detect,tests}.rs`, `helix-term/src/gitignore/templates/*`; modify `helix-term/src/lib.rs`.

**Interfaces:** `Template { id, name, category, description, aliases, text }`, `TEMPLATES: &[Template]`; `Project { target: PathBuf, is_git: bool, detected: BTreeMap<&'static str, Vec<String>>, warnings: Vec<String> }`; `detect(start: &Path) -> anyhow::Result<Project>`; `generate(ids: &BTreeSet<&str>) -> anyhow::Result<String>`.

- [x] Write tests for root-only and combined detection, nested markers/worktree files, all marker groups, enumeration cap, symlink exclusion, stable ordered generation and preserved exceptions.
- [x] Run `cargo test -p helix-term --lib gitignore` and observe the missing module/API failure.
- [x] Implement the catalog (19 templates), metadata-only detector and deterministic generator. Use original concise rules and document provenance; no copied upstream text needed.
- [x] Run the unit tests, including `git check-ignore` behavior tests for outputs, lockfiles, environment examples and source directories.
- [x] Commit only Task 1 paths after green tests.

### Task 2: Modal, entry points and document handling

**Files:** Create `helix-term/src/ui/gitignore.rs`, `helix-term/src/ui/gitignore/{selection,tests}.rs`, `helix-term/tests/test/commands/gitignore.rs`; modify `ui/mod.rs`, `commands.rs`, `commands/typed.rs`, `keymap/default.rs`, `tests/test/commands.rs`.

**Interfaces:** Consumes Task 1 `Project`, `TEMPLATES`, `detect`, `generate`; produces `ui::gitignore::open(&mut commands::Context)` and `open_typed(&mut compositor::Context)` plus a modal identified by `gitignore-generator`.

- [x] Add integration tests: existing file/buffer reuse, shortcut/draft/save, cancel, no selection, composed selection through search, target appears, directory disappears, invalid target, no-Git directory. Add selection and cancelled-session unit tests.
- [x] Run `cargo test -p helix-term --features integration --test integration gitignore` and observe failures from missing command behavior.
- [x] Implement loading/session cancellation, fixed target, explicit selection state, search and rendering; use normal buffer opening with repeated checks before a single insertion transaction.
- [x] Register both commands and default shortcut, preserving existing UI module edits.
- [x] Run focused unit/integration tests, new-file regression tests and `cargo check -p helix-term --no-default-features`.
- [x] Commit only feature paths/hunks after green tests.

### Task 3: Documentation, full verification and review

**Files:** Update `book/src/keymap.md`, `book/src/commands.md`, generated command references, approved spec status; create `docs/superpowers/reports/2026-11-16-gitignore-generator-validation.md`.

**Interfaces:** Documents Task 2 command and exact interaction; records verification evidence.

- [x] Document template categories, existing-file handling, root-only limitation, explicit save and tracked-file caveat.
- [x] Run touched-file rustfmt checks, focused and surrounding command tests, feature/no-feature checks and scoped whitespace checks. Record actual results, not anticipated results.
- [x] Request one independent final review against the spec and diff; fix material findings with regression tests.
- [x] Commit feature documentation and report, verify commit scope and report final usage/results.

## Execution decisions

The user explicitly approved the design and asked to start implementation. Execute inline now without a second approval round. Preserve the current checkout under desktop workspace guidance; no new worktree. A manual ledger in this plan's `.superpowers/sdd/` directory replaces shell-only skill helpers on Windows.

## Completion evidence

Implemented and reviewed. Focused tests: 9 unit + 11 integration + 11 new-file regressions passed. No-default-features check passed. The expanded command run was incomplete due to existing save tests stalling and an independently reproduced deletion-prompt test failure; no unrelated fix was included. See `docs/superpowers/reports/2026-11-16-gitignore-generator-validation.md` for exact evidence and review resolutions.
