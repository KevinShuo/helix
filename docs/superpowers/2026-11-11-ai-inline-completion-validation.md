# Local AI inline completion validation

Environment: Windows, Rust 1.90.0, existing Junie Local Qwen3.8 27B Q5 model.
Implementation commit: `06d76288e`. All work remains on local branch
`codex/ai-inline-completion`; nothing was pushed and remotes were not changed.

## Results

- Release build passed: `cargo build --release --locked -p helix-term` with
  `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1`. Output: `target/release/hx.exe`, reporting
  `helix 25.07.1 (06d76288)`.
- 18 AI editor integration tests passed. Coverage includes grey rendering,
  cursor and multiline Unicode positioning, entire-suggestion acceptance,
  undo, ordinary popup Tab/Enter, empty acceptance, key remapping, dot repeat,
  macros, command sequences, old-response rejection, timeout and CRLF.
- 407 workspace library/integration tests passed with three tests requiring
  unavailable Windows symlink privileges excluded (see below).
- An opt-in real-model editor test passed: automatic preview `sum(items)`,
  document unchanged until Shift+Right, then insertion through the event loop.
  Time from input through acceptance was approximately 1.70 seconds in the final
  post-review check (2.43 seconds in the earlier check).
- Launcher validation passed: PowerShell parsing, preserving the user's
  original config, file arguments containing spaces, temporary-config cleanup,
  cold model startup, synthetic completion, and stopping its own model process.
  A cold-start check returned `return a + b` in 1.14 seconds of inference.
- Python release health checks passed for parser, highlights, text objects,
  indents, tags and rainbow queries.
- Changed Rust files pass rustfmt; `git diff --check` passes.

The broad editor integration run completed: **191 passed, 1 baseline failure,
1 optional live-model test ignored, 3 symlink tests excluded**. The baseline
failure is described below. The ignored live-model test was run separately and
passed. The full run took approximately 12 minutes on this Windows machine.

## Existing environment/baseline limitations

- Windows reports privilege error 1314 when creating symlinks. The workspace
  tests `helix-stdx::test_normalize_path`, `helix-vcs::git::test::symlink` and
  `helix-vcs::git::test::symlink_to_git_repo` could not run successfully.
  The editor tests `test_symlink_write`, `test_symlink_write_fail` and
  `test_symlink_write_relative` have the same restriction. No permission or
  operating-system security setting was changed.
- `test_open_below_at_block_end_stays_in_block` fails in an existing C++
  comment/indent case on this machine. It expects the newly opened line to be
  unindented; the actual line is indented four spaces. This was reproduced with
  the same failure using an isolated source archive of pre-feature commit
  `13bd6aa1a`. No indentation production code was changed for this feature.
- Repository-wide `cargo fmt --all -- --check` flags a pre-existing tuple
  formatting difference in `helix-core/src/indent.rs`. That unrelated file was
  left unchanged; the files changed by this feature pass formatting checks.
- 298 grammar libraries were built. Sources for `hare`, `slisp` and `metamath`
  could not be fetched because their upstream host's certificate revocation
  check failed in this environment. TLS verification was not disabled.
- The first broad integration run with default parallelism did not finish
  promptly. The final run uses two test threads and Git Bash on the child
  process PATH, avoiding Windows' WSL `bash` shim for formatter fixtures.

## Review and fixes

A fresh whole-change reviewer found three important issues, all fixed with
regression tests that failed before the fixes and passed afterwards:

1. Closing the final view must not access a removed current document.
2. A multi-key acceptance mapping must remove pending prefix keys from repeat
   recording before recording the accepted literal text.
3. Acceptance must execute in order inside a command-sequence binding; repeat
   and macros preserve companion movement/mode commands as well as literal text.

There were no critical issues, deferred minor issues or declined judgments.

## Implementation decisions

- Reused the initially clean checkout on a local feature branch rather than
  creating another worktree. This keeps the requested project directly usable;
  the tradeoff is less filesystem isolation from concurrent user edits.
- Implemented the tightly coupled editor changes in one execution context and
  used one fresh reviewer for the whole feature. Integration and regression
  tests cover the interfaces where a late review could otherwise miss a bug.
- Used the repository's existing grammar-download build opt-out after upstream
  download failures, then built available grammars. The limitation is the three
  unavailable languages above, rather than weakened TLS verification.
- Used a separate loopback server on port 19240 with 60 GPU layers, an 8192-token
  context and Q4 KV cache. The saved Junie gateway's full-offload settings did not
  fit the available VRAM. This leaves Junie's settings intact; compared with its
  normal setup the completion server has a shorter context and CPU-offloaded
  layers. `-UseGateway` remains available when the saved setup fits in memory.
- Kept default editor behavior opt-in: the launcher adds AI settings to a
  temporary copy of the normal config. Launching a different installed `hx`
  does not enable this feature. Use `contrib/helix-junie.ps1` for this build.
