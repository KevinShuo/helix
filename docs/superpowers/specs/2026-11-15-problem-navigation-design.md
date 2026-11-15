# Diagnostics and compilation navigation

User selected approach C on 2026-11-15: first cross-file diagnostic navigation,
then compilation error navigation. This records that approved direction and the
implementation defaults below. No deployment, installation or Git publishing is included.

## Intent and approaches

Make errors and warnings reachable without manually opening files or finding line
numbers. A (configuration only) leaves compiler output as text; B (LSP navigation)
does not cover builds; C (selected) adds both, sharing navigation and a problem
picker while keeping the sources distinct.

## Phase 1: live diagnostics

- Keep `]d`, `[d`, `Space d`, `Space D` compatible.
- Add `next_problem`, `previous_problem`, `problems_picker`, with `]q`, `[q`
  and `Space E` bindings in normal/select mode. The previously suggested e keys
  already navigate syntax entries/open the file explorer and remain untouched.
- Also expose `:next-problem`, `:previous-problem`, `:problems`. Each accepts
  optional `--source all|lsp|build`, `--severity warning|error|all`,
  `--scope workspace|document`; filters persist in the editor for these commands.
  Default: all sources, errors and warnings, workspace. Severity is a minimum:
  `warning` includes warnings and errors, `error` only errors, `all` all severities.
- Order by normalized absolute path, start position, source and message;
  preserve equal-position messages as individually navigable entries. No wrap:
  explicitly report the end/start. Numeric prefixes apply to mappable navigation.
- Recollect LSP diagnostics on each invocation. Prefer converted current document
  diagnostics for open documents (correct positions after edits), and server
  offset encoding for unopened files. Respect enabled diagnostic providers.
- Retain the selected problem identity across updates; if removed, navigate from
  its last sort position. A cursor moved elsewhere starts from that position.
- Opening a location pushes jump history, reveals folds, selects the range and
  centers it; report message, source and current index. No diagnostic arrival
  automatically moves the cursor. A missing file reports an error.

## Phase 2: compilation

- `:compile <shell command>`, `:recompile`, `:compile-cancel`, `:compile-log`.
  Explicit user commands run in the current editor working directory using the
  configured shell. Remember exact command and cwd for recompile. Reject a second
  build while one runs; cancellation preserves received logs/problems. Replace
  old results only after successful spawn; a shell launch failure preserves them.
- Run asynchronously, drain stdout and stderr concurrently, show a read-only
  split log when requested; new output updates the existing log without focus
  changes. Commands never save buffers automatically. Warn about unsaved files.
- Parse GCC/Clang `file:line[:column]: severity: text`, MSVC
  `file(line[,column]): severity CODE: text`, Rust human diagnostics with following
  `--> file:line:column`, and Cargo `--message-format=json` compiler messages.
  Resolve relative paths against build cwd; handle CRLF, ANSI colors, Windows
  drive letters, spaces, Unicode paths, fragmented output and multiline notes.
  Cargo/Rust columns use Unicode character offsets. Generic compiler text cannot
  distinguish byte/display/character columns: use columns for ASCII lines without
  tabs, otherwise visit the line start with an explicit precision notice. Invalid/out-of-bounds locations
  report stale/unusable positions, never panic or claim precise navigation.
- Keep unlocated errors/logs visible without fabricating a source position.
  Build entries do not masquerade as an LSP server. Filter sources separately.
  Do not deduplicate LSP and compiler messages heuristically.
- Each run has an ID. Late events from canceled/previous runs are discarded.
  Cancellation kills the owned process tree; process ownership also ends on exit.
- Bound retained log to 1 MiB, line input to 64 KiB, located entries to 10,000;
  visibly report truncation and continue draining child output.
- Build locations are snapshots: mark as potentially stale for modified buffers
  or changed on-disk files. Do not attempt to track compiler locations through edits.

## Architecture and builder decisions

`helix-view::problems` owns shared problem/filter/navigation/build state.
`helix-term::commands::problems` collects LSP entries and owns picker/navigation.
`helix-term::handlers::compilation` owns parsing/execution/log callbacks; isolate
the parser and runner in child modules. Reuse existing selection, jumps, picker,
TaskController and callback dispatch. No new dependencies required. Build ownership
uses a kill-on-close Windows Job Object or a Unix process group. Use raw command
text for cmd.exe, whose parsing differs from CommandLineToArgvW escaping.

Build result persistence across editor restart, arbitrary terminal capture,
debugger exception navigation, automatic build-on-save, and configurable parser
languages are deferred. LSP navigation only sees diagnostics the servers have
actually reported; it does not force a project-wide scan.

## Acceptance

Real editor tests cover commands/keymaps, cross-file forward/backward traversal,
equal locations, filters, live replacement/removal, UTF-16 and folded targets,
missing files and unsaved state. Parser tests cover all formats and limits.
Process tests use local fixture processes for streamed output, both pipes,
nonzero exit, missing executables, cancel and obsolete-run events. Run focused
tests serially (`-j 1`) because this workspace has prior compiler memory failures,
then formatting and compilation checks, and a fresh independent review.
