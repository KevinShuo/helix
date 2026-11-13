# Distant function contracts in AI completion

## Change and scope

The previous context collector recovered distant declarations/import aliases but
dropped the behavior documented at the function entry. A long function could
therefore provide parameter names without the constraint needed to choose an
expression. This round extends source notes rather than changing models/prompts.

Conservative requests now recover Python function docstrings and contiguous,
standalone declaration comments when they lie outside the nearby prefix. The
existing 600-character source-note budget and total prefix budget remain. The
first declaration is prioritized; documentation is included atomically or omitted.
Python formatted/byte strings, assignments and later string expressions are not
docstrings. Oversized/future text and nearby docs are omitted. A blank line or
code on the comment's line ends attachment. At most eight comment nodes and 32
initial suite children are considered; oversized whitespace gaps are not scanned.

The user had already authorized direct conservative implementation. No model or
user configuration change, dependency installation, commit or push was needed.

## Verification

The new docstring/comment tests failed before implementation. A separate trailing
comment regression was observed failing and then fixed. Final normal tests pass:
74 library tests, 2 doctests; 3 live tests and 1 doctest remain intentionally
ignored in the normal run. AI editor integration has 25 passing tests and 4 live
tests ignored. The editor's captured HTTP request now also verifies that a distant
docstring reaches the prompt alongside parameters/imports.

Actual requests used the already running Qwen3.5-9B Q6_K service on localhost
19240, conservative gap mode, the unchanged source/prefix budget, 48 output tokens,
and no extra reasoning/verification model call. Reference answers/checks were not
sent to the model.

| Set | Before | After |
| --- | --- | --- |
| Existing 30 executable cases | 24 pass, 6 wrong | 24 pass, 6 wrong |
| Eight new distant-contract cases, source includes only | 0 pass | 5 pass, 2 wrong, 1 compile error |
| Same eight, scorer's standard headers supplied | 0 pass | 6 pass, 2 wrong |

All eight new reference insertions pass. The generated `std::min(a,b)` expression
passes semantic checks only with the scorer's supplemental `<algorithm>` include;
it fails with the fixture's own includes. It is therefore **not** counted as a
self-contained passing completion. Other remaining errors return constant zero
for a constrained maximum and call a string method on a Python list. The original
six semantic failures remain unresolved. Neither set estimates general accuracy.

The five existing actual-model source/member regressions also pass. A fresh
read-only reviewer found no remaining important product issues after bounded-scan
checks were added, and independently confirmed the missing-header limitation.

Evidence:

- `target/ai-contract-red.log`, `ai-contract-trailing-red.log`,
  `ai-contract-green.log`, `ai-contract-standard.log`, `ai-contract-integration.log`.
- `target/ai-contract-reference.log`, `ai-contract-baseline.json`,
  `ai-contract-selected.json`, their score reports, `ai-contract-strict-score.json`.
- `target/ai-contract-original30*.json` and `ai-contract-grounded.log`.
- Reusable fixtures: `contrib/ai-completion-contract-fixtures.json`; existing scorer
  recipe: `contrib/ai-completion-eval.md`.

## Delivery

The optimized executable is rebuilt with a timestamped binary backup, preserving
running editor processes and the existing model server. Install metadata, binary
hash and backup path are in `target/ai-contract-installed.json`; runtime health
checks are in `target/ai-contract-health-python.log` and
`target/ai-contract-health-cpp.log`. Save work and restart Helix to load this round.
The user configuration remains unchanged from the previously installed 9B profile.

Release build completed successfully. Installed SHA-256:
`b81e9834ef6711299893300a512406ed71a727cde71dc13c70e91dcc49f8acaf`.
Binary backup: `target/release/hx-before-contract-ai-quality-20261113-164147.exe`.
Python/C++ parser, highlighting, textobject and indentation checks pass.
`cargo fmt --all -- --check` and `git diff --check` pass. User editor PID 41364
and model server PID 60536 remained running; no owned model probe was needed.
