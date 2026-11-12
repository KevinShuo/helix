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

- Built from clean commit `8b3b22b7fd079a7fd86102d10a54d9eb882d47b2` using
  `cargo build --release --locked --jobs 2 -p helix-term` (4m 02s).
  `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1` reuses the previously verified grammar
  cache. Clean-source checks also pass: 41 term library tests, 10 folding
  integration tests, and the no-default-features check.
- The four pre-existing local edits were temporarily stashed, excluded from the
  build, then restored byte-for-byte against their saved SHA-256 hashes. The
  temporary stash was removed after restoration.
- Pushed the implementation to `fork/master` and `fork/codex/code-folding` and
  published [code-folding-2026.11.12.1](https://github.com/KevinShuo/helix/releases/tag/code-folding-2026.11.12.1).
  The public release API confirms the release is not a draft and its three
  uploaded assets have the expected sizes and SHA-256 digests. The release tag
  remains on the implementation commit; this rollout record is a later docs
  commit.
- The Windows ZIP is 37,397,456 bytes and includes the executable, runtime,
  298 Windows grammar DLLs, Chinese installation notes and build information.
  ZIP SHA-256:
  `9dd2dc67de2db82db86c8cdf47a5f1743cb264f0623780dcc45ee37bf380387f`.
  Executable SHA-256:
  `f52ddede160e0ca7eda95035c4984f83466af1cb03d22d769c1f0193df7fcd80`.
- Standalone package and installed Python/C++ health checks pass. Native
  terminal smoke checks using actual mouse events verify closing and reopening
  a C++ class, folding its method, and folding a Python class. Declaration-line
  arrows and preserved headers were inspected in terminal output.
- The installed executable is `C:\dev\helix\target\release\hx.exe`; resolving
  `hx` from the persisted user/machine PATH selects it and reports
  `helix 25.07.1 (8b3b22b7)`. Installed Python/C/C++ folding queries were updated
  from the verified package. The user's `mouse`, `fold` gutter and portable
  `${USERPROFILE}` AI paths remain configured.
- Existing editor sessions were left running. Their previous executable is
  preserved at `C:\dev\helix\target\release\hx-before-class-folding-899a5b51.exe`.
  Restarting Helix loads the new executable and class-fold queries. The earlier
  Scoop Helix uninstall and PATH setup remain in effect.
