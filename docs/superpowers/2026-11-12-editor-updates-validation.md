# Recent editor updates validation — 2026-11-12

This release combines the previously committed Minimap work with the pending
AI line-completion context and function-hint cache fixes. The user authorized
committing the recent changes, pushing the fork and publishing a Windows Release.

## Changes

- Line-mode AI requests retain the original source order, with `<CURSOR>` inside
  the incomplete line. Exact prefix/suffix validation and insertion extraction
  remain in place. The launcher check uses the same prompt format.
- Function reference counts and Git authors remain visible during asynchronous
  refreshes. Viewport results replace annotations within their source interval
  while preserving offscreen cached annotations, including EOF annotations.
  Successful empty/null symbol results remove stale visible annotations.
- Existing cancellation, document/version, server, configuration, fold and
  generation invalidation rules are retained.
- The language-server guide documents `auto-document-highlight = true` and
  `:config-reload`. This is an existing opt-in feature, not a new default.
  Live C++ clangd and Python ty requests returned all three references to a
  sample `sidebar` variable when checked for the user's highlighting request.

Minimap implementation and its earlier independent review, terminal acceptance
and performance measurements are recorded in
[the Minimap validation](2026-11-12-minimap-validation.md).

## Fresh verification

Windows, Rust 1.90.0. Commands set `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1` and use
the existing runtime/grammar cache. Git Bash is added to the test process PATH.

| Check | Result |
| --- | --- |
| `cargo test --workspace --locked --no-fail-fast -- --test-threads=2 --skip test_normalize_path --skip git::test::symlink` | 588 passed, 6 ignored, 3 filtered out, 0 failed across 42 suites, including documentation tests |
| `cargo test --locked -p helix-term --features integration --profile integration --test integration -- ai_completion function_hints minimap folding git_tools --test-threads=2` | 56 passed, 4 ignored, 177 unrelated tests filtered out, 0 failed |
| `rustfmt --edition 2021 --check` for the four changed Rust files | Passed |
| PowerShell parser check for `contrib/helix-junie.ps1` | Passed |
| `git diff --check` | Passed |
| Independent read-only review of the pending AI/hint changes | No Critical, Important or Minor findings |

The unfiltered workspace run first reproduced Windows error 1314 in
`helix-stdx::test_normalize_path`. That test and the two `helix-vcs::git::test::symlink*`
tests depend on unavailable Windows symlink privileges and were explicitly
excluded in the successful workspace run. OS security settings were not changed.

The integration run covers source-order AI prompts, completion acceptance,
function-hint scrolling and asynchronous refreshes, external Git author changes,
empty symbol results, Minimap rendering/navigation/invalidation, folding and the
native Git panel. The four ignored tests retain their existing optional/manual
status. This is the relevant integration subset, not the entire editor suite.
Previously recorded full-suite C++ indentation and platform limitations are not
claimed fixed; see [the folding validation](2026-11-12-code-folding-validation.md).

## Release checks

The release process builds the clean committed source with default features via
`cargo build --release --locked --jobs 2 -p helix-term`. It packages the executable,
tracked runtime files, 298 cached Windows grammar DLLs, Chinese instructions,
configuration examples and build information. The package enables Minimap and
symbol reference highlights in an optional example, preserving editor defaults.

Before publication, the package is extracted and its files are compared against
the ZIP, the executable's reported source commit is checked, and Python/C++/Rust/Go
parser and query health checks run with isolated configuration and no
`HELIX_RUNTIME` override. The release tag and uploaded asset sizes/SHA-256 digests
must match the local source and artifacts. Final artifact details are supplied in
the release's `BUILDINFO.json` and `SHA256SUMS.txt`.

Fresh logs and package verification artifacts are kept in the ignored
`target/recent-release` directory. Personal `config.toml` and `languages.toml`
are not part of the release package or source commit.
