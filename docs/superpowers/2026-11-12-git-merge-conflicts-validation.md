# Native Git merge conflict workflow — validation

Status: paused checkpoint at the user's request on 2026-11-12. Implementation
and final acceptance are incomplete. See the [handoff](2026-11-12-git-merge-conflicts-handoff.md)
for the exact resume point. Pushing this checkpoint is not final feature approval.

Environment: Windows, PowerShell, Rust 1.90; temporary real Git repositories.
User repository merges/conflicts were not used as test fixtures.

## Baseline

Before implementation, `cargo test --workspace --lib` failed the existing VCS
tests `git::test::symlink` and `git::test::symlink_to_git_repo`: Windows error
1314 (required privilege not held). A run excluding symlink tests also exposed
an existing intermittent AI runtime port collision in
`handlers::ai_completion::server::tests::local_model_reports_a_missing_runtime_promptly`.
Final verification will rerun the affected test and distinguish these from
feature failures.

Existing warning output includes seven unused parameters in the no-git VCS
provider and the Windows integration helper's unused `line_feed_handling` field.

## Executed feature checks

| Area | Command / evidence | Result |
| --- | --- | --- |
| Structured stage reads | `cargo test -p helix-vcs --features git git_conflicts` after Task 1 | 6 passed |
| Pure model / real merge-file | `cargo test -p helix-vcs --features git merge::` after Task 2 | 22 passed |
| Pure model without Git | `cargo test -p helix-vcs --no-default-features merge::` | 15 passed |
| Compatible VCS suite after Task 2 | Full VCS tests excluding existing symlink cases | 67 passed |
| Write/stage/Finish/Abort backend | Task 3 targeted conflict suite | 28 passed, 1 Windows symlink test ignored |
| Native draft lifecycle | `cargo integration-test git_conflicts` after Task 4 fix | 7 passed |
| No-git editor build | `cargo check -p helix-term --no-default-features` after Task 4 | Passed; existing VCS warnings |
| Scan lifecycle state | Task 6 initial pure state tests | 2 passed; UI not connected |
| Git UI unit tests | `cargo test -p helix-term --lib ui::git -- --test-threads=1` after Task 5 | 11 passed |
| Git keymap compatibility | `cargo test -p helix-term --lib git_group -- --test-threads=1` | 1 passed |
| Native conflict workflow | Task 5 full batch + focused regression | 13/14 initially; ordering fixed, failing Finish case and expanded mouse case each passed on rerun |
| Paused checkpoint compile | `cargo check -p helix-term` | Passed, 24.44s; unused `present_snapshot` awaits Task 6 wiring |
| Paused checkpoint whitespace | `git diff --check` | Passed |
| Paused checkpoint formatter | `cargo fmt --all -- --check` | Blocked by unchanged `helix-core/src/indent.rs:906` formatting already present at baseline `4bd21126f`; no unrelated source change made |

Native lifecycle tests use the actual Application event loop and cover original
ViewIds/focus/folds, unchanged disk while editing, native undo/redo review
invalidation, frozen text/history during Apply, direct and typed save guards,
q/q! keep/discard, split restoration, external manual text preservation, multiple
drafts, write-all with a hidden original view, retained unapplied-draft warning,
reload-all, quit guards, and closing buffers from the saved layout.

Tasks 2, 3 and 4 received independent task reviews: spec compliant and quality approved.
Task 5 independent review is pending. Its complete implementation report is
[saved here](reports/2026-11-12-git-merge-conflicts-task-5.md). The two Task 6/7
integration child files saved in this checkpoint are not registered or tested.

## Outstanding final acceptance

- Complete list/workspace/actions/discovery and end-to-end real Git tests.
- Run the final scoped regressions, formatting, no-git build and release build.
- Perform terminal smoke checks where the environment permits, explicitly
  recording any platform or interactive coverage limitation.
- Complete independent whole-branch review and resolve blocking findings.
- Verify push to the personal `fork` remote's `codex/git-merge-conflicts` branch.

## Supported boundary

Ordinary merge, regular UTF-8 text (BOM and LF/CRLF preserved), at most 2 MiB and
50,000 lines per input/result. Unsupported operations/file types/attributes use
external resolution. A merge with external `MERGE_AUTOSTASH` requires external
Finish/Abort; Helix-created merges use `--no-autostash`. Drafts are retained only
within the current process.
