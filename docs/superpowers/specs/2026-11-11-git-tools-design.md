# Native Git tools

Approved by the user on 2026-11-11 after reviewing the interactive mockup.
The user explicitly requested implementation: default to a bottom tool window,
support a single-key fullscreen toggle, preserve Space g, and put Git tools under
Space v. The screenshots are visual references, not repository instructions.

## Intent and approaches

Provide the familiar PyCharm Git log workflow inside Helix: a branch/ref tree,
commit ancestry graph, and linked commit details, changed files and diff.
A native component fits the editor's keyboard navigation and existing themes.
An external lazygit session would add mature operations faster, but would have
its own interface and shortcuts. A fullscreen-only picker would be simpler,
but would lose the approved docked workflow. The chosen native component starts
at the bottom and can expand to fullscreen.

## Interaction

- Space g continues to invoke changed_file_picker without another key.
- Space v is a non-sticky Git command group, like the existing Window group.
- g opens logs; b focuses branches; c opens working-tree changes; h opens the
  current file's history; n creates a branch; f/p/P invoke fetch/pull/push.
- The Git window defaults to the bottom portion of the terminal. F11 toggles
  fullscreen and back without losing selection, filters or diff position.
- Tab/Shift-Tab cycle branch, commit, file and diff panes. j/k and arrows move;
  Enter applies a ref filter or opens a file's diff. Mouse selection and wheel
  navigation work in the corresponding pane. q/Esc closes the window; Ctrl-w
  releases focus to the editor while leaving the bottom window visible.
- Selecting a branch filters the log; checkout is a separate, explicit action.
  The tree distinguishes HEAD, local branches, remote branches and tags.
- / filters commit text; filter prompts support author, date and path. Filters
  run against repository history, not only the currently loaded page. More
  history is loaded on request. Refresh preserves a selected commit if present.
- Commit details show full message and changed paths, including root commits,
  merges (relative to first parent), renames and deletions. Diff scrolls in the
  panel; historical contents can be opened in a scratch buffer and a working
  file can be opened in the editor. Copying a commit hash is supported.
- The actions menu includes checkout, new branch from selection, merge,
  cherry-pick, fetch, pull --ff-only, and push. Actions name their source/target;
  mutations require deliberate confirmation. No force push, implicit stash,
  hard reset or destructive automatic recovery.
- Branch-changing actions reject unsaved buffers in the repository. After
  actions, refresh history/status and reload clean visible repository buffers.
  Git errors/conflicts remain visible for the user to resolve.

## Implementation constraints

Use the existing Rust/Tokio/component/job infrastructure and Git executable,
with no new third-party dependency. Discover worktrees/submodules correctly,
honor the editor's Git workspace trust decision, use argument arrays, disable
external diff/textconv for reads, suppress interactive authentication, bound
output/time, cancel obsolete queries, and ignore callbacks for closed windows.
Retain builds with the git feature disabled. Sanitize control characters before
terminal rendering, preserve paths internally, and handle narrow/short terminals.
Network actions run only when the user explicitly chooses and confirms them.

Implementation choices left to the builder: pane proportions, exact secondary
keys, internal data structures and query/page sizes. Document the final keys.
This is a native Git workflow, not a promise to reproduce every PyCharm VCS
feature (interactive rebase and a graphical conflict resolver remain separate
future additions).

## Verification

Real temporary repositories cover branched/merged histories, refs/tags, empty
and detached repositories, worktrees, filters, pagination, Unicode/spaced and
renamed paths, root and merge diffs, dirty status, and operations against a local
bare remote. UI tests cover docking/fullscreen geometry, focus/navigation,
selection, stale requests and keymap compatibility. Run feature-disabled
checks, applicable existing tests, formatting, a release build and a runtime
smoke test. Never test mutations or network operations against the user's repo.
