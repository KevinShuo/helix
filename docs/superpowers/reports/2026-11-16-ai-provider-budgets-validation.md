# Provider-aware budget validation — 2026-11-16

The user approved automatic official-capability budgets for DeepSeek, a
configured/manual switch, implementation and local installation. Existing local
settings and current provider selection are preserved.

## Policy and behavior

Provider `budget-mode` defaults to `auto`; `configured` opts out. Auto recognizes
only the explicit DeepSeek type and the four verified public model IDs/aliases.
Unknown models use configured limits. The current known profile is 1048576
context tokens and a 393216-token generated-output ceiling, based on official
API/model documentation linked in the design. The requested maximum is an
allowance; task instructions still determine response length.

Auto DeepSeek no longer applies local feature byte/file/output limits. Exact
token accounting is delegated to the service rather than treating UTF-8 bytes
as token counts. Complete serialized requests/responses and raw code remain
within a finite 16 MiB envelope; documentation formatting can add indentation
and escaping, so this is not a total-process RSS bound. Commit snapshots cap at
4096 files, summary batches at 128, and analysis at 256 logical calls. Each call
has at most three transport attempts within one finite two-hour deadline.

A recognized context reservation error can retry with a small output reserve
without altering messages. A length-limited response with numeric prompt-token
usage can then allocate the actual remaining context, capped at the official
output limit. Unknown errors, authentication, rate limits and invalid JSON do
not trigger speculative retries. Typed input-limit errors allow Auto commit
analysis to split only resource-limited batches. Existing configured/local
batch-reduction behavior remains unchanged.

Automatic inline completion clones a configured-budget transport view so its
short fragment/prediction output and deadline remain task-sized. This clone does
not mutate provider selection or thinking settings. Larger generated doc/test/
merge/commit output is validated under the provider envelope while structural
checks, incomplete coverage and explicit review/acceptance remain in place.

Documentation/merge preview rendering caches compact row offsets, renders only
the visible viewport and uses `usize` scrolling. Home/End and CtrlHome/CtrlEnd
reach the full preview, including beyond 65535 rows and at narrow Unicode widths.
Commit panes slice visible logical lines before invoking Paragraph so large
message offsets no longer clamp at u16.

## Regression evidence

- Policy parser RED rejected the new `budget-mode` field; final provider suite
  passed **16 tests, 0 failures**.
- Transport RED passed 10 and failed 4 expected cases (large response, wider
  deadline, reservation recovery and bounded retry chain); focused GREEN passed
  **14/14** before the final typed-resource-error regression was added.
- Client integration RED passed 1 and failed 7, covering complete large inputs,
  provider-sized scope, generated artifacts and response acceptance.
- Preview API RED and focused helper RED caught missing large-preview APIs and
  CRLF row handling. Format **9/9**, merge preview **2/2**, and shared rendering
  **3/3** passed. The final rendering regression uses the actual End/Home key
  handler and reaches the tail after 70000 visual rows.
- An initial integrated run passed **162**, with **12 ignored**. Independent
  transport review found two material integration defects: short completion was
  receiving the cloud maximum, and generic Auto HTTP400 still caused legacy
  commit batch splitting. Both were fixed. Their behavior regressions failed
  first: 393216 versus 48 requested completion tokens, and two versus one HTTP
  requests for an unrelated parameter error. The first batch-error test also
  required correcting an assertion to inspect the full anyhow error chain;
  the subsequent RED verified the intended two-versus-one request failure.
- Final integrated run passed **166 tests, 0 failures, 12 ignored**. This includes
  the new typed-resource classification tests, short-completion contract, manual
  opt-out, full large payload/candidate/test/message preservation, and retention
  of incomplete-coverage review for unanalyzed commit files.
- A final independent policy/integration review found no material issues after
  those two corrections. Formatting and whitespace checks passed.

All HTTP verification used loopback fixture servers. No paid DeepSeek request or
real repository source upload was made, and no actual 1M-token/384K-token cloud
generation was performed. Capability enforcement and retry integration were
tested with deterministic service responses and Tokio's virtual clock.

## Concurrent progress work

The user authorized coordination with the separate progress-display chat.
Its stable callbacks and UI heartbeat were preserved and are included in the
unified release. Its native progress regression passed. The broader native
suite passed **14**, failed **1**, and ignored **1**. The existing
`ai_commit_recommendation_requires_explicit_stage_before_generating_message`
exceeded its eight-second end-to-end UI wait, and an unmodified isolated retry
also timed out. The snapshot showed final Git-state validation still running;
this is retained as a failed timing check, not represented as a passing suite.
See `2026-11-16-ai-commit-progress-validation.md` for that chat's evidence.

## Commands and artifacts

```text
HELIX_RUNTIME=C:/dev/helix/runtime
HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1
cargo test -p helix-view --test ai_providers --profile integration -j 2
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib deepseek_ -- --test-threads=1 --nocapture
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib provider_contract_ -- --test-threads=1 --nocapture
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib provider_contract_deepseek_does_not_split -- --test-threads=1 --nocapture
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib ai -- --test-threads=1 --nocapture
cargo fmt --all -- --check
git diff --check
```

Final log: `target/ai-provider-budgets-final-green.log`. Earlier logs and isolated
preview harnesses remain in ignored target files. Source changes are uncommitted;
no user-repository staging, commits, pushes or provider switch was performed.

## Installed release

- Built `cargo build --release --locked --jobs 2 -p helix-term` successfully in
  4m20s. Four unused-wrapper warnings remain; there were no build errors.
- Installed binary: `C:/dev/helix/target/release/hx.exe`, also resolved by PATH.
  Version: `helix 25.07.1 (bc354cca)` plus the uncommitted changes recorded in
  `target/ai-provider-budgets-installed.json` and `target/release/BUILDINFO.json`.
- SHA-256: `576704683b03d71d098148a5350703aa803eaed3201d605a2cfbcd17e40a33e1`.
  All 22 recorded Rust/manifest source fingerprints matched after the build.
- Previous executable retained at
  `target/release/hx-before-ai-provider-budgets-20261116-121950.exe`.
- Updated `C:/Users/wangs/AppData/Roaming/helix/config.toml` to add
  `budget-mode = "auto"` to its existing DeepSeek provider. A parsed TOML
  comparison verified every other value was preserved, including the default
  local provider and commit `max-files = 128`.
- Configuration backup:
  `C:/Users/wangs/AppData/Roaming/helix/config-before-ai-provider-budgets-20261116-122517.toml`.
- Installed executable accepted that config in both clipboard and Python health
  checks. Python's available ty/ruff servers, parser and highlight queries were
  detected. Optional absent Python servers were not installed by this task.
- Formatting and whitespace checks passed after installation. Running editor
  sessions were not terminated; restart Helix to load the installed executable.

The first installation-finalizer check assumed complete model-name literals
would survive optimized compilation. They did not; that check failed before
any config mutation. Verification instead used source fingerprints, executable
SHA-256, the budget-mode key and successful installed configuration loading.
