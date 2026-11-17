# AI commit file limit validation — 2026-11-16

## Correction

The UI accepted configured limits up to 1024 while the handler silently applied
`min(64)`. Both now call one file-count validator: positive configuration,
effective ceiling 1024, unchanged default 64, and an error identifying the actual
count and effective limit. Generation validates staged-file IDs; recommendation
validates all changed-file IDs. The 64-request cap and byte/context budgets are
unchanged. No summaries or diffs are truncated, and no Git write is automated.

## Evidence

Before the correction, both new handler tests failed with the screenshot's exact
hard-coded 64-file error despite `max-files = 128`. The final tests exercise
boundary counts, zero, the effective 1024 ceiling, and complete HTTP-backed
recommendation and generation for all 65 files.

The new native regression initially used 65 disk files. Its initial captured
run stalled and only that run's test process was stopped; an uncaptured probe
then failed at the fixture's 8-second Git-reading deadline. This was not counted
as a meaningful bug reproduction. The fixture was reduced to two changed files,
one staged file and `max-files = 1`, preserving the exact scope-boundary case.
On the old code it failed because generation remained blocked by the unrelated
unstaged file. After the correction it passed, verifying both HTTP payloads
contain only the staged file, with byte-identical index and unchanged HEAD.

The focused handler run passed **23 tests**, with **1 ignored** real-model
evaluation. The native suite initially passed **12**, failed **2** at existing
8-second UI-wait deadlines, and ignored **1** real-model evaluation. The failed
tests were:

- `ai_commit_recommendation_requires_explicit_stage_before_generating_message`
  (still displaying generation in progress).
- `ai_commit_unstage_selected_staged_row_preserves_disk_and_other_index_entries`
  (still displaying unstaging in progress, with AI disabled).

Both existing tests passed individually without changing assertions, timeouts,
or production code. Thus all **14 non-ignored native tests** have passing evidence
across the suite and isolated retries; there was no single fully green native
suite run. These timing observations are retained rather than represented as an
unqualified green suite. No real-model run or full-workspace run was performed.

One independent read-only review found no material issues. `cargo fmt --all --
--check` and `git diff --check` passed. The integration helper's existing unused
`line_feed_handling` warning remains. Changes are uncommitted. At the end of the
source-validation task, installed binaries, user configuration, and running editor
sessions were not changed; the subsequent requested installation is recorded below.

## Requested local installation

On the user's subsequent request, built and installed the patched release using
`cargo build --release --locked --jobs 2 -p helix-term`. The optimized build
finished successfully in 3m 49s. Existing grammar assets were reused with
`HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1`; no grammar sources changed.

Installed `C:/dev/helix/target/release/hx.exe`, version
`helix 25.07.1 (bc354cca)`, SHA-256
`c79b2fd24373cca480b50c16f810595ac2d39ab5fe6f2ba7feb89aa92586f9ee`.
The build includes the uncommitted correction, recorded by source-file hashes in
`target/ai-commit-file-limit-installed.json` and `target/release/BUILDINFO.json`.
Checked matching installed/dependency image hashes, presence of the new validator
diagnostic, and absence of the old hard-coded 64-file diagnostic in the PE image.
The version hash alone is not used as evidence of the uncommitted correction.

Changed only `[editor.ai-commit].max-files`, from 200 to **128**, in
`C:/Users/wangs/AppData/Roaming/helix/config.toml`. TOML values for all other
settings were compared against the backup and preserved. The installed binary's
`--config <path> --health clipboard` successfully loaded the configuration.
`hx --version` resolved to the installed image, and `hx --health python` succeeded
with available tree-sitter parser, highlight, textobject and indent queries.
The health report still lists unavailable alternative Python language servers;
the installed `ty` and `ruff` executables are present.

Backups:

- `target/release/hx-before-ai-commit-file-limit-20261116-111803.exe`
- `target/release/deps/hx-before-ai-commit-file-limit-20261116-111803.exe`
- `C:/Users/wangs/AppData/Roaming/helix/config-before-ai-commit-file-limit-20261116-112205.toml`

Installation did not terminate any existing editor process. Running old sessions
retain their mapped images; a newly launched Helix uses the installed correction
and configuration. One of the original processes had exited by the final check.
`max-input-bytes` remains 4096 as requested settings outside `max-files` were not
changed; the independent model byte/context budgets remain applicable.

## Commands and logs

All test commands used `HELIX_RUNTIME=C:/dev/helix/runtime`.

```text
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --features integration file_limit -- --test-threads=1
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --features integration --test integration ai_commit_file_limit_counts_only_staged_files_for_generation -- --test-threads=1 --nocapture
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --features integration ai_commit -- --test-threads=1 --nocapture
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --features integration --test integration ai_commit_recommendation_requires_explicit_stage_before_generating_message -- --test-threads=1 --nocapture
target/integration/deps/integration-db750459ee1ab1cd.exe ai_commit_unstage_selected_staged_row_preserves_disk_and_other_index_entries --test-threads=1 --nocapture
cargo fmt --all -- --check
git diff --check
```

Ignored logs: `target/ai-commit-file-limit-red.log`,
`target/ai-commit-file-limit-native-red-small.log`,
`target/ai-commit-file-limit-green.log`,
`target/ai-commit-file-limit-existing-retry.log`, and
`target/ai-commit-file-limit-unstage-retry.log`.
