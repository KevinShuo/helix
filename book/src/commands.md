# Commands

- [Typable commands](#typable-commands)
- [Static commands](#static-commands)

## Typable commands

Typable commands are used from command mode and may take arguments. Command mode can be activated by pressing `:`. The built-in typable commands are:

{{#include ./generated/typable-cmd.md}}

To create a file beside the currently focused file, press `<space>n`, enter
the filename, and press Enter. The prompt shows the destination directory.
You can also use `:new-file filename` (quote paths containing spaces). Relative
paths are resolved against the current file's directory, or the current working
directory for scratch buffers. The new buffer opens in insert mode; the original
buffer remains available. Existing files and named buffers are rejected.

No file or directory is created until you save. Use `:w` to save, or `:w!` if
the path requires new parent directories. Escape cancels the prompt. The `:new`
command continues to create an unnamed scratch buffer.

Use `:gitignore` (or `Space v i`) to generate an editable `.gitignore` draft.
Helix checks project markers in the nearest Git working-tree root, or the
workspace/current file's directory when no Git root is found. The target path
is shown before generation. Subdirectories are not scanned.

Recognized project markers recommend Rust, Node, Python, Go, Java/Maven,
Java/Gradle, C/C++/CMake and .NET templates. Search the list to add other
templates. Windows, macOS, Linux, VS Code, JetBrains, Vim, Emacs, temporary-file
and dotenv templates are optional and are never selected automatically.
The templates are built in and work offline, even before `git init`.

Move with the arrow keys or `Ctrl-n`/`Ctrl-p`, toggle a template with Tab,
and press Enter to generate from all selected templates, including selections
hidden by the current search. Escape cancels. Review and edit the resulting
buffer, then use `:w` to create the file. No file or directory is created during
selection or generation. Existing `.gitignore` files and buffers are opened
without replacing their contents; automatic merging is not supported.

Templates preserve lockfiles. The dotenv template keeps `.env.example`,
`.env.sample` and `.env.template`; add exceptions for any other example names.
Git already-tracked files remain tracked even if a new rule matches them.

While editing a `.gitignore`, press `Ctrl-x` in insert mode to see completion
suggestions. On an empty line the menu offers project-specific rules and nearby
paths. Type `/build/` to browse that directory, `*.` to complete an extension,
or `!` to find exceptions to earlier rules, including unsaved edits. Candidate
details explain the rule's scope and its effect on a sample path when known.
Paths are relative to the current `.gitignore`, including in subdirectories.
The language's `path-completion` setting controls local path and extension
suggestions. Project template rules remain available when it is disabled.

In a Git worktree, place the cursor on a rule and run `:gitignore-preview` to
see its effect on untracked files under this `.gitignore`. Helix compares the
current buffer with the same buffer minus that one rule, so you can preview
edits before saving. Other ignore files are read from disk. Changed files
appear first. Each file shows its final state and the
source file, line number and text of the deciding rule, including a rule that
excluded a parent directory. Files included by default are marked accordingly.
The preview shows a sample for large result sets and flags a partial scan when
the counts cover only part of the tree. Press Tab to show all files whose
ignore status changes because of the current rule. Use `Alt-i` to show only
newly ignored files or `Alt-r` to show only re-included files. Press the
active filter's shortcut again to show all matches.

Press `/` and type part of a repository-relative path to filter scanned matches,
including files beyond the initial sample. The path filter ignores case and
updates as you type. All three status shortcuts work inside the path prompt
and preserve the query.
Select a file with the arrow keys and press Enter to jump to its deciding rule.
Escape clears the path filter and keeps the current status filter. Outside
the path prompt, `j` and `k` also move the selection. Helix reuses open
buffers, including unsaved edits. Press `Ctrl-o` to return to the previous
position, or Escape to close the preview without jumping.

Use `:delete-file` to permanently delete the current file from disk and close
its buffer. The confirmation prompt shows the path: type `yes` and press Enter
to delete, or press Escape to cancel. Any other answer also cancels. Files with
unsaved changes are protected; use `:delete-file!` to discard those changes,
still with confirmation. Deletion does not move the file to the recycle bin.
Scratch buffers, files that do not exist on disk, and merge documents cannot
be deleted. Other open buffers remain available; deleting the last file opens
an empty scratch buffer without quitting the editor. Neither command accepts
a path argument.

Use `:ai-doc` to preview reST documentation for the function at the cursor,
then Enter to insert or Escape to cancel. `:ai-doc-cancel` cancels a pending
request. Requires `[editor.ai-doc] enabled = true`; see
[AI documentation configuration](./editor.md#editorai-doc-section).

Use `:ai-commit [intent]` to open the native Git commit workspace. AI recommends
files without staging them. Review the file diffs, select candidates with Space,
then press `s` to stage selected complete files. Existing staged changes are
always included. Press `g` to generate a Conventional Commits message from the
actual index, `l` to switch Chinese/English, `e` to edit, and `C` to commit.
Escape finishes message editing; it does not commit. `:ai-commit-cancel` cancels
analysis while keeping the index and draft. See [commit configuration](./editor.md#editorai-commit-section).

## Static Commands

Static commands take no arguments and can be bound to keys. Static commands can also be executed from the command picker (`<space>?`). The built-in static commands are:

{{#include ./generated/static-cmd.md}}
