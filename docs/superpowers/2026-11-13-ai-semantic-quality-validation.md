# AI completion semantic quality validation

The selected local profile is Qwen3.5-9B Q6_K, conservative mode, explicit `gap`
prompting for expression holes and strict line prompting when authoritative LSP
member names are available. Source declarations/import aliases and LSP metadata
remain within the existing context budget. No extra model verification request or
native reasoning pass is enabled.

## Executable comparison

The 30 fixed Python/C++ fixtures were split into 15 development and 15 holdout
cases before selection. Only source/context is sent to the real Rust client;
reference insertions and executable checks remain outside the model request.
All 30 reference programs pass. Mutation checks reject both overflowing positive
modulo and an incorrect closed-interval equality comparison (28 pass, 2 wrong).

| Profile | Development correct/wrong/withheld | Holdout correct/wrong/withheld | Total |
| --- | --- | --- | --- |
| Previous 7B + conservative line | 6 / 7 / 2 | 9 / 4 / 2 | 15 / 11 / 4 |
| Selected 9B + gap/member-line | 11 / 4 / 0 | 13 / 2 / 0 | 24 / 6 / 0 |

Both runs have zero backend errors. Final selected results reproduced the earlier
24/30 result. Warm-client request median/p95/max is 129/252/576ms; the original
baseline is 120/184/379ms, excluding suppressed zero-duration cases. Timings omit
model startup, editor debounce and compilation/scoring.

This is a targeted regression set, not a general accuracy estimate. The remaining
six failures concern descending lower/upper bounds, preserving the last duplicate,
lazy factory evaluation, overflow-safe averaging and large-modulus normalization.
Conservative filters do not establish semantic correctness. No fixture answer or
check was hardcoded into the production prompt.

Raw evidence is under `target/ai-semantic-baseline*.json`,
`target/ai-semantic-final*.json` and `target/ai-semantic-final-comparison.json`.
The reusable recipe is `contrib/ai-completion-eval.md`.

## Behavior and regression checks

- Gap prompts preserve prefix/suffix, prioritize surrounding behavior/edge cases,
  and use two fixed formatting examples. Typed-prefix and partial-word echoes,
  extra lines, unbalanced fragments and extra statements are rejected. Copied
  closing suffixes are removed only when the remaining insertion stays balanced.
- Strict line mode restores omitted leading indentation only when all typed
  nonblank code matches. It still rejects changed code or interior whitespace.
- Python request gating recognizes block-header colons without suppressing lambda,
  slice/dictionary colons or variables containing underscores/soft keywords.
- Actual 9B gap-only member prompting invented `fetch(store, id)`. Authoritative
  member contexts therefore use strict line mode and retain name validation.
  Real-model parameter, member-field, member-method and import-alias cases all
  pass (five scenarios). This avoids shipping the observed member regression.
- Regressions were observed failing before corresponding fixes. Independent
  read-only reviews checked fixture quality, gap cleanup, colon gating and the
  final dispatch; no material findings remain in those reviewed scopes.

Final checks:

- `cargo test --locked -p helix-term`: 69 library tests and 2 doctests pass;
  3 live tests and 1 doctest intentionally ignored in the normal run.
- AI editor integration: 25 pass, 4 live tests ignored in the normal run.
- AI configuration tests: 6 pass. Scorer CLI tests: 3 pass.
- Explicit managed-model integration with the downloaded 9B: passes; actual
  `sum(items)` preview/acceptance, disable, startup cancellation, re-enable and
  shutdown release are verified.
- Explicit real-clangd integration: passes; actual member metadata reaches the
  outgoing AI prompt and `category_code` is accepted. Model reply is controlled
  in this LSP integration; separate real-model tests cover generation.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- Optimized release build passes. Installed executable starts; Python/C++ parser,
  highlighting, textobject and indentation runtime health checks pass.

Logs: `target/ai-gap-standard.log`, `ai-gap-integration.log`,
`ai-gap-config-tests.log`, `ai-gap-grounded-live.log`, `ai-gap-managed-live.log`,
`ai-gap-clangd-live.log`, `ai-gap-release-build.log`, `ai-gap-health-*.log`.

## Installed profile and rollback

Weights: [Unsloth Qwen3.5-9B GGUF](https://huggingface.co/unsloth/Qwen3.5-9B-GGUF),
checked against the [official Qwen model card](https://huggingface.co/Qwen/Qwen3.5-9B).
Pinned quantization revision: `3885219b6810b007914f3a7950a8d1b469d598a5`.
`Qwen3.5-9B-Q6_K.gguf` is 7,458,301,152 bytes; verified SHA-256:
`91898433cf5ce0a8f45516a4cc3e9343b6e01d052d01f684309098c66a326c59`.

Installed `C:/dev/helix/target/release/hx.exe` SHA-256:
`381ccb0563b8df8d5f327afca4015a3fd4f81a0cab6192cb5a48103e62c9e283`.
Previous binary: `target/release/hx-before-gap-ai-quality-20261113-161907.exe`.
User configuration backup:
`target/ai-gap-config-backup/config-20261113-162731.toml`.
Parsed config comparison verifies only model, model-path and prompt-mode changed;
conservative=true, 350ms debounce, token/context limits and other preferences remain.
The previous model file remains available. Junie's global settings were not changed.

User editor PID 50644 and its previous-model server PID 68944 were preserved.
Owned probe/model-test processes have exited. Restart Helix after saving work to
load the installed binary/profile; the currently running session still uses its
previous executable/configuration. No commit or push was made.
