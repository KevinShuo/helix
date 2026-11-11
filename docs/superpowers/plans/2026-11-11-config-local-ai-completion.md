# Configurable Local AI Completion Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Launch ordinary Helix with local AI completion controlled entirely by config.toml.

**Architecture:** Extend the existing view configuration and add an application-owned
asynchronous llama.cpp supervisor in helix-term. The completion handler waits for
readiness through a runtime-local watch receiver, retaining the current cancellation
and HTTP request behavior.

**Tech Stack:** Rust, serde/TOML, Tokio watch/process, reqwest, installed llama.cpp.

**Spec:** `docs/superpowers/specs/2026-11-11-config-local-ai-completion-design.md`

## Global Constraints

- Default disabled; no model-path preserves external HTTP provider behavior.
- Model/server paths are arguments to a process, never interpolated shell commands.
- Local model startup requires an IPv4 loopback endpoint and never replaces unrelated processes.
- Owned processes stop on disable/reconfigure/application drop; reused processes remain alive.
- Use the supplied Qwen2.5-Coder Q6_K GGUF and preserve unrelated user configuration.

## Review Focus

- Reload during cold startup must cancel and release the previous process.
- A port serving another model must not receive source code or be stopped.
- Changing ordinary editor settings should not restart the loaded model.
- Windows paths containing spaces must reach llama.cpp unchanged.
- Startup failure or timeout must not freeze editing or retry on each keystroke.

### Task 1: Local provider configuration and lifecycle

**Files:** `helix-view/src/ai_completion.rs`, `helix-view/tests/ai_completion.rs`,
`helix-term/src/handlers/ai_completion/server.rs`,
`helix-term/src/handlers/ai_completion.rs`, `helix-term/src/application.rs`.

**Interfaces:** `LocalServer::new(&AiCompletionConfig) -> LocalServer`,
`LocalServer::configure(&mut self, &AiCompletionConfig)`,
`server::wait_ready(&AiCompletionConfig) -> impl Future<Output = anyhow::Result<()>>`.

- [x] Add configuration tests for a local GGUF with Windows paths, disabled settings,
  and compatibility with external endpoints; run and observe unknown-field failures.
- [x] Add config fields and supervisor. Gate automatic requests on local readiness.
- [x] Test invalid endpoint, missing model/runtime, incompatible/compatible running
  server, process exit, cancellation and settings reload as applicable.
- [x] Run `cargo test --locked -p helix-view --test ai_completion` and AI term unit tests.

### Task 2: Editor integration, documentation and usable local setup

**Files:** `helix-term/tests/test/ai_completion.rs`, `book/src/editor.md`,
`contrib/helix-junie.md`, `target/local-ai/config.toml` (ignored local artifact),
user config/themes/PATH and the Scoop Helix package (backed up).

- [x] Add disabled/local-failure integration coverage and an opt-in managed-model test.
- [x] Document a complete local config, external compatibility, reload/disable and logs.
- [x] Prepare a standalone config preserving the user's settings and selecting Q6_K;
  parse the result and verify its model/runtime paths.
- [x] Run AI integration tests and a real managed-model preview/accept/disable test.
- [x] Run appropriate workspace checks, changed-file formatting and `git diff --check`.
- [x] Build the release editor, review the complete diff, and report evidence/limitations.
- [x] Back up the Scoop package/config/PATH and preserve the complete runtime plus
  the selected `gruvbox_dark_soft -> gruvbox` theme inheritance chain.
- [x] Verify the new build, uninstall Scoop Helix and add `target/release` to user PATH.
- [x] Apply the Q6_K settings to the normal user config and verify theme preservation.
- [ ] Commit and push the verified changes to `KevinShuo/helix`.

## Execution record

- Task 1 complete: configuration tests failed on unknown fields before implementation;
  server tests failed before startup implementation. Final 4 configuration and 13 AI
  unit tests pass. A reload/retry regression and a dynamic external-model replacement
  regression were both observed failing before their fixes.
- Review found SIGUSR1 bypassing supervisor reconfiguration and stale identity of reused
  external servers. Both were fixed; the same independent reviewer verified the fixes
  and reported no remaining Critical/Important findings.
- Task 2 integration: final built test executable passes all 21 automatic AI tests
  (3 opt-in live tests ignored). The managed Q6_K live test passes separately, returning
  `sum(items)` and verifying acceptance, disable, startup cancellation, re-enable and exit.
- Ruling: initially prepared an independent config because the system `hx` was the
  original Scoop release. The user then explicitly requested uninstalling Scoop,
  preserving themes, setting PATH to the new build and pushing the finished work.
  Complete that migration after validating the release build; keep the standalone
  config as a verified staging artifact before updating the normal config.
- Ruling: local Windows closed-port TCP connection refusal takes about 2 seconds;
  use a bind check to detect available/conflicting ports promptly.
- Ruling: the integration rebuild initially exhausted memory at default parallelism.
  Rebuilding with `--jobs 1` passed; use bounded parallelism for remaining builds.
- Final workspace library/tests: 456 passed, zero failures, three Windows symlink
  tests explicitly skipped because error 1314 prevents symlink creation. Two syntax
  failures were resolved by building the repository-pinned Rust grammar for tests.
- Deployment verified: Scoop Helix uninstalled, user PATH begins with
  `I:/DEV/helix/target/release`, normal config enables Q6_K, native `:theme` reports
  `gruvbox_dark_soft`, and the normal editor advertises the Q6_K model on port 19240.
  Python/C++/Rust parser and query health checks pass; closing the editor releases
  its server. Existing settings and both inherited theme files compare unchanged.
