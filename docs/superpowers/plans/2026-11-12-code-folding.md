# Function Folding Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Deliver the Python/C++ function folding described in the existing handoff, with shared display coordinates, independent view state and safe editing.

**Architecture:** Partially adapt the formatter, movement and UI integration from upstream PR #16453, commit `0cfe8f9bae685e88a631ead0b4880b6ac93afb61` (A2ureIm4la, MPL-2.0, still open on 2026-11-12). Replace its line-based generic block detection with dedicated function queries and a document-version cache. Retain nested closed ranges but expose only outermost ranges to layout; invalidate affected folds before mapping edits.

**Tech Stack:** Existing Rust 1.90 workspace, Ropey, Tree-house/Tree-sitter, terminal UI; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-12-code-folding-design.md`

## Global constraints

- Preserve Python decorators/multiline signatures, C++ opening/closing braces and trailing source text; use `…`.
- Functions and methods only; no arbitrary selection folds or indentation fallback.
- Keep text, modified flag, text version and undo history unchanged by view operations.
- Keep nested state, split independence, buffer persistence and existing hints/AI/Git tools.
- Preserve the four pre-existing AI-completion edits. Work in the existing checkout on `codex/code-folding`, based on `9cc212a80`; the user subsequently authorized Git push, a GitHub Release and replacing Scoop Helix with the compiled release on PATH.

## Review focus

1. Hidden annotation anchors at a fold start/end must not leak diagnostics or stall iteration.
2. Editing through another split must invalidate affected folds without corrupting unaffected ranges.
3. A multiline signature, constructor initializer or trailing comment must never be mistaken for the body.
4. Folded EOF, CRLF, Unicode and narrow soft wrapping must retain correct forward/reverse positions.
5. Large hidden functions must not force per-character work in formatting or per-frame query collection.

## Task 1: Shared fold layout and movement

**Files:** `helix-core/src/{doc_formatter.rs,doc_formatter/test.rs,text_annotations.rs,movement.rs,position.rs}`.
**Interfaces:** `TextAnnotations::add_folds(&[Range<usize>])`, `fold_containing(usize)`, explicit `GraphemeSource::Fold { codepoints }`.

- [x] Add upstream formatter/movement behavior tests first; verify the missing fold API fails compilation.
- [x] Adapt the upstream implementation with explicit fold source, checkpoint normalization and concealed line-annotation skipping.
- [x] Test CRLF/Unicode, hidden anchors, reverse coordinates, nested ranges, end-of-file, horizontal/vertical movement and wrapping.
- [x] Run `cargo test -p helix-core --lib`; expect all tests passing (baseline: 167 passed).

## Task 2: Function candidates and per-view lifecycle

**Files:** `helix-core/src/{fold.rs,syntax.rs,lib.rs}`, `runtime/queries/{python,c,cpp}/folds.scm`, `helix-view/src/{document.rs,folding.rs,view.rs}`, `helix-view/tests/folding.rs`.
**Interfaces:** cached `Document::fold_candidates() -> &[FoldCandidate]`; `FoldCandidate { declaration, range, function }`; `ViewData` closed and effective ranges; document methods to close/reveal/map folds.

- [x] Add real Python/C++ syntax fixtures for multiline signatures, decorators, nesting, methods, invalid/empty/single-line bodies, CRLF/Unicode and closing suffixes.
- [x] Compile dedicated fold queries through the syntax loader; cache candidates until edits/language changes.
- [x] Add state tests for nested restoration, other cursors, fold-all cursor exclusion, transactions, cross-view edits, undo/redo, language change and view cleanup.
- [x] Integrate selection reveal and conservative edit invalidation in the existing document transaction path; clear stale AI previews on state changes.
- [x] Run core and view tests; expect all folding tests and existing relevant suites passing.

## Task 3: Commands, gutter, highlighting and hints

**Files:** `helix-term/src/{commands.rs,commands/folding.rs,keymap/default.rs,ui/editor.rs,ui/document.rs,handlers/function_hints.rs}`, `helix-view/src/{editor.rs,gutter.rs,view.rs,function_hints.rs}` and folding integration tests.
**Interfaces:** `fold`, `unfold`, `toggle_fold`, `fold_all`, `unfold_all`; opt-in `fold` gutter with configurable `▾`/`▸`; existing theme keys from the handoff.

- [x] Test the commands and mouse toggle with real parsed buffers and exact visible output.
- [x] Adapt upstream keymaps/gutter and bind both `z` and `Z` without replacing existing actions.
- [x] Highlight actual visible source segments, avoiding traversal of hidden bodies; filter hidden function requests and retain/project declaration hints.
- [x] Verify search/navigation reveal, true-text copy/delete/save, split/buffer persistence, AI preview invalidation and Git dock/resize compatibility.
- [x] Run folding integration tests, affected library suites and no-default-features check.

## Task 4: Regression, review and delivery

**Files:** book editor/configuration/themes/keymap/generated commands; `docs/superpowers/2026-11-12-code-folding-validation.md`.

- [x] Document setup, commands, supported languages, conservative invalidation and source attribution.
- [x] Run standard `cargo test`, core/view tests, affected editor integrations, scoped formatting and `git diff --check`; separately report reproduced baseline/environment failures.
- [x] Build `hx`, verify a real terminal with Python/C++ if the environment permits, and record a fixed large-function timing comparison.
- [x] Request an independent whole-change review as required by executing-plans; fix material findings and run targeted regressions.
- [x] Leave verified implementation in this branch, with the user's pre-existing edits preserved.

## Progress

- Latest fork/master fetched and fast-forwarded; original edits restored without conflicts. Safety patch is in ignored `target/folding-local-backup/`.
- Upstream patch inspected. Reuse is partial: its generic line folding hides multiline signatures, its edit mapping does not invalidate edited bodies, and its contiguous highlighter traverses hidden source. These parts require replacement.
- Baseline `cargo test -p helix-core --lib`: 167 passed.

## Follow-up authorized rollout

- [ ] Commit only the folding change; temporarily preserve original edits for a clean release build.
- [ ] Build and verify the Windows release package and matching runtime.
- [ ] Push the implementation and release tag to KevinShuo/helix and publish GitHub assets.
- [ ] Uninstall Scoop Helix, add the compiled directory to user PATH, verify hx resolution and retain configuration.
