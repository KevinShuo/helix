# AI test generation validation

Implemented current-function GoogleTest/CMake and pytest generation with named
providers, conditional on-demand local 27B thinking, multi-file diff preview,
undoable buffer application, saved-snapshot execution, cancellation and one
assertion-preserving compile/import repair.

## Verified behavior

- CMake links the original C business library to a separately compiled C++17
  GoogleTest file. Actual vcpkg `gtest:x64-windows 1.17.0` configure/build/test
  succeeded in a pure-C root project.
- Actual reuse of an existing GTest target succeeded, including adding its new
  business-library linkage without mixing CMake link signatures and retaining a
  pre-existing CTest name prefix.
- With installed-package searches explicitly disabled, actual FetchContent
  fallback downloaded GoogleTest v1.17.0, verified its SHA256, built and passed.
- Real pytest success, assertion failure, zero collection and all-skipped runs
  were exercised. Failed assertions/empty/all-skipped execution cannot trigger
  an automatic rewrite to get a green result.
- Real pytest missing-import -> one model repair -> review/accept/save -> passing
  execution succeeded through the editor and a controlled HTTP fixture.
- An invalid repair preserves the original tests and run diagnostics. AST checks
  reject assertion-looking comments and helper-only assertions; repairs must
  preserve original assertion expressions exactly.
- Previews never write to disk; acceptance changes editable buffers. File/source
  changes, dirty buffers, changed generated CMake dependencies and configuration
  changes invalidate stale work. Cancellation stops the process tree.

## Final checks

- Default `cargo test -j 1 --config 'profile.test.package.helix-term.debug=0'`:
  151 unit tests and 2 doctests passed; 11 opt-in unit tests and 1 doctest ignored.
- AI editor regressions: 60 passed, 10 opt-in tests ignored.
- View provider/configuration tests: 11 passed.
- Explicit AI-test unit/tool suite: 18 passed, including actual installed-GTest,
  existing-target and FetchContent runs.
- Explicit AI-test editor suite: 6 passed, including all three real-pytest cases.
- The expanded real-pytest assertion/empty/all-skipped regression passed after
  reproducing and fixing the all-skipped false success.
- Changed Rust files passed rustfmt checking; `git diff --check` passed.
- An initial parallel default build exited inside rustc with
  STATUS_ACCESS_VIOLATION; the final single-job default suite passed. The existing
  integration helper dead-field warning remains.

## Review

A fresh read-only reviewer found three material issues: missing snapshots for
unchanged generated CMake dependencies on regeneration, comments satisfying
assertion validation, and incorrect source-directory context for included CMake
files. All three were reproduced by failing tests, fixed and rechecked. The
later failed-repair diagnostic preservation and pytest skip classification were
also rechecked with no material findings.

## Local enablement

`hx` resolves to `I:/DEV/helix/target/release/hx.exe`. The existing Helix config
was backed up to
`C:/Users/wangshuo/AppData/Roaming/helix/config.toml.before-ai-test-20261115-090243.bak`
and an enabled `[editor.ai-test]` section was appended. Parsed-value equality
verified that every pre-existing setting, including direct commit generation,
remained unchanged. Tests use `local-27b` with thinking in local mode and follow
the global switch to DeepSeek. Installed/vcpkg GTest is preferred and fallback
download is allowed. Both local and DeepSeek variants passed release-binary
configuration parsing without starting a model server.

For tool validation only, pytest was installed in the repository's ignored
`.superpowers/ai-test/venv`; no global Python environment was modified. A user's
Python project still needs pytest in its selected environment.

## Limits

Complex/custom CMake ownership may require explicit `library-target` and
`test-target`. Existing-project/environment configure failures remain manual;
automatic repair owns generated tests only. Non-CMake C/C++ projects receive a
draft. Inaccessible internal functions and automatic executable-to-library
restructuring are outside this version. Provider protocols and routing were
tested through controlled HTTP responses. A subsequent real 27B-thinking versus
DeepSeek generation/compile/mutation comparison is now available in
[the live benchmark report](../benchmarks/2026-11-15-ai-test-generation/report.md).

Final `cargo build --release -j 2 -p helix-term --bin hx` succeeded. The final
release binary passed `--version` startup and both local/DeepSeek configuration
health checks. No model server was started by these checks.
