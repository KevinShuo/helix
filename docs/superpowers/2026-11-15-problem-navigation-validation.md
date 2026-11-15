# Problem navigation validation — 2026-11-15

Implemented the selected approach C in the existing checkout, preserving the
uncommitted AI feature work. No installation, release, commit or remote mutation.

## Delivered behavior

- `]q` / `[q`, `Space E`, `:next-problem`, `:previous-problem`, `:problems`.
  Persistent source/minimum-severity/scope filters, stable traversal across files,
  refreshed LSP positions, same-position messages, previews, jump history and fold reveal.
- `:compile`, `:recompile`, `:compile-cancel`, `:compile-log`.
  Streamed independent stdout/stderr parsing for GCC/Clang, MSVC, Rust human and
  Cargo JSON diagnostics. Build command/cwd retention and source-specific filtering.
- Bounded logs/lines/problems (1 MiB / 64 KiB / 10,000), read-only named output
  documents with bounded history, stale-location notices, and owned process trees.

## Verification evidence

- `cargo test -p helix-term --features integration --test integration test::problems -j 1 -- --test-threads=1`:
  **9 passed, 0 failed**, final run 29.90 seconds.
- `cargo test -p helix-term --lib -j 1 -- --test-threads=1`:
  **157 passed, 0 failed, 12 ignored** (existing opt-in tests). All 5 new parser
  tests passed. Existing test-helper dead-code warning only in the integration build.
- `cargo test -p helix-view --lib -j 1 -- --test-threads=1`:
  **88 passed, 0 failed**.
- `cargo check -p helix-term -j 1`: **passed** (31.45 seconds).
- `rustfmt --check` on new Rust modules, integration fixture/tests and document
  output guard: **passed**. `git diff --check`: **passed**.

The integration tests use a real std-only compiler fixture, built into an owned
temporary directory. They cover fragmented messages, both pipes, no final newline,
exit 3, Unicode and space-containing paths/executable names, recompile, source focus,
canceling descendants, detached descendants holding pipes, launch failure preserving
previous results, truncation, missing files, CRLF bounds, UTF-16 columns, folds,
picker selection, live diagnostic removal, atomic filters and grapheme normalization.

## RED → GREEN evidence and review fixes

- New cross-file tests failed against the original commands (cursor remained at 0),
  then passed after implementation.
- Parser tests initially failed without parsing; the Cargo path test was isolated
  from the repository's real `src/lib.rs` and now checks cwd and manifest fallback.
- A standalone Rust probe reproduced cmd.exe quote corruption with `.arg(command)`;
  using raw command text plus cmd's outer command delimiter fixes quoted executables
  and separately quoted arguments. Final integration fixtures use direct quoted executables.
- Fresh independent review found output edit protection, clean scratch retention,
  and newline-inclusive bounds defects. Reproducing integration tests failed before
  their fixes, then passed. Read-only logs now reject user transactions, retain
  their document identity, and have no undo revision accumulation.
- Review also identified child startup/EOF ownership races. Windows shells start
  suspended, join a kill-on-close job, then resume their owned thread; Unix uses
  a process group. Ownership closes when the shell exits, before pipe EOF waits.
- A combining-character regression failed because the stored anchor preceded
  grapheme alignment. Store the actual post-selection cursor; final regression passes.

## Implementation choices and limits

- Preserve `]e` / `[e` and `Space e`, which already have syntax/file-explorer bindings;
  use the previously free `]q` / `[q` and `Space E` instead.
- `--severity warning` is a minimum threshold (warnings + errors).
- Generic text compiler formats do not declare column units. For non-ASCII/tabbed
  source lines, visit the correct line start with an explicit precision notice;
  Rust/Cargo character columns remain precise. GCC's display-column default is
  documented in [GCC diagnostics options](https://gcc.gnu.org/onlinedocs/gcc/Diagnostic-Message-Formatting-Options.html).
- LSP navigation sees reported diagnostics, not an independently forced project scan.
- Build output is decoded as UTF-8; legacy Windows compiler code pages/localized
  severity names are not adapted in this version. Commands that change directories
  should emit absolute paths. Build stale checks use buffer dirty state and file metadata.
- No new dependencies; all process fixtures and build logs remain local.
