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
