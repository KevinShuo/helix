# Native Git merge conflict workflow — validation

2026-11-12, resumed in `I:/DEV/helix` on `codex/git-merge-conflicts`.
Implementation commits: `b46e6172`, `e7c9c357`, `96c94bfe`. Implementation and
Windows acceptance are complete. Full-suite baseline failures remain explicitly
listed below; this is not a claim that all repository tests are green.

Environment: Windows, PowerShell, Rust 1.90.0, Git 2.56.0.windows.4.
Git Bash is placed first on the test process PATH. Builds use the existing
runtime grammars and `CARGO_BUILD_JOBS=2`; no dependency versions were changed.
All merge fixtures are temporary repositories, separate from the user checkout.

## Current verification

| Check | Actual result |
| --- | --- |
| Latest integration executable, `git_conflicts --test-threads=1` | 33 passed, 0 failed; 169.93s |
| `cargo test --workspace --no-fail-fast -- --test-threads=1` | 645 passed, 3 failed, 7 ignored; failures below |
| `cargo fmt --all -- --check` | Passed; the pre-existing indent.rs tuple wrapping was normalized |
| `git diff --check` | Passed before implementation commit |
| `cargo integration-test --no-fail-fast -- --test-threads=1` | 260 passed, 5 failed, 4 ignored at b46e6172; one regression repaired, four baseline failures below |
| `cargo integration-test git_tools -- --test-threads=1` after repair | 12 passed, 0 failed |
| `cargo integration-test folding -- --test-threads=1` | 10 passed, 0 failed |
| `cargo integration-test test::commands::write -- --test-threads=1 --skip test_symlink_write` | 32 passed; the three known privilege-dependent cases excluded |
| `cargo test -p helix-view -- --test-threads=1` | 112 passed across unit, integration and documentation tests |
| `cargo check -p helix-term --no-default-features` after final repair | Passed; seven existing no-git VCS unused-variable warnings |
| `cargo build -p helix-term --release` at 96c94bfe | Passed; 3m 05s |
| Real terminal, 160×40 and 80×24 | Passed with the final release executable; details below |

The three workspace failures all report Windows error 1314 (symlink privilege):

- `helix-stdx --test path`: `test_normalize_path`.
- `helix-vcs --lib`: `git::test::symlink` and `git::test::symlink_to_git_repo`.

These tests exercise unchanged code. The VCS failures were already recorded
before implementation; the broader non-library run also exposes the same
privilege restriction in stdx. The feature's symlink-parent test is explicitly
ignored on this runner; Windows junction traversal tests ran and passed.
No system privilege or security settings were changed for testing.

The full integration run also exposes three unchanged write tests with the same
OS 1314 restriction: `test_symlink_write`, `test_symlink_write_fail`, and
`test_symlink_write_relative` in `test::commands::write`.

`test::commands::test_open_below_at_block_end_stays_in_block` fails for a C-style
comment followed by CRLF: the inserted line lacks four spaces. It was reproduced
with the pre-merge executable `integration-b411ed6aee806f9f.exe`, built on
2026-11-11 at 22:02 and containing zero git_conflicts tests. The expected/actual
text is byte-for-byte identical to the current failure; indentation behavior
and its test are unchanged by this feature.

The fifth integration failure was a real regression in
`git_review_operation_survives_reopen_and_refreshes_new_panel`. The operation
callback restricted data refresh to its original panel. Commit `e7c9c357`
restores refresh of a replacement panel for the same worktree while keeping
automatic resolver presentation bound to the original instance/operation/context.
The complete Git tools suite subsequently passed 12/12. The merge suite and
build checks are rerun after that repair; the entire 269-test batch is not
claimed green.

## Workflow and regression evidence

The Application tests use the real event loop and real Git repositories.
The end-to-end case starts with two conflict files and one clean automatic
merge, then independently stages an unrelated file. It combines Keep Both,
manual editing/review and Incoming selection, checks each stage-0 result and
preservation of other staged OIDs, reviews all staged paths in Finish, and
verifies the final commit has the correct two parents and no MERGE_HEAD.

Discovery cases cover menu-initiated merges, ordinary non-conflict errors,
explicit refresh notifications, FocusGained without stealing focus, pathless
merge drafts, switching repositories, closing the panel, and completing an
already-confirmed operation during application exit. Changes groups unmerged
files and routes Enter to the resolver instead of presenting an ordinary patch.

Native lifecycle coverage includes original views/focus/folds/splits, manual
external work, multiple drafts, save/auto-save guards, q/q!, write-all, buffer
close and quit guards, frozen editing/history during Apply, and exact staging.

## Independent reviews and repairs

Task 5 and the whole branch received separate read-only reviews. All Important
findings have reproducing Application tests and fixes verified RED to GREEN:

- Read-only previews cannot trigger hidden Review/Apply footer hitboxes.
- Staged results reject native edits, undo/redo and side choices, and are visibly
  read-only; their displayed contents continue to match the staged result.
- A retained staged draft is stale if its path becomes conflicted again.
- Native buffer switching back to an older draft restores panes and actions.
- Refresh after resuming a draft from another worktree keeps the other draft
  intact and reloads the intended worktree. Pending requests retain confirmation
  parameters; callbacks validate session, document version, root and path.

A 40-column regression also verifies that whole-file replacement and rebuild
confirmations show the discarded side or draft edits before confirmation.

Real-terminal QA identified two additional issues, each reproduced by a failing
Application test and repaired in `96c94bfe`: canonical Windows scan paths now
use the editor's normal path form so configured trusted-workspace globs match;
automatic Git reloads retain folds for unchanged text. Changed text still
clears folds, and explicit `:reload` retains its original clear-folds behavior.

Deferred Minor: the native buffer picker still labels pathless merge drafts as
unnamed scratch buffers. Target names are shown in the conflict list and merge
headers. This does not change the draft's worktree/path binding.

## Final real-terminal acceptance

Both runs used the final release executable in isolated temporary repositories,
an explicit test configuration and Windows ConPTY. No user repository merge was
performed and no globally installed executable was replaced.

- 160×40: started a real merge from the Git branch menu; the conflict list opened
  automatically. Native SGR mouse clicks entered a read-only source preview and
  typing there did not alter the draft. Keep Both plus manual editing/review was
  applied to the first file; Incoming was applied to the second. Finish preview
  included both files, the clean automatic merge and an unrelated staged file.
  Verified the actual staged/committed contents, unchanged unrelated OID,
  correct two parents, missing MERGE_HEAD and clean status. Closing the list and
  Git panel restored both original splits and the unchanged C++ fold.
- 80×24: an external fixture merge was discovered on native terminal focus gain
  without opening a resolver or stealing focus. Tested Base/Current read-only
  previews, Result editing, q keep and reentry with the same manual draft,
  q! discard, Abort cancel and separately confirmed Abort. Verified unchanged
  original HEAD, restored original file contents, missing MERGE_HEAD and clean
  status. Configured trusted-workspace globs produced no false trust error.

## Supported boundary

Ordinary merge, regular UTF-8 text, BOM and LF/CRLF preservation, at most 2 MiB
and 50,000 lines per input/result. Binary/special files, path decisions, mixed
newlines, unsupported attributes and rebase/cherry-pick require external
resolution. MERGE_AUTOSTASH created externally also requires external
Finish/Abort. Apply stages one exact path; Finish is separately confirmed and
does not push. Drafts are retained within this process only.

This host establishes Windows behavior. Linux/macOS execution and real symlink
permissions were not independently validated here.
