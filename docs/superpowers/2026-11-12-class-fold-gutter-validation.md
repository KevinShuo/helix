# Class folding and clickable gutter validation — 2026-11-12

This follow-up implements the user's PyCharm-style mouse folding request and
extends the original function-only scope to Python classes, C++ classes and
C/C++ structs. The user also requested portable AI model/server paths in the
preceding turn; examples now use the already-supported `${USERPROFILE}` syntax.

## Behavior

- `▾` and `▸` appear on function/class declaration lines, after decorator or
  template wrappers. The existing optional `fold` gutter is enabled in the
  user's configuration, together with `mouse = true`.
- Mouse clicks toggle the displayed definition. Empty gutter rows, wrapped
  continuation rows without a symbol and virtual diagnostic rows are inert.
- Class headers/bases, C/C++ closing braces and suffixes remain visible. Nested
  method folding survives closing and reopening the class. Existing cursor,
  selection, editing and per-view state rules are reused.
- Empty, single-line or incomplete definitions and arbitrary blocks/docstrings
  are not added as fold targets.
- Gutter hit-testing derives row-start source lines from the same formatter,
  anchor and annotation layout as rendering. It handles soft wraps, horizontal
  scrolling, virtual rows and source lines merged by a preceding fold.

## Verification

- Real syntax tests were observed failing before class support, then 8 passed
  (one separate layout measurement remains ignored).
- The final folding integration group passes 10 tests. New tests cover Python
  and C++ class/method mouse toggles, declaration-line arrows, nested restoration,
  wrapped blank rows, and a second class header sharing the preceding folded
  class's closing line. Existing Git dock/resize integration remains green.
- Core/view suites pass 300 tests, with the same 4 ignored cases as before.
  Standard `cargo test` passes 42 library and 2 doc tests (1 doc test ignored);
  this working-tree run includes the pre-existing local AI regression test.
- `cargo check -p helix-term --no-default-features` passes. Scoped formatting and
  whitespace checks pass. No dependencies were added.
- The earlier complete integration run's baseline failures are documented in
  [the original validation](2026-11-12-code-folding-validation.md). That broad run
  is not relabeled as green or claimed as a new run for this focused follow-up.

An independent read-only review found two related mouse-coordinate issues:
blank wrapped rows toggled folds, and the first attempted correction could reject
a visible arrow after folded source lines joined a later wrapped header. Both
were reproduced by failing integration tests. The final formatter-based mapping
passes both tests, and the review reports no remaining correctness findings.

## Rollout

The user's existing push/release/local-install instructions still apply. The
implementation will be built from a clean commit with unrelated local edits
temporarily preserved, and published as `code-folding-2026.11.12.1`.
The installed path remains `C:\dev\helix\target\release\hx.exe`.
Running editor sessions are preserved; restarting Helix loads the new executable
and class-fold queries. Final artifact and installation details follow after the
release checks.
