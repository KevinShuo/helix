# Task 5 — native conflict list and three-pane workspace

Base: `7eb4f12dd01df657195e54b2d251df89aed98a10`.

## Implementation

- Added a separate full-screen `ConflictList`, preserving the existing GitPanel instance, dock/fullscreen settings, selection and focus. It reads structured conflicts, shows operation identity, exact display-safe paths, degradation reasons, live drafts/staged/stale/write-failure state, and exposes independent Finish/Abort previews and confirmations.
- `OpenAction::Conflicts`, `git_resolve_conflicts` (`Space v r`), and `git_merge_menu` (`Space v m`) are included by the root's explicit Task 5/6 integration ruling. Existing Space g and every existing Space v action are retained.
- `MergeEditor` is an overlay around the native pathless Document/View. It never owns editable text. Source panes are immutable ropes, strip terminal control characters, clip by terminal display width and have read-only input handling. Wide layout activates at exactly 120 columns / 18 rows with 30/40/30 widths; narrow tabs and Base preview use the same native draft. Source line maps are cached by Document version.
- Contextual `Component::editor_area_for(area, editor)` makes both GitPanel suspension and merge geometry depend on the live Editor session. Native :q, buffer/file switch and splits immediately stop the old overlay; closing a result does not reactivate the hidden list. Focus gained/lost events pass through to native EditorView even in confirmations/read-only preview. Bufferline is disabled only during an active merge; command line is reserved exactly once.
- All toolbar/menu decisions use one action handler. Block choices checkpoint history, apply the model transaction through Document, mark reviewed only after successful apply and checkpoint again. Native edits/undo/redo remain Task 4's original path. Whole-file acceptance and rebuilding require their own confirmations and do not write disk. Whole-file acceptance requires review again.
- `MergeCandidate::rebuild()` explicitly constructs a generated model only when backend-verified ranges are unambiguous. It does not rewrite/fake `matches_working`; ordinary initialization still preserves manual working content. A regression verifies explicit reconstruction and refusal of ambiguous ranges.
- Apply freezes the draft before dispatch, checks trust and every ordinary dirty matching-path buffer, encodes the original file format and invokes the opaque-token backend. Apply, stage-only retry, Finish and Abort are application Jobs with `wait_before_exiting`. Completion is an Editor callback so even Jobs.finish without a compositor unfreezes surviving drafts; optional list refresh is queued separately and cannot recreate a closed component.
- `MergeWriteStatus` lives with the Document: Draft / Staged / NeedsRefresh / Retry(token, Document version, message). A retry survives leaving/reentry, and subsequent edits invalidate its version. Written-but-unstaged results reload all matching clean buffers, including hidden buffers, and remain visibly unstaged. All clean matching docs are also refreshed after success.
- Reentry compares fresh data against the immutable draft token without replacing the draft. Stale or externally resolved drafts remain accessible. Confirmed discard/refresh only replaces an existing draft after replacement data/candidate loaded successfully, and checks the original session/document version before committing that read result. Rebuild reads also bind component instance/session/document version. List queries bind component instance/root/request version and are canceled on drop.
- Finish shows all staged paths, HEAD, parents and default merge message, with scrolling; Abort presents its recovery limitation and refuses outstanding drafts/unsaved buffers. Unsupported Git operation kinds expose no functioning Finish/Abort action. Staged rows remain in the in-process list and are sorted with unresolved rows by the actual path.

## APIs for Task 6

- `ui::git::conflicts::open(&mut commands::Context)` — manual list entry, preserving any active draft.
- `ui::git::conflicts::present(&mut Compositor, &mut Editor, PathBuf)` — show/refresh a list after the caller validates its own trigger.
- `ui::git::conflicts::present_snapshot(&mut Compositor, &mut Editor, GitRepository, GitConflictSnapshot)` — present already-read structured state, no second query. The caller owns triggering panel/operation validation.
- `ui::git::merge::{open_merge, merge_action, MergeUiAction, MergeLayout}` — requested manual path/action/layout integration APIs. `open_merge` takes an exact index-relative path.
- `ui::git::merge::reload_clean(editor, root, target)` — all clean ordinary documents, using the existing synced-view helper; target=None is repository-wide.
- `Editor::git_context_path()` — moved from root's uncommitted Task 6 work into Task 5's owned `helix-view/src/merge.rs` so this commit has no Task 6 dependency. Ordinary Git entries capture the active worktree before leaving a merge; file history uses the draft's real target path.

## Verification evidence

- Behavioral RED: `cargo test -p helix-term --lib ui::git -- --test-threads=1` reported the new `merge_layout_wide_narrow_tiny` failure at header/result overlap, with the original 7 GitPanel tests passing.
- Initial integration invocation accidentally omitted `--features integration` and ran zero cases. It is not counted as verification. The corrected manual-entry run failed while the incomplete async UI was being integrated; later index assertions exposed that one native idle tick does not mean a Git write Job completed. The harness now polls actual UI read readiness and only actual Apply/Retry inputs await draft busy completion (native tests intentionally set busy themselves).
- An accidental overlapping integration rebuild hit Windows `LNK1104` for the running integration executable. The superseded run was stopped; subsequent term builds/runs are serial. This is recorded as an infrastructure/run-coordination failure, not a feature failure.
- Targeted real manual list → native Incoming decision → Apply → stage-0 verification passed (1/1, 24.04s). The unresolved first Apply was rejected, actual disk text matched incoming and no unresolved index entries remained for that path.
- Full 14-case run and final focused commands: results pending below.

Integration coverage added: manual menu flow and Apply review gate; narrow keyboard previews; wide mouse source controls; native close/reentry and immediate geometry removal; independently confirmed whole-file acceptance; explicit rebuild of external manual work; stale Apply and confirmed refresh preservation; dirty hidden ordinary buffer refusal; written-not-staged retry over reentry; all-clean hidden buffer refresh; trust revocation; confirmed Apply completing after native :q without reopening; two-file resolution and independent Finish creating two parents; cancel/confirm Abort.

## Scope and remaining handoff

No automatic operation/focus discovery is included; root owns Task 6 handler/scan state and the EditorView focus hook. Root's book, validation document, editor scan fields/module, handler and discovery tests are intentionally excluded. No new third-party dependencies. Report is local ignored task evidence, matching prior task reports. No push.

## Final validation (continuation)

- Full `cargo test -p helix-term --features integration --test integration git_conflicts -- --test-threads=1`: **13 passed / 1 failed**, 239.67s. The one failure was `git_conflicts_ui_finish_and_abort_are_separate_confirmations`: refreshed rows appended the staged file after unresolved files, so the selected index moved from file.txt to second.txt. The test's next-row selection therefore returned to file.txt. Fixed by sorting all unresolved/draft/staged rows together by the original path.
- Focused rerun of that exact Finish test: **1 passed / 0 failed**, 44.84s. It resolves and stages both files, verifies MERGE_HEAD still exists before Finish, inspects both staged paths in the confirmation and verifies the final commit has two parents.
- Final widened `git_conflicts_ui_preview_readonly_and_native_close`: **1 passed / 0 failed**, 21.47s. Includes actual mouse down/wheel in a source pane, attempted edit keys, switching back to Result via its header, native :q geometry removal and draft reentry.
- Therefore every one of the 14 conflict integration cases has passed. The full 14-case batch was not redundantly repeated after the isolated ordering fix; root explicitly requested not repeating unchanged passing tests. The new focus-event pass-through will also be exercised by root's Task 6 focus integration.
- Debug progress output was removed from tests. Existing Windows helper warning `line_feed_handling` remains. `present_snapshot` is intentionally unused until Task 6 attaches automatic discovery and currently produces a dead-code warning. No feature failure is hidden by those warnings.
- Scoped `git diff --check` passed; only normal Windows LF/CRLF advisories were emitted.
- Final keymap command `cargo test -p helix-term --lib git_group -- --test-threads=1`: **1 passed / 0 failed** (Space g and existing Git bindings retained, new r/m checked).
- The new Unicode-render assertion initially had two test-only type errors (`Buffer::get` returns Option; `Cell::symbol` is ArrayString). Corrected to `get(...).unwrap().symbol.as_str()`; these compile failures are not counted as behavioral RED. Final UI command result follows.
- Parent requested immediate scoped handoff after UI/lib succeeds; model unit, no-git and broad final regressions are explicitly deferred to root's final validation. The new rebuild unit is present, but is not claimed run by Task 5.
- Final self-review also resets manual source scroll offsets on Next/Previous so navigation returns to linked line-map positions, preserves reload failure messages, and passes FocusGained/FocusLost through both overlay components.
- Final `cargo test -p helix-term --lib ui::git -- --test-threads=1`: **11 passed / 0 failed**, including layout/read-only/review/Unicode cases and all existing GitPanel unit tests.
- Final staged `git diff --cached --check` passed. All 17 owned code/test files were committed, with no root draft paths included.
- Commit: `2f9a85446214ec6d30e71adb96380ea9a822d16e` — `feat: add native three-pane conflict workspace`, parent `7eb4f12dd01df657195e54b2d251df89aed98a10`. No push.
- Remaining worktree changes are root's book/validation drafts, scan controller registration, handler and discovery/acceptance tests. No cargo process from Task 5 is running.
