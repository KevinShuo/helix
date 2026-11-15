# AI commit fixes validation — 2026-11-16

## Changes

The generic text error was shared by file summaries, recommendation reasons,
generated subjects/bodies and manual messages. Summaries previously allowed only
160 UTF-8 bytes, rejecting a 160-character Chinese summary. They now permit 160
Unicode scalar values, with the matching prompt and a derived 640-byte bound.
Aggregation still checks actual serialized UTF-8 bytes and model context. No
summary or diff is truncated. Errors identify the field and applicable limit.

The second reported error was the explicit recommendation eligibility check.
Models can include already-staged, partially-staged or unanalyzed files despite
the prompt. Local state now supplies `selectable_ids` and filters those known
ineligible suggestions while preserving valid selection order. Excluded choices
receive local explanations and a visible count. Unknown and duplicate selected
IDs, unknown/duplicate/missing reasons, invalid JSON and text remain rejected.
Coverage (`incomplete`) is separate from the number of excluded suggestions.

Recommendation never writes Git. Explicit staging, draft review after scope or
coverage changes, and explicit commit requirements remain in effect.

## Regression evidence

Four diagnostic/Unicode tests failed on the previous implementation with the
generic text error before the first correction. Two recommendation tests then
failed with the exact second reported error before filtering was implemented.

Final focused client run: **21 passed, 0 failed, 1 ignored**. It covers mixed
eligible/ineligible recommendations, all-staged recommendations, whitelist data,
160/161-character Chinese boundaries, unchanged summaries, useful errors and
continued rejection of unknown/duplicate IDs.

Native application run: **13 passed, 0 failed, 1 ignored**. The added test returns
all files as model selections, checks the visible exclusion notice and surviving
valid choice, and verifies byte-identical index, unchanged HEAD and unchanged
working file for a partially staged file. Existing explicit staging/commit,
cancellation, stale state, manual draft and language regressions also pass.

Workspace unit/documentation run: **833 passed, 0 failed, 17 ignored, 4 filtered**.
Three filters cover existing Windows path/symlink limitations. The fourth is
`commit_git_timeout_stops_hook_descendants_before_releasing_guard`, which hung in
the initial unfiltered run: Git had exited but its temporary hook shell remained
alive and held the test open. Only this run's test process and hook were stopped.
That initial run exited abnormally and is not counted as passing. No helix-vcs
source was changed. Remaining tests passed in the explicitly filtered rerun.

Two independent read-only reviews found no material issues. Reviews explicitly
did not claim to establish real-model compliance or the unavailable original
model response. Rust formatting and diff whitespace checks passed.

## Commands

Tests selected `HELIX_RUNTIME=I:/DEV/helix/runtime`. Package-specific optimization
was disabled for the terminal test target to shorten compilation; dependency
settings and release build settings were unchanged.

```text
cargo test -j 1 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib ai_commit
cargo test -j 1 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --features integration --test integration ai_commit -- --test-threads=1
cargo test --workspace -j 1 --profile integration --config profile.integration.package.helix-term.opt-level=0 -- --skip test_normalize_path --skip git::test::symlink --skip commit_git_timeout_stops_hook_descendants_before_releasing_guard
cargo fmt --all -- --check
git diff --check
```

Logs remain in ignored `target/ai-commit-*.log` and
`target/ai-commit-selection-*.log`. Release metadata records the committed source,
exact build command, binary version/hash and package verification. The running
old executable is retained under a backup name when Windows prevents replacing
an active image; editor sessions are not terminated.
