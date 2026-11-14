# New file validation — 2026-11-14

## Behavior

`Space n` prompts for a filename beside the active file. Scratch buffers use
the editor cwd. The named buffer opens in insert mode; disk writes remain in
`:w` / `:w!`. `:new-file <path>` shares the same validation. Existing paths and
unsaved named buffers are rejected. Escape is inert, and `:new` remains unchanged.

The existing editor policy removes an empty, unmodified scratch buffer when
replacing it; file buffers and unsaved scratch contents remain available.

## Verification

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `cargo test -j 1 -p helix-term`: 79 unit tests and 2 doc tests passed;
  3 unit tests and 1 doc test ignored by the existing suite.
- Final focused integration run: 11 passed, covering deferred save, original
  buffers, cancellation, validation, disk/buffer collisions, scratch fallback,
  Unicode and space-containing filenames, nested paths, insert repeat, and
  ASCII/Unicode Windows case aliases.
- Final unit rerun (`cargo test -j 1 -p helix-term --lib`): 79 passed,
  3 ignored.
- Final complete integration rerun (`cargo test -j 1 -p helix-term --features
  integration --test integration -- --test-threads=4`): 281 passed, 5 failed,
  7 ignored. All 11 new-file tests passed within this run.
- Independent code review: no remaining material findings after fixes.

The feature tests were run before implementation: 7 missing-feature failures
and 2 existing-behavior passes. Insert-repeat and Windows case-alias regressions
also failed before their fixes. Unicode alias handling was separately reproduced
with `École.txt` / `école.txt` before replacing ASCII-only comparison.

## Earlier validation failures and configuration

Initial concurrent builds crashed in rustc (`STATUS_ACCESS_VIOLATION`, an LLVM
out-of-memory diagnostic, and `STATUS_HEAP_CORRUPTION`). Subsequent verification
uses `-j 1`; no compiler/profile changes were made to repository configuration.

Running unit tests with `--features integration` enabled encountered the
existing `integration_test` runtime-local requirement: these synchronous tests
do not enter a Tokio runtime. All six passed under the normal default unit-test
configuration:

- `handlers::function_hints::tests::late_results_are_rejected_after_cross_file_change_or_scroll`
- `handlers::function_hints::tests::late_results_cannot_attach_to_renamed_document`
- `handlers::function_hints::tests::late_results_cannot_revive_hints_after_edit_or_view_close`
- `handlers::function_hints::tests::reference_and_author_results_arrive_independently`
- `handlers::function_hints::tests::refresh_replaces_and_removes_an_annotation_at_eof`
- `handlers::function_hints::tests::viewport_results_preserve_offscreen_hints_and_replace_visible_hints`

The first complete integration run with default test concurrency passed 275
tests before aborting in `test::movement::find_char_line_ending` with a standard
library BTree invariant failure (`STATUS_STACK_BUFFER_OVERRUN`). An isolated
rerun passed. The final complete rerun reduces concurrency to four test threads.
No movement or function-hint source files were changed.

The final complete integration run finished without the BTree abort. Its five
failures were:

- `test::commands::test_open_below_at_block_end_stays_in_block`: a C++
  `if (x) // c` insertion indent differs from the expected result. Reproduced
  in isolation with the repository runtime explicitly selected. No indentation
  implementation or query files were changed by this feature.
- `test::commands::write::test_symlink_write`: Windows privilege error 1314.
- `test::commands::write::test_symlink_write_fail`: Windows privilege error 1314.
- `test::commands::write::test_symlink_write_relative`: Windows privilege error 1314.
- `test::commands::write::test_write_quit_auto_format_exits_after_format`:
  the Bash formatter did not produce the expected content. An isolated rerun
  passed after adding the existing Scoop Git `bin` directory to the child
  process PATH. No machine environment or formatter source was changed.

Four existing tests therefore remain unresolved in this environment: the
indentation assertion and the three tests requiring symlink privileges.

The existing integration helper also emits a dead-code warning for
`TestCase::line_feed_handling`.

## Windows comparison

The existing `windows-sys` dependency gains its `Win32_Globalization` feature.
Normalized Windows paths are compared with `CompareStringOrdinal`, preserving
UTF-16 data and using the OS case table rather than lossy string conversion or
ASCII-only folding. See the [Microsoft API reference](https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-comparestringordinal).

## Build artifact

`cargo build -j 1 -p helix-term --bin hx` passed with the ordinary default
features, producing `target/debug/hx.exe`. Its `--version` invocation passed.
Use `cargo run -- <file>` from the repository to select the repository runtime.
The implementation was initially left uncommitted on
`codex/new-file-in-current-directory`. The user subsequently requested committing,
pushing, compiling a release build, updating the installed executable, and
publishing a GitHub release. See `docs/releases/new-file-2026.11.14.md` for that
release's notes; exact revision and binary hashes are recorded in the release's
`BUILDINFO.json` and `SHA256SUMS.txt`.
