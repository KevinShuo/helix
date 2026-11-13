# Conservative AI completion validation

2026-11-13, Windows, existing `C:/dev/helix` checkout. The user requested direct
implementation of the conservative approach. No model weights, dependencies or
project-file upload scope changed.

## Behavior

The new opt-in `editor.ai-completion.conservative` setting uses current-line
completion, deterministic sampling, a 64-token ceiling and shorter suggestions.
It avoids blank/comment-only lines, punctuation-only input, single bare letters,
statement/block terminators, and expression-like tokens immediately before an
existing semicolon. The last heuristic intentionally sacrifices some useful
partial-identifier suggestions.

Prompts request abstention on ambiguity. Responses with truncation/other abnormal
finish reasons, multiple nonblank lines, more than 160 inserted characters,
extra top-level statements or text beyond the known suffix are rejected. Existing
closing suffixes/comments, quoted semicolons, for-loop headers, nested blocks,
Rust lifetimes/characters and C++ numeric separators have regression coverage.
Disabled conservative mode retains existing insertion/multiline behavior.

## Fresh verification

| Check | Result |
| --- | --- |
| Configuration regression | Failed with unknown `conservative` field before implementation; 5 configuration tests pass afterward |
| Conservative client regressions | Observed failures before the corresponding fixes; all 7 tests pass in final standard run |
| `cargo test --locked --jobs 2` | 58 library tests and 2 doc tests pass; live comparison and one documentation example ignored |
| AI editor integration group, integration profile | 23 pass, 3 opt-in live tests ignored; existing unused test-helper field warning |
| Actual local-model comparison | 1 opt-in comparison test passes, eight paired fixtures below |
| `cargo fmt --all -- --check` | Pass |
| `git diff --check` | Pass |
| `cargo build --release --locked --jobs 2 -p helix-term` | Pass, 4m 14s |
| Installed Python/C++ parser and query health | Pass |
| Independent read-only review | Reported suffix and scanner edge cases were reproduced by failing tests and fixed; final review found no remaining material findings |

The final standard, integration, config and live logs are in
`target/ai-conservative-{standard,integration,config-tests,live}.log`.
This was the normal project test command plus relevant configuration/editor
groups, not a complete workspace/integration-suite run.

## Live comparison

The already-running server advertised `Qwen2.5-Coder-7B-Instruct-Q6_K`. The
comparison used the actual Rust client, 48 output tokens, baseline line prompting
versus conservative mode, without restarting the server. Results from one final
paired run:

| Fixture | Baseline insertion | Conservative insertion |
| --- | --- | --- |
| Blank function-body line | `for item in items:` | None |
| Bare `x` | ` = 0;` | None |
| Completed `int count = 0;` | ` // Initialize count to zero` | None |
| Comment | ` data` | None |
| Explicit sum-of-items function | `sum(items)` | `sum(items)` |
| Insertion-sort shift | `a[j]` | `a[j]` |
| Explicit string uppercase task | `upper()` | `upper()` |
| Already complete `return value;` | ` > 0` | None |

Stronger abstention prompting alone did not fix the last case; it failed the live
regression before the semicolon trigger gate. These eight synthetic examples
demonstrate suppressed speculation and retained useful completions in this
sample, not a general accuracy estimate. Syntax-like heuristics and model
instructions cannot establish semantic correctness.

## Rollout

The optimized binary is installed at `target/release/hx.exe`. The build script preserved the
running executable as `target/release/hx-before-conservative-ai-20261113-133615.exe`. The build
log and resulting executable/hash/backup record are kept in
`target/ai-conservative-release-build.log` and `target/ai-conservative-installed.json`.
The configuration backup is `target/ai-conservative-backup/config-20261113-134043.toml`.
Installed settings enable `conservative = true` and change `debounce-ms` from 200
to 350; the installer verified all other parsed preferences were preserved.
The original editor PID 50644 and model-server PID 68944 remained running after
installation. Restart Helix to load the replacement executable. No commit, push
or public release is part of this request.
