# Git tools implementation and validation

User-approved behavior: Space g unchanged; Space v is the Git group; default
bottom window with z/F11 fullscreen toggle. This record will be finalized after
the completion checks.

## Decisions and progress

- Reused the current checkout on a new codex/git-tools branch, following the
  app's preference for the existing checkout. No remote publication is planned.
- Public backend module is git_tools (source log.rs), avoiding shadowing the
  existing log crate. It uses the existing Tokio/Git/gix dependencies.
- The compositor exposes reserved editor space through Context::editor_area;
  command/status rendering keeps the full terminal area.
- z is the primary fullscreen key because terminals may intercept F11.
- Queries cancel on replacement/close and callbacks compare instance/version
  identifiers. Confirmed Git operations continue to completion if their panel
  closes, and still report their result.
- The existing Nucleo picker callback needed an integration-only runtime guard:
  its Rayon thread otherwise called runtime-local redraw state without entering
  the application's Tokio runtime, aborting the Space g regression test. The
  normal build retains the existing redraw callback.
- The test driver resets the editor idle timer before waiting, matching the
  existing AI-completion test helper. TestBackend resize changes both backend
  dimensions and terminal buffers to model a real terminal resize.

## Evidence so far

- Baseline helix-vcs: 28 tests pass (the two known Windows symlink privilege
  tests excluded).
- Tests were written and observed failing before the repository API, panel,
  and Space v keymap were implemented.
- Real-repository tests cover graphs, local/remote/tag refs, messages, path/
  author/date/hash filters, paging, empty/detached HEAD, linked worktrees, root/
  rename/delete/merge diffs, dirty status, unborn worktrees and local remotes.
- A failing test caught unborn previews displaying index contents instead of
  current working contents. The preview now compares the working file to empty.
- A failing test caught push.default=matching publishing unrelated branches.
  Push now supplies only HEAD:refs/heads/<destination>, with no force/follow-tags.
- Failing UI tests caught pointer movement stealing editor focus, modifier keys
  confirming checkout, and the missing full-message popup. These cases now have
  direct integration coverage.

## Pending final checks

Complete backend/UI/integration suites, feature-disabled checks, scoped
formatting, independent review, production build and executable/runtime check.

## Independent review and follow-up

The read-only reviewer found five Important issues (no Critical issues):
repository reuse across nested repositories, symlink/alias membership in the
unsaved-buffer guard, fullscreen editor focus handoff, concrete push destination
confirmation, and operation serialization after panel close/reopen. All five
were verified against the source and are being fixed with reproducing tests.

Pre-review checks passed: 36 VCS tests (two known Windows symlink privilege tests
excluded), 41 terminal library tests, four native Git integration flows, the
workspace library/test suite (with the three previously documented Windows
symlink tests excluded), and helix-term --no-default-features compilation.

The review explicitly accepted the documented 5,000-commit limit, first-parent
merge diffs, combined working-tree preview, binary preview limitations, and
remote checkout refusing to overwrite an existing local branch.

All five Important findings are now resolved:

- Every explicit Git command discovers the active file's repository
  asynchronously and reuses a panel only for the same canonical worktree.
- Canonical filesystem paths, including directory junctions/symlinks and
  nonexistent tails, are used consistently for history and buffer membership.
- Editor focus handoff docks the window before opening a working/historical
  file or invoking window commands. Historical buffers start at the first line.
- Push resolves an immutable plan before confirmation: local branch/commit,
  remote name and destination ref. Changes to tracking config cannot redirect a
  confirmed push, and a changed HEAD requires a new confirmation.
- Operations are serialized in helix-vcs by canonical worktree Git directory,
  independently of panel lifetime. Completion refreshes a reopened matching
  panel as well as reporting the result.

Both backend review regressions fail before the fixes and pass afterward.
The seven review integration cases are green, along with the four original
native Git flows: 11 Git integration tests pass. They cover nested-repository
navigation, aliased unsaved buffers, three fullscreen handoffs, push target
confirmation/config changes, and a slow pre-push hook across close/reopen.

The post-review workspace run passes 474 tests. Git-disabled compilation passes
with the seven existing unused-parameter warnings in DiffProvider's disabled
branches.

The full existing integration run finished with 204 passed, six failed and three
ignored. All 11 Git tests passed. Three failures are Windows symlink privilege
error 1314. The other failures are test_open_below_at_block_end_stays_in_block,
tree_sitter_motions_work_across_injections, and the bash-based
test_write_quit_auto_format_exits_after_format. The syntax/indent assertions are
outside the Git implementation; the installed runtime also reports a Go query
and parser mismatch. The formatter resolves bash through the WindowsApps/WSL
shim unless Git Bash is put first on the test process PATH. No operating-system
privilege or persistent PATH setting was changed for these checks.

An additional real-repository regression reproduced merge.autoStash=true
overriding the intended no-stash behavior. Merge and pull now explicitly pass
--no-autostash; the final VCS suite passes 39 tests with the two known Windows
symlink-creation tests excluded. No test invokes network operations against the
user's repository or configured remote.
