# Delete current file validation — 2026-11-15

## Behavior

`:delete-file` deletes the active file and closes its buffer after an explicit
`yes` confirmation. `:delete-file!` also allows discarding unsaved changes and
still requires confirmation. Escape and other answers cancel. There is no
default shortcut and no arbitrary-path argument. Deletion is permanent.

Successful deletion returns to another buffer or an empty scratch buffer.
Scratch buffers, missing files, directories, and merge documents are rejected.
Pending writes finish before deletion; failed deletion retains the document.

## Verification

- Focused integration suite (`cargo test -j 1 -p helix-term --features
  integration --test integration delete_file -- --test-threads=2`): 12 passed.
- Default package tests (`cargo test -j 1 -p helix-term`): 163 unit tests and
  2 doc tests passed; 12 unit tests and 1 doc test ignored by the existing suite.
- `cargo build -j 1 -p helix-term --bin hx`: passed.
- `target/debug/hx.exe --version`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Full integration regression suite (`cargo test -j 1 -p helix-term --features
  integration --test integration -- --test-threads=2`): 346 passed, 2 timed out,
  17 ignored. All 12 deletion tests passed in this run. The two timeouts were:
  - `test::problems::compilation_bounds_output_and_ends_detached_descendants`
  - `test::workbench::declaration_cache_tracks_reopened_documents_and_language_changes`
- Both timed-out tests passed individually using the same compiled integration
  executable with `--exact <test-name> --test-threads=1`: the compilation test
  passed in 14.65 seconds and the declaration-cache test in 3.34 seconds.
  No source changes were made between the full run and these reruns. This is
  consistent with timing sensitivity; the complete suite did not have a fully
  passing run in this session.

The full integration and default package runs selected the repository runtime
with `HELIX_RUNTIME=I:/DEV/helix/runtime`. Builds used one compiler job.

The initial missing-feature run failed seven of ten tests. The focused suite
covers confirmation, cancellation, dirty buffers, forced deletion, Unicode
filenames, multiple views, pending saves, Windows sharing violations, changes
received during confirmation, missing/replaced paths, rejected arguments,
and merge documents. Tests use disposable files, not workspace source files.

## Review

An independent read-only review identified truncated confirmation paths in
narrow terminals. A roughly 200-character path at 40 columns reproduced the
missing filename before the fix. Prompt help now measures its height using
the actual viewport width. The regression passes at 80 and 40 columns,
including a resize while confirmation is open. No other material issues
were reported in the scoped review.

## Workspace and artifact

At the end of feature validation, changes were uncommitted alongside the user's
existing work and no installed executable had been replaced. The user subsequently
requested committing the recent updates, publishing a release, and updating the
installed executable; see `docs/releases/editor-updates-2026.11.15.1.md`.
The default-feature debug executable from feature validation is
`I:/DEV/helix/target/debug/hx.exe`.
