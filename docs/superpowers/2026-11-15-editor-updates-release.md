# Editor updates release — 2026-11-15

User authorized building the latest changes as release, pushing Git, uploading a
GitHub Release, and finally replacing local `hx` while preserving a backup.

## Source and scope

Repository: `KevinShuo/helix`; source branch: `codex/ai-test-generation`.
Release tag: `editor-updates-2026.11.15` (a fork feature release; upstream package
version remains 25.07.1).

This release combines named local/online AI providers and system-proxy support,
bounded on-demand reasoning/model switching for explicit generation, C/C++
GoogleTest/CMake and Python pytest generation with preview and execution, its
synthetic quality-evaluation evidence, and cross-file live/build problem navigation.
Individual validation records are linked in the adjacent feature reports.

## Build and package contract

- Commit the selected source, tests, documentation, example configuration and
  benchmark evidence. Exclude personal IDE files and Python caches; include no
  personal Helix configuration, API credentials, models or model server binaries.
- Build the exact committed source with `cargo build --release --locked -j 1 -p
  helix-term --bin hx --target-dir target/publish-build`.
- Package Windows x86_64 `hx.exe`, tracked runtime data, cached Windows grammar
  DLLs, Chinese usage instructions, examples, license, build metadata and SHA-256.
- Extract and compare every ZIP entry with its staged source, then run version
  and C/C++/Python/Rust/Go parser/query health using isolated empty configuration
  and only the packaged runtime (no ambient `HELIX_RUNTIME`).
- Record exact commit, version, hashes, test counts and artifact sizes in
  `BUILDINFO.json` and `SHA256SUMS.txt`, shipped with the GitHub Release.
- Push the current branch and annotated release tag. Create a draft release,
  upload/verify asset sizes and SHA-256 digests, then publish it as latest.
- Finally replace `I:/DEV/helix/target/release/hx.exe` from the verified package,
  retaining the prior executable/symbols and the existing personal configuration.
  Verify installed hash, reported commit and language health. Keep active editor
  sessions running; they pick up the new binary after restart.

## Validation

The feature verification already passed 9 problem-navigation editor integrations,
157 terminal unit tests (12 existing opt-in ignores), 88 view unit tests,
format/diff checks and cargo check. This publication also runs the full editor
integration suite and provider/configuration checks. Full logs, any baseline
limitations, package verification and delivery details are retained under ignored
`target/release-20261115/` and embedded in the release build metadata.

Generic compiler text with ambiguous Unicode/tab column units intentionally
visits the correct line start; Rust/Cargo character positions remain precise.
AI features still require explicit configuration and user review of generated
changes. CMake/environment failures and unsupported project layouts retain the
documented first-version limits rather than claiming automatic repair.
