# Delete Current File Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the active file after explicit confirmation and close its buffer.

**Architecture:** Register two typable commands sharing a confirmation prompt and validation. Flush pending writes, reuse `Editor::delete_path`, and close the document only after successful deletion.

**Tech Stack:** Rust, existing Helix compositor prompts and Tokio integration tests.

**Spec:** `docs/superpowers/specs/2026-11-15-delete-current-file-design.md`

## Global Constraints

- No new default shortcut, dependency, file explorer action, or arbitrary-path deletion.
- Both commands require `yes`; `:delete-file!` alone permits unsaved changes.
- Preserve unrelated uncommitted work in the current checkout.

## Review Focus

- Pending saves must finish before deleting and cannot recreate the deleted file.
- Failed deletion must retain the original document and contents.
- Replaced or missing paths must never cause directory deletion.
- Multiple views of the document must not retain a deleted buffer.
- Merge documents must not delete the backing source file.

### Task 1: Commands, confirmation, documentation, and integration coverage

**Files:** Modify `helix-term/src/commands/typed.rs`, `helix-term/tests/test/commands.rs`, `helix-term/tests/test/git_conflicts.rs`, `helix-term/src/ui/prompt.rs`, `book/src/commands.md`, and `book/src/generated/typable-cmd.md`; create `helix-term/tests/test/commands/delete_file.rs`.

**Interfaces:** Consume `Context::block_try_flush_writes`, `Editor::delete_path(&Path, bool)`, and `Editor::close_document(DocumentId, bool)`. Produce `:delete-file` and `:delete-file!`, each taking zero arguments.

- [x] Write tests for confirmed deletion, cancellation, unconfirmed input, dirty-buffer rejection and forced confirmation, scratch/missing files, path arguments, multiple buffers/views, pending saves, failed deletion, and a path replaced by a directory.
- [x] Run `cargo test -j 1 -p helix-term --features integration --test integration test::commands::delete_file -- --test-threads=2`; observe missing-feature failures.
- [x] Implement shared validation, a path-bearing `yes` prompt, revalidation after flushing writes, deletion, and buffer closing; reject merge documents.
- [x] Document command usage, permanent deletion, cancellation, and unsaved changes.
- [x] Repeat focused integration tests; run neighboring new-file/command-line tests and ordinary library tests.
- [x] Run formatting and whitespace checks, review only this feature's changes, and record the results. Leave changes uncommitted because the shared checkout contains unrelated work.

## Execution record

Using the current checkout and inline execution to preserve the user's active
uncommitted changes. No shared task interfaces require coordination.

- The missing-feature run failed 7 of 10 initial tests; three existing rejection
  cases already passed. Subsequent focused verification passed all 12 tests.
- Independent review identified clipping of the target filename at 40 columns.
  The expanded long-path regression failed before the fix. Size prompt help
  using the actual viewport width; retain saturating arithmetic for tiny windows.
- Use the existing wait-futures path for deferred confirmation so it does not
  depend on the global job callback channel's lifetime across editor instances.
- Final verification: 12 feature tests, 163 unit tests, 2 doc tests, formatting,
  whitespace checks, and the default-feature debug build passed. The complete
  integration run had 346 passes, 17 ignored tests, and 2 timeouts; both timeouts
  passed in isolated reruns without source changes. See
  `docs/superpowers/reports/2026-11-15-delete-current-file-validation.md`.
