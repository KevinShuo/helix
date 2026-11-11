# Configurable local AI completion validation

## Result

The normal user config now controls local AI completion. `enabled` turns it on
or off, `model-path` selects a GGUF, and `server-path` selects llama.cpp. With no
model path, the original HTTP-provider behavior remains available. The process
loads in the background and is released on disable, model changes or editor exit.

The configured model is `Qwen2.5-Coder-7B-Instruct-Q6_K.gguf` in the user's Junie
models directory. The installed Junie llama.cpp b11081 runtime is used with line
mode, 4096 context tokens, F16 KV cache, 99 GPU layers and 48 output tokens.

## Verification

- Four configuration tests passed, including literal Windows paths with spaces,
  default/external compatibility and disabling a configured model.
- All 33 `helix-term` unit tests passed in the workspace run. The AI-specific run
  passes 13 tests, including missing model/runtime, invalid endpoint, port/model
  conflicts, external reuse, early process exit and retry after configuration reload.
- All 21 automatic AI editor integration tests passed. Three opt-in live tests
  were ignored in that run; the new managed-model test was run separately.
- The managed-model test passed with the actual supplied Q6_K weights. Preview
  was `sum(items)` without modifying the document; Shift+Right accepted it.
  Disable, cancellation during startup, re-enable and editor close released the
  owned listener. Existing external services are not stopped.
- Final workspace library/test command: **456 passed, zero failures**, with three
  explicit skips described below. Doc-test groups passed in the preceding full
  workspace run, with five ignored documentation examples.
- Release build passed with `cargo build --release --locked --jobs 2 -p helix-term`
  and `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1`.
- Changed Rust files pass `rustfmt --check`; `git diff --check` passes.
- An independent read-only reviewer found SIGUSR1 skipping supervisor updates
  and reused servers retaining stale model identity. Both were fixed and verified
  by the same reviewer. The dynamic model replacement assertion failed before its
  fix, then passed after requests began checking the model alias before sending code.

Final workspace command:

```powershell
$env:HELIX_DISABLE_AUTO_GRAMMAR_BUILD = '1'
$env:HELIX_RUNTIME = 'I:\DEV\helix\target\release\runtime'
cargo test --workspace --locked --jobs 1 --lib --tests --no-fail-fast -- --skip test_normalize_path --skip git::test::symlink --skip git::test::symlink_to_git_repo
```

## Environment limitations resolved or recorded

- Windows error 1314 prevents `helix-stdx::test_normalize_path`,
  `helix-vcs::git::test::symlink` and `helix-vcs::git::test::symlink_to_git_repo`
  from creating symlinks. These three tests were explicitly excluded from the
  final successful workspace run. No operating-system privilege was changed.
- `helix-core::syntax::test::test_textobject_queries` and `test_pretty_print`
  initially failed without a grammar matching the checkout's queries. Building
  the pinned Rust grammar `77a3747266f4d621d0757825e6b11edcbf991ca5` resolved both.
  The generated library and grammar checkout are ignored local build assets.
- The first integration rebuild hit LLVM memory errors with default build
  parallelism. `--jobs 1` completed successfully; release used `--jobs 2`.
- Source grammars were not fetched globally. Deployment preserves the complete
  official Windows runtime, with missing source runtime files added, so the
  installed editor has working themes, compiled grammars and query files.

## Windows migration

- Backed up the old Scoop package, user config and user PATH under
  `%LOCALAPPDATA%\helix\backups\2026-11-11-before-local-ai-12b66080`.
- Copied the complete Scoop runtime beside `target/release/hx.exe` and added 406
  source runtime files missing from that older package.
- Preserved `gruvbox_dark_soft.toml` and its inherited `gruvbox.toml` as user
  themes in `%APPDATA%\helix\themes`. Both compare byte-for-byte with the old
  package; all existing config preferences compare equal after excluding the new
  AI section.
- Successfully ran `scoop uninstall helix`; its `hx` and `helix` shims were removed.
- Prepended `I:\DEV\helix\target\release` to persistent user PATH and broadcast
  the environment update. `where.exe hx` resolves to the new executable.
- Wrote the verified Q6_K settings to `%APPDATA%\helix\config.toml`.
- Launched the production binary in a terminal with the normal config after
  removing Scoop. `:theme` reported `gruvbox_dark_soft`; `/v1/models` advertised
  `Qwen2.5-Coder-7B-Instruct-Q6_K` on localhost:19240. Exiting released the server.
- Python, C++ and Rust health checks passed for parser, highlights, textobjects,
  indentation, tags and rainbow queries.

Edit `[editor.ai-completion]` in the normal config and use `:config-reload`.
Existing terminal sessions may need to be reopened to inherit the updated PATH.
