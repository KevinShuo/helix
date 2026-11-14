# AI function documentation validation

Feature: native `:ai-doc` and `:ai-doc-cancel`, C/C++ and Python, Chinese reST.
Design: `specs/2026-11-14-ai-function-doc-design.md`.
Branch: `codex/ai-function-doc`, baseline `ab10f29b`.

## Recorded development evidence

- Configuration test first failed on unsupported `[ai-doc]`, then passed.
- Target/formatter/context tests first failed with missing behavior, then passed.
- Additional regressions reproduced void-pointer return detection, a template
  header cursor, reST directive injection, parenthesized Python docstrings,
  exception-name comment terminators, and comments inside parameter lists.
- Existing AI completion baseline: 42 passed, 3 ignored.
- First complete AI unit subset: 53 passed, 3 ignored.
- Expanded editor integration checkpoint: 9 passed, covering native commands,
  C/C++ and Python insertion, nonediting preview, undo/redo, keyboard isolation,
  retry, truncation, cancellation, focus, readonly and prompt deferral.
- Term unit checkpoint: 94 passed, 4 ignored.

## Real model probe

The user's already-running service was inspected at `127.0.0.1:19240`.
It reported `Qwen3.5-9B-Q6_K`, with a 4096-token context. No user configuration
was changed and no replacement model was downloaded. All six probes used the
production request/formatter path, non-streaming JSON output and per-request
thinking disabled. Automatic completion was not required.

| Language | Function | Time | Reviewed behavior |
| --- | --- | --- | --- |
| C | `larger` | 1.018 s | Two parameters and maximum return value |
| C | `clear` | 0.582 s | Pointer mutation to zero; no fabricated return |
| C++ | `add<T>` | 1.234 s | Template and ordinary parameters in reST |
| C++ | `checked` | 1.336 s | Visible negative check and `std::invalid_argument` |
| Python | `divide` | 1.442 s | Source types, quotient and explicit `ValueError` |
| Python | async `identity` | 0.751 s | Awaited string result and source types |

Each returned a structurally valid Chinese annotation. The outputs were read
against these small sources; no invented external contracts were observed.
This is a six-example smoke check, not a measured accuracy rate or latency SLA.
The ignored test `ai_doc_live_model_six_functions` repeats this check when
`HELIX_AI_DOC_LIVE_ENDPOINT` and `HELIX_AI_DOC_LIVE_MODEL` are explicitly set.

A local tokenizer sanity probe counted 18 tokens for a mixed English/Chinese
98-byte input. The request uses a deliberately conservative UTF-8-byte upper
bound for this byte-fallback vocabulary, plus output allowance and a 256-token
chat-template reserve. Optional source notes are dropped first. Over-budget
functions are refused intact rather than truncated. Arbitrary external model
vocabularies still require an appropriately configured context window.

## Review and implementation decisions

An independent read-only review identified three important issues; each has a
regression test reproducing the failure before correction: existing parenthesized
docstrings, source-derived exception delimiters, and named comment extras in
Python/C++ template parameter lists.

The current checkout is reused on a feature branch, leaving `.idea/` untouched.
The implementation runs inline with one independent final review. Each test
Application uses its own Tokio runtime because the existing runtime-local job
queue initializes once. Shared model settings remain in `[editor.ai-completion]`;
the effective server-enable flag is the OR of completion and documentation.

## Final verification

Review regressions are now green: 18 documentation unit tests and 10 editor
integration tests passed. This includes the three Important findings and tests
for nested template ordering and revealing the insertion while preserving the
logical cursor. There are no deferred review findings.

`cargo xtask docgen` succeeded; only the two new typable command rows are retained.
Unrelated pre-existing generated static-command/language-table drift is excluded.
The final supported-environment workspace run completed: 696 passed, zero
failed, 11 normally ignored, and the three privilege-dependent cases excluded.
The unrestricted full editor integration run completed: 297 passed, 5 failed,
7 ignored (309 total). All ten new documentation cases passed. Its five failures
match the pre-existing `target/new-file-final-integration.log` baseline exactly:

- `test_open_below_at_block_end_stays_in_block` (existing C indentation expectation).
- `test_symlink_write`, `test_symlink_write_fail`, `test_symlink_write_relative`
  (OS error 1314).
- `test_write_quit_auto_format_exits_after_format` (existing formatter fixture).

None of those test implementations or production code paths were modified by
this feature. A separate symlink creation probe also returned OS error 1314.

An additional, nonstandard `--features integration --lib` full-unit attempt ran
93 cases successfully but six existing synchronous `function_hints` tests
panicked because `helix-event`'s integration-test runtime-local implementation
requires a Tokio reactor. Their `#[test]` setup does not provide one. The default
unit suite is therefore run separately from `--features integration --test
integration`, as in the normal project workflow; these six tests pass with the
default features.

The final six-function model rerun passed in 8.92 s overall (individual generation
0.65–1.57 s). The default-feature binary built successfully with `cargo build -j 1 -p helix-term --bin hx`.

A real Windows PTY ran that binary against a scratch Python file and the existing
local model. The actual terminal displayed the Chinese preview. Escape canceled;
a second request followed by Enter inserted it; `u` restored the original buffer;
`U` replayed the same annotation; `:wq` saved and exited. CPython parsed the saved
file, verified the original function statements were unchanged after removing the
new docstring, and executed `multiply(6, 7) == 42`. No user configuration was
changed. The earlier desktop UI launch of cmd.exe was blocked by product policy;
this coding PTY check verified the native editor instead.

The unrestricted workspace run exposed OS error 1314 (Windows symlink privilege)
in the existing `helix-stdx::test_normalize_path`, `helix-vcs::git::test::symlink`
and `helix-vcs::git::test::symlink_to_git_repo`. The final supported-environment run
explicitly skips those three tests; their failure is not attributed to this change. One intermediate LLVM build exhausted memory; the
build was retried with `-j 1` without weakening tests or product behavior.

## User-visible limits

Existing documentation is skipped, C/C++ declarations without bodies and
unnamed parameters are unsupported, and a Python one-line body must first be
expanded manually. Description correctness requires human preview. C/C++ reST
comment extraction depends on the project's documentation toolchain.


## Final source-state checks

After the broad runs, completion auditing added regression-backed preservation
for comments between C++ template heads and definitions, parenthesized Python
docstrings containing comments, document-language changes, and a visible Git
dock that has yielded keyboard ownership. Each added case was observed RED
before its correction. A component keyboard-ownership query now distinguishes
a nonmodal dock from an active prompt/picker; normal event dispatch is unchanged.

On the final code:

- Default-feature `cargo test -j 1 -p helix-term --lib -- --test-threads=1`:
  **99 passed, 0 failed, 4 intentionally ignored**.
- `cargo test -j 1 -p helix-term --features integration --test integration ai_doc
  -- --test-threads=1`: **12 passed, 0 failed**.
- Default-feature native binary build succeeded. Formatting and scoped diff
  checks passed. The real model and native PTY checks above cover the production
  transport, formatting, preview and transaction paths.

The broad workspace/full-integration counts above are checkpoints before these
last targeted additions; unchanged crates were not repeatedly rebuilt/tested.
All current documentation-specific tests were rerun after the additions.
There are no deferred review findings. The existing broad-suite failures are
listed explicitly rather than described as a green full suite.
