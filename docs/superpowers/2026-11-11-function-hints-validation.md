# Function hints validation — 2026-11-11

Implemented the requested function reference counts and the most recent Git commit
author. The author interpretation was explicitly confirmed by the user.

## Usage

Build output: `target/debug/hx.exe`.

```toml
[editor.function-hints]
references = true
git-author = true
```

Both settings default off. The displayed text resembles `2 usages  wangs*`.
`*` means the function has local changes relative to HEAD. The text is virtual and
does not modify the document. `gr` on the name continues to use the reference picker.

References are deduplicated LSP locations with the declaration excluded. Git authors
come from pinned-HEAD range history, including deletion-only commits. Flat symbol
locations, untracked functions, ambiguous dirty mappings, non-UTF-8 Git blobs and
unavailable providers omit the corresponding author hint. The book documents limits
and independent theme keys.

## Verification

- Standard `cargo test`: 27 library tests and 2 documentation tests passed; one
  existing documentation example is ignored.
- Affected library suites: **150 passed** (17 LSP, 27 term, 28 VCS, 78 view), excluding
  two existing VCS tests requiring Windows symlink privileges.
- New end-to-end test uses a compiled std-only Rust LSP fixture and a real temporary
  Git repository. It validates capability negotiation, declaration exclusion,
  deduplication, author lookup, virtual rendering, no file edits, request caching
  and rearming the idle refresh timer. Passed.
- The hierarchy capability assertion was observed failing before enabling support,
  then passing. Pure behavior and lifecycle tests were also observed red then green.
- Real installed Python server `ty` verified over stdio: document-symbol and reference
  capabilities enabled, hierarchical function ranges returned, two usages returned
  for the two-call example.
- Built `hx.exe` successfully; `hx --version` succeeds. Python health checks pass
  (ty, ruff, parser and all query groups).
- `cargo check -p helix-term --no-default-features` passes. Existing unused-variable
  warnings in the feature-disabled VCS registry remain.
- All changed Rust files pass scoped rustfmt checks; `git diff --check` passes.
- Independent review found no remaining substantive defects after fixes for
  periodic refresh, shared invalidation/cancellation, flat symbols, Git range
  ambiguity and symbol-picker parent names.

## Broad regression and environment notes

The full editor integration run completed: **193 passed, 1 existing failure,
2 ignored, 3 symlink tests excluded** in 770.70 seconds. It used two test threads
and Git Bash on the child process PATH. The final log is
`target/function-hints-integration-final.log`. The new function-hints end-to-end
test passed within this full run as well as independently.

- The first broad run with default parallelism terminated with Windows status
  `0xc0000409` (`STATUS_STACK_BUFFER_OVERRUN`). A diagnostic serial run was stopped
  and replaced with the repository's previously documented two-thread/Git-Bash setup.
- `test_open_below_at_block_end_stays_in_block` reproduces the existing C++ indentation
  failure documented in `2026-11-11-ai-inline-completion-validation.md`. No indentation
  code was changed by this feature.
- The full VCS run encountered Windows privilege error 1314 in `git::test::symlink`
  and `git::test::symlink_to_git_repo`. The remaining 28 tests pass.
- Repository-wide formatting still flags the pre-existing tuple at
  `helix-core/src/indent.rs:903`; its presence in HEAD was verified and it is unchanged.
- Builds/tests use the existing `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1` switch after
  unrelated grammar fetching failed certificate revocation checks for hare, slisp
  and metamath. TLS verification was not disabled.

Pre-existing AI completion changes were preserved. No user configuration was changed.
After implementation, the user requested committing and pushing this feature to
`KevinShuo/helix` on `master`. The pre-existing AI completion edits are excluded from
that commit. The standard `cargo test` passed again before publication.
