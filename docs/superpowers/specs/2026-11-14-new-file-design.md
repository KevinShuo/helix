# New file beside the current buffer

Approved by the user on 2026-11-14: `Space n` creates a named buffer in
the current file's directory; `:w` creates the disk file.

- Prompt for a filename while displaying the destination directory. Enter
  opens the named buffer in insert mode; Escape has no side effects.
- Relative paths resolve against the current buffer's parent directory,
  falling back to the editor working directory for scratch buffers.
- Retain the original file buffer and unsaved edits, and detect the new buffer's
  language normally. Empty scratch buffers follow the editor's replacement policy.
- Reject empty names, directories, existing files, and paths already assigned
  to another buffer. Never overwrite or switch to an existing file.
- Support nested paths without creating directories or files during new-file
  handling. Existing save rules apply: `:w!` creates missing parent directories.
- Provide `:new-file <path>` using the same behavior. Keep `:new` unchanged.

## Approaches

The user chose a direct `Space n` prompt over extending the file explorer:
it is quicker when the destination is the file currently being edited.
Reuse the editor's existing deferred file opening and saving mechanisms.

## Scope

No file templates, automatic directory creation, or file-explorer changes.
Implementation detail: use the existing prompt mechanism;
all filesystem writes remain in the existing save path.
