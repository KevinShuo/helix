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

## Static Commands

Static commands take no arguments and can be bound to keys. Static commands can also be executed from the command picker (`<space>?`). The built-in static commands are:

{{#include ./generated/static-cmd.md}}
