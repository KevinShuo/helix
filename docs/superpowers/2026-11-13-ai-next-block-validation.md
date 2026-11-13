# AI next-line/block prediction and syntax quality guard

The user requested continued AI completion quality improvement and next-line/block
prediction, retaining Shift+Right. The optimized executable and opt-in prediction
settings are installed. Save work and restart Helix to load this round.

## User behavior

- Partial-line completion retains conservative gap prompting and authoritative
  member-line prompting. The existing 48-token ordinary limit stays unchanged.
- `predict-next = true` automatically predicts after a block opener or on a useful
  indented blank line, including after a Python docstring or intent comment.
- One line or one short coherent block appears as gray virtual text. Shift+Right
  accepts the whole preview as one undo step. Continued typing, movement, buffer
  change and mode/config changes cancel stale results through the existing
  controller/snapshot checks. No preview changes source text by itself.
- Prediction output has separate defaults of 6 lines / 192 tokens, hard caps of
  12 lines / 256 tokens and 1200 characters. Overlong/truncated output is rejected
  whole. Ordinary current-line limits are independent.
- Predicted indentation preserves document spaces/Tabs. A backend that trims the
  first line is normalized without shifting later absolute columns; wholly
  relative blocks can restore their common base. Existing suffix code and CRLF
  line endings are preserved. Nested closing braces are distinguished from the
  existing outer closing brace. Copied following code is rejected even with
  trailing whitespace changes.

## Quality improvement and limits

When the document has syntax support and is at most 128 KiB, a cloned snapshot is
parsed on a blocking worker, never on the editor event-loop thread. Conservative
inline completion cannot introduce a parse error into previously valid syntax.
Predictions must parse without ERROR/missing nodes. Changed embedded-language
layers are included, e.g. JavaScript inside HTML and newly inserted script tags.
Traversal is bounded to 20,000 nodes and 16 affected layers; unknown/oversized
ordinary checks fall back to existing output rules. Unsupported/oversized files
use output heuristics. Existing incomplete code is still eligible for ordinary
inline completion.

This is syntax validation, not a type checker or semantic proof. Six failures in
the previous targeted semantic set remain unresolved. No model upgrade or extra
model-verification request was added in this round.

## Evidence

Fresh checks:

- Normal `cargo test --locked`: 79 library tests and 2 doctests pass; 3 live
  library tests and 1 doctest intentionally ignored in the normal run.
- AI configuration: 7 pass, including independent prediction limits and opt-in
  defaults. AI editor integration: 30 pass, 6 live tests ignored normally.
- Explicit actual-model next-line and next-block integrations: 2 pass. Actual
  previews are `return sum(items)` and a 4-line for/if/append/return block.
  Each preview is accepted using Shift+Right, executed with positive/empty/
  negative-input assertions, and undone back to the original buffer.
- Existing 30 Python/C++ executable semantic cases were rerun with prediction
  enabled and the new syntax validator in the actual Rust report path:
  24 pass, 6 wrong, 0 withheld, 0 request errors. No regression in this set.
- Existing five actual-model parameter/member/import scenarios all pass.
- Captured HTTP/editor tests cover multi-line previews, omitted indentation,
  absolute/relative indentation, CRLF, preserving suffix code, nested braces,
  whole-response length rejection and cancellation after further typing.
- An editor-level invalid Python continuation that previously appeared as a
  preview is now rejected. Python/C++/HTML injection syntax regressions pass.
- `cargo fmt --all -- --check` and `git diff --check` pass. Optimized release
  build and Python/C++ parser/highlight/textobject/indent runtime checks pass.

New failures were observed before their fixes: unknown prediction config,
suppressed blank-line prediction, new syntax errors, docstring request gating,
nested-brace suffix rejection, embedded JS errors, copied code with trailing
whitespace, and first-line indentation shifting later absolute columns.
Independent read-only review found no remaining important issues after fixes.

The initial combined live test incorrectly created two editor Applications in
one Tokio runtime. The runtime-local OnceLock job queue belongs to the first
Application; this caused a false missing-preview failure in the second one.
Tests now use independent runtimes, one editor each. Production job-queue code
was not changed to accommodate the test.

Logs/reports under `target/`:
`ai-next-standard.log`, `ai-next-unit.log`, `ai-next-config-green.log`,
`ai-next-integration.log`, `ai-next-live.log`, `ai-next-grounded.log`,
`ai-next-original30-validated*.json`, `ai-next-original30-validated-score.log`,
and `ai-next-*-red.log`.

## Installation and rollback

Installed executable: `C:/dev/helix/target/release/hx.exe`.
SHA-256: `aa45f5d1f909ecce40f6e2e52d8c2b96218a4076275a4ebc2744ff55890aea33`.
Backup: `target/release/hx-before-next-block-ai-quality-20261113-173704.exe`.
Config backup: `target/ai-next-config-backup/config-20261113-174231.toml`.

Only `predict-next = true`, `prediction-lines = 6` and `prediction-tokens = 192`
were added to the user's existing AI section. Parsed comparison preserves model,
endpoint, conservative mode, ordinary token/context limits and all other user
preferences. Shift+Right still maps to `ai_completion_accept`; no new key required.
The existing Qwen3.5-9B model/service is retained. Running user editor/model
processes were not stopped. Owned diagnostic proxies have exited. No commit,
push or Junie global settings change was made.
