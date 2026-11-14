# AI Git add / commit validation

Design approved on 2026-11-14. Implementation uses the existing Git executable,
native terminal components and shared OpenAI-compatible transport, with no new
third-party dependency. All staging/commit test writes use temporary repositories.

## Feature checks

- Git backend focused suite: 29 passed. Covers immutable reads; exact paths;
  staged/worktree separation; initial/detached repositories; partial staging;
  rename/deletion/recreation; stale disk, index and symbolic HEAD; shared operation
  guard; manual-review metadata; local-only complete diff; hook and signing errors.
- AI client: 14 passed. Covers both languages, strict JSON/IDs, sensitive and
  unsupported input withholding, complete-diff budgets, model context bounds,
  HTTP context-rejection splitting and a hard maximum of 64 requests.
- Independent configuration serialization/default test: passed.
- Draft state regression tests: 4 passed. Late callbacks cannot overwrite edits;
  scope/incomplete-analysis review cannot be bypassed by changing a character;
  Unicode editing preserves valid boundaries.
- Native application final feature checks: 10 passed, 1 intentionally ignored
  real-model test. Covers explicit stage/commit, partial index preservation,
  read-only selection/cancel, staged-only generation, Chinese/English switching,
  manual draft preservation, stale-index rejection, selected unstage, paste and
  switching from a parent repository into a nested repository's own workspace.
- The ignored real-model test was explicitly executed against local
  Qwen3.5-9B-Q6_K at 127.0.0.1:19240, context 4096: 1 passed in 12.09 seconds.
  It verified recommendation without writes, explicit staging, a Chinese
  Conventional Commit, explicit commit, exact committed Python contents and a
  clean repository afterward.

The final workspace run completed successfully: 739 passed, 0 failed, 11 ignored,
with three known Windows symlink-privilege tests explicitly filtered. The runtime
was set to the installed complete runtime directory for grammar/query discovery.
The first run without that explicit runtime failed the existing folding tests;
an isolated eight-test folding rerun with the runtime configured passed. No
folding product code was changed.

The unrestricted complete editor integration run is also performed as a release
gate. Its final counts and any baseline failures are recorded in the release
README/BUILDINFO.json, which are generated after that run finishes. The preceding
release already documents five unrelated indentation/formatting/symlink failures;
new failures must be investigated before delivery rather than assumed baseline.

## Review findings and regression evidence

Independent whole-feature review found four material issues; each was reproduced
before correction and independently re-read after correction:

1. Selecting a staged rename's destination could also stage an unrelated file
   recreated at its old name. Stage now includes the old path only when it is
   still tracked in the original index. The recreated file remains untracked.
2. A staged deletion and untracked recreation of the same path need cross-record
   partial-stage protection. The recreation is now manual-only by default,
   preserving its own ID/worktree content and the existing staged deletion.
3. Automatic generation after adding files could replace a manual message.
   Existing drafts are now retained and invalidated for explicit `g` or `v`.
4. Killing only Git on timeout left hook descendants able to stage files later.
   Windows operations now terminate the owned process tree and await exit before
   reconciliation/guard release. Unix uses a separate process group. If tree
   termination cannot be confirmed, further writes to that repository are blocked
   for the session with explicit diagnostics.

A real short-timeout regression starts a sleeping pre-commit hook, times out Git
after 250ms, then waits another 1.3s and verifies the hook never writes/stages its
late file. Unix process-group handling is not runtime-tested on this Windows host.
External processes can still race between the final preview check and Git's own
locks; this is optimistic validation, not a promise of global repository isolation.

Additional native RED/GREEN checks caught pasted messages reaching the source
buffer and manual drafts being overwritten after staging. Paste is now consumed
by the focused workspace, CRLF is normalized, and source contents stay unchanged.
The final self-audit also reproduced reuse of the parent repository's workspace
after opening a nested repository. Reuse now checks intervening .git boundaries;
the native test proves the nested branch and its own index are selected.

## Reproducible commands

```text
cargo test -j 1 -p helix-vcs --features git commit
cargo test -j 1 -p helix-term --lib ai_commit
cargo test -j 1 -p helix-term --features integration --test integration ai_commit -- --test-threads=1
cargo test -j 1 -p helix-term --features integration --test integration ai_commit_real_local_model -- --ignored --test-threads=1
cargo check -j 1 -p helix-vcs --no-default-features
cargo check -j 1 -p helix-term --no-default-features
cargo test --workspace -j 1 -- --skip test_normalize_path --skip git::test::symlink
cargo fmt --all -- --check
```

Both backend and full terminal git-disabled checks passed. The backend reports
seven pre-existing unused-variable
warnings in helix-vcs/src/lib.rs. Existing editor integration helpers emit a
dead-code warning for line_feed_handling. These were not suppressed.

## Delivery

The release executable is built from the final source commit, after tests and
review, using:

```text
cargo build -j 1 --release -p helix-term --bin hx --target-dir target/publish-build
```

The release archive's BUILDINFO.json records the exact source commit, binary
version/hash and validation results. Packaging verifies ZIP integrity, every
extracted file hash and isolated runtime/config health. Release uploads are
verified against local SHA256 digests before publication. Installation retains
the prior executable/configuration backup and does not kill existing sessions.

First-version boundaries: one commit at a time, whole-file selection, no automatic
push/amend/rebase or multi-commit splitting. Unsupported/oversized/sensitive
contents require manual review; AI correctness still requires inspecting the
actual staged diff and message.
