# Editor regression validation

This release follows the five failures recorded in the AI commit release.

## Root causes and changes

1. `test_open_below_at_block_end_stays_in_block`: the C grammar includes the CR
   of CRLF in a line-comment node. Looking up indentation at that byte selected
   the comment instead of the surrounding statement. The syntax lookup now
   passes the CR while preserving actual newline insertion coordinates. The
   native regression explicitly exercises LF and CRLF. It failed before the
   change and passed afterward.
2. `test_write_quit_auto_format_exits_after_format`: the test required Bash.
   On this machine the Bash executable is a WSL entrypoint without an installed
   distribution, so formatting failed before producing output. Save/quit and
   formatter-failure tests now compile a small standalone Rust formatter, using
   the existing integration-fixture convention. Both check the exact saved bytes
   and an input marker proving the external formatter actually ran. The failure
   case also rejects stdout from a formatter that exits unsuccessfully.
3. `test_symlink_write`, `test_symlink_write_fail`, and
   `test_symlink_write_relative`: all failed during fixture creation with Windows
   error 1314. The current account has no `SeCreateSymbolicLinkPrivilege`.
   These tests are now explicitly ignored by default on Windows, with a reason;
   they are not reported as passed. Their real file-symlink setup and assertions
   remain unchanged. The same policy covers three related path/VCS cases that
   previously needed command-line filtering. Windows CI separately invokes the
   ignored symlink tests and requires them to succeed. No machine privileges or
   Developer Mode settings were changed.

Full verification also exposed a timing assumption in
`commit_git_timeout_stops_hook_descendants_before_releasing_guard`.
Under controlled heavy load, Windows tree termination returned after 2.40–2.50
seconds, and the old one-second hook had already written before termination
returned and before the operation guard was released. The hook now waits for a
test-owned sentinel created only after termination and guard release. The
original absence-of-disk/index-mutation assertions remain intact. Subsequent
writes must be allowed after confirmed termination, or rejected by the existing
quarantine after unconfirmed termination. This checks surviving descendants without assuming that
the operating system terminates processes instantaneously; Git production
termination code is unchanged.

## Verification environment

- Windows x64, existing installed grammar runtime at
  `I:\DEV\helix\target\release\runtime` (302 cached grammar DLLs).
- Compile with one Cargo job to avoid exhausting compiler memory.
- Preserve the existing AI provider, Chinese reStructuredText documentation,
  AI commit settings, and untracked `.idea/` directory.

The privileged Windows symlink cases cannot run under this local account.
An ignored local result is not evidence of privileged execution. See
`docs/CONTRIBUTING.md` for exact commands to run them on a capable account.

## Diagnostic runs

An initial editor run concurrent with workspace verification aborted with an
allocator request for 72,057,594,037,927,944 bytes. The cause is undetermined;
no memory fix is claimed. An isolated 68-case auto-pairs rerun passed, and the
complete editor suite rerun separately passed: 311 passed, 0 failed, 11 ignored,
0 filtered (246.33 seconds). The aborted run is not counted as a successful gate.

Deliberately running three VCS suites simultaneously at 24 test threads also
exposed `commit_rejects_real_unmerged_index_without_operation_marker` failing
during fixture `git commit` setup. Those stress runs were diagnostic, and their
results do not substitute for the normal workspace verification below.

Both the patch and the additional synchronized hook-test change received an
independent read-only review with no actionable findings. Formatting and diff
checks passed. Final workspace counts, release artifact checks, and installation
evidence are recorded in the release README and BUILDINFO.json after verification
completes.

## Final workspace gate

`HELIX_RUNTIME=I:\DEV\helix\target\release\runtime cargo test --workspace -j 1`
completed with 739 passed, 0 failed, 14 ignored, 0 filtered. No `--skip` filters
were used. The synchronized Git timeout test passed as part of this run.
