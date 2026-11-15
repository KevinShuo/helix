# Delete the current file

The user selected confirmation followed by direct deletion on 2026-11-15,
after reviewing the proposed command, unsaved-change protection, and buffer
closing behavior.

## Behavior

- Add `:delete-file` for the current document only, without path arguments.
- Show the file path and require an explicit `yes` confirmation. Escape,
  an empty answer, or another answer leaves the file and buffer unchanged.
- Reject unsaved changes; `:delete-file!` permits discarding them but still
  requires confirmation. Neither command saves the document automatically.
- Flush pending writes before deletion. Use the existing file deletion and
  language-server notification mechanism, then close the deleted buffer.
- Reuse buffer closing behavior to show another document or an empty scratch
  buffer without exiting the editor.
- Reject scratch buffers, paths missing from disk, directories, and merge
  documents. On deletion failure, retain the buffer and report the error.

## Approaches

The user chose direct deletion with confirmation, reusing the existing editor
file operations. Moving to the system recycle bin would allow recovery but
requires additional cross-platform dependencies and handling.

## Scope and implementation choices

This is a small command addition. No new default shortcut, dependency, file
explorer action, or arbitrary-path deletion is needed. Existing command and
prompt APIs, documentation conventions, and integration test helpers apply.
Confirmation wording and accepting only `yes` are implementation choices.
