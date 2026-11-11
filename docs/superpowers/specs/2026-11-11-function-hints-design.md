# Function reference and Git author hints

The user requested the two hints shown beside a function in the supplied screenshot:
reference count and Git commit author. They explicitly selected the author of the
most recent commit modifying the function, rather than the largest contributor.

## Design

Display virtual text at the end of each visible function declaration, for example
`2 usages  wangs`. This text must never modify the buffer. Use LSP document symbols
to identify functions, methods and constructors, and LSP references with declarations
excluded to count usages. Deduplicate locations, and do not represent unsupported or
failed queries as zero usages. Existing `goto_reference` remains the navigation command.

Use Git range history to find the latest commit touching each function, including
deletion-only commits. Map unsaved buffer ranges to HEAD; a modified function retains
its historical author with `*`. New/untracked functions have no invented author.
Only hierarchical symbols provide reliable full function ranges for Git lookup;
flat symbol responses can still provide reference hints. Advertise hierarchical
symbol support and preserve parent names in the existing symbol picker.

Add independent opt-in `[editor.function-hints]` settings `references` and `git-author`.
Support theme keys `ui.virtual.function-hint.references` and
`ui.virtual.function-hint.author` with the existing inlay-hint style as fallback.

## Approaches

- Native LSP CodeLens depends on server-specific commands and is unavailable for many
  Python servers. It also cannot supply Git history consistently.
- LSP document symbols/references plus Git range history works with existing Helix
  interfaces. This is the implementation choice; no new language-specific parser.
- Searching function names in text is cheap but gives incorrect counts for comments,
  duplicate names and imports; do not use it.

Run only while idle and outside insert mode. Limit work to visible declarations,
bound concurrent requests, cache results, send LSP cancellation for obsolete work and reject stale
responses after edits, view closure, LSP restart, path or config changes. Periodic
idle refresh picks up external commits and reference changes. Respect workspace Git
trust. Missing servers, Git, history or capabilities simply omit the relevant hint.

## Constraints

Reuse the current checkout and preserve pre-existing uncommitted AI-completion work.
No user config changes are required. After implementation, the user requested
committing this feature and pushing it to `KevinShuo/helix` on `master`.
Both hints need LSP document
symbols; usage counts additionally need references support. Git author hints require
the Git build feature and executable. Keep non-Git builds compiling.

## Verification

Test nested/flat symbols, UTF-16 and CRLF positions, duplicated reference locations,
malformed/out-of-range symbols, hidden/stale annotations and independent switches.
Test Git history with two authors, deletion-only changes, unsaved line shifts and
untracked files. Run relevant crate tests, formatting and the standard cargo tests.
