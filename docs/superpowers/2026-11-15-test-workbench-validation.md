# Native test workbench validation

Scope: Python pytest/unittest and C/C++ GoogleTest gutter run actions, a docked
results tree/output, failure navigation and failed-only reruns. No debugger changes.

## Evidence during implementation

- Test identity selection failed before implementation, then view tests passed.
- Gutter/command integration tests failed for missing `test` gutter / `:test-show`
  before wiring. The initial dock test caught asynchronous focus/close ordering;
  explicit editor-owned focus requests replaced delayed compositor callbacks.
- Real pytest bridge regressions: four passed, covering mixed outcomes, teardown
  errors, unittest, parameter IDs containing `::`/brackets, exact failed-only
  selection, an ancestor pytest root, and configured deselection.
- CTest fixture/duplicate-name tests failed before the fixes, then passed. JUnit
  selects the requested case by name; File API target sources disambiguate shared
  GoogleTest names across executables. Relative preset directories resolve against
  the project root, relative source paths against the codemodel source root.
- Independent read-only design/code reviews found and checked fixes for decorator
  selection, document/language cache identity, merge cursor, fixture results,
  pytest rootpath/deselection and CMake File API paths.

## Harness notes

The ordinary term unit suite passed during the first combined-package run; three
view coordinate fixtures required their default gutter offset updated from 7 to 8.
Final verification uses ordinary unit mode and integration mode separately.

Combining `--features helix-term/integration` with all unit tests triggers existing
synchronous function-hint tests that call the integration runtime without Tokio:

- `reference_and_author_results_arrive_independently`
- `refresh_replaces_and_removes_an_annotation_at_eof`
- `viewport_results_preserve_offscreen_hints_and_replace_visible_hints`
- `late_results_cannot_attach_to_renamed_document`
- `late_results_are_rejected_after_cross_file_change_or_scroll`
- `late_results_cannot_revive_hints_after_edit_or_view_close`

That invocation reported 154 passed, 6 failed, 12 ignored; each failure was
`there is no reactor running`. This work does not change those tests/runtime.

A debug build also hit a full I: drive. The project's `target/debug/incremental`
cache was preserved by moving it to
`C:/Users/wangshuo/AppData/Local/Temp/helix-test-workbench-cache-20261115`, freeing
about 50 GB. Subsequent workbench verification disables incremental cache creation
for these commands. Source, benchmark artifacts and user files were retained.

## Final checks

- Default unit mode: helix-term 161 passed / 12 opt-in ignored; helix-view 91 passed.
- Workbench editor integration: 3 passed / 3 real-framework tests opt-in ignored.
- All 3 real-framework editor tests then passed explicitly (44.29s): relative
  CMake preset, C library linkage, duplicate GoogleTest suite names in two targets,
  parameterized cases, CTest setup fixture, skip, exact single/file/failed reruns,
  build errors; pytest --maxfail, cross-file reruns, cancellation and a subsequent
  fresh run with no late-state pollution.
- Python bridge pytest suite: 4 passed; Ruff checks/format passed.
- Complete editor regression: 334 passed / 17 opt-in ignored, with 4 workers,
  267.20s. An initial unrestricted parallel attempt exited 1 without a harness
  summary during concurrent builds; a diagnostic serial run was stopped after
  over 130 passing tests to use bounded parallelism. Neither is counted as passing.
- Release build succeeded; startup and Python/C++ health checks passed from the
  installed `target/release/hx.exe`, with parser and query discovery confirmed.
- The existing running editor was preserved. Previous binary is
  `target/release/hx.before-test-workbench-20261115.exe`; config backup is
  `C:/Users/wangshuo/AppData/Roaming/helix/config.toml.before-test-workbench-20261115.bak`.
  Parsed-value comparison confirmed only `editor.gutters` changed (added `test`).
- A final real CTest probe confirmed duplicate registered names are legal and
  `-R` would select both commands. Added a regression that fails without a
  uniqueness guard; selected ambiguous names now require explicit TEST_PREFIX.
  Post-guard unit verification: helix-term 163 passed / 12 ignored; helix-view
  91 passed. A compiler process crashed once with STATUS_HEAP_CORRUPTION while
  another release build was active; retry completed and exposed the expected
  failing guard regression before implementing the fix. The subsequent full unit
  run passed. New Rust files pass rustfmt; Python files pass Ruff lint/format.
  Post-guard workbench verification: all 6 editor/framework tests passed,
  including all 3 real-framework cases, in 43.84s. Full workspace rustfmt,
  new Python Ruff lint/format, and git diff whitespace checks passed.
  The broad 334-case regression above predates only the final duplicate-name
  guard; its focused regression, full unit suites and all six workbench flows
  were rerun after that guard.

## Final installed build

The ordinary final ThinLTO binary link crashed with STATUS_HEAP_CORRUPTION. A
main-crate-only LTO override failed with LNK1136 against the existing dependency
artifacts. The successful final build regenerated the complete dependency graph:

```powershell
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_INCREMENTAL = '0'
cargo build --release -p helix-term --bin hx --target-dir C:/Users/wangshuo/AppData/Local/Temp/helix-test-workbench-native-20261115 -j 2
```

This completed in 4m49s. Repository profile defaults are unchanged; the local
installed build retains release optimization and disables cross-crate LTO. Its
size/performance tradeoff was not benchmarked.

Installed: `I:/DEV/helix/target/release/hx.exe` (35,040,768 bytes).
SHA-256: `77af097d5b68280270fc069e94324b9b64cfb3a5cc69b3503517994e7f9dedd4`.
Staged-copy and installed hashes matched. The preceding workbench binary was
preserved as `target/release/hx.before-test-workbench-guard-20261115.exe`, in addition
to the original pre-workbench backup. The running editor process was not stopped.
The final installed binary passed default-config Python and C++ health checks,
including parser and all query types; the custom gutter contains `test`.

Working changes remain in the existing checkout without a commit/merge. Concurrent
diagnostics changes from another user task were preserved.
