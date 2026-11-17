# Provider-aware automatic AI budgets

The user approved automatic official-capability budgets for DeepSeek, a manual
limit switch, and direct implementation/installation. Local settings established
earlier in this conversation remain in effect.

## Decisions

Add provider `budget-mode = "auto" | "configured"`, default auto. Auto only
activates a known capability profile for recognized DeepSeek model IDs, never
guesses unknown-model capabilities. Known current IDs are `deepseek-flash`,
`deepseek-v4-pro` and the two documented retired Flash aliases now served by
the current Flash backend. Other providers retain configured budgets.

Known profile: context 1048576, maximum generated tokens 393216. These are ceilings,
not requested response lengths. Keep the current provider selection and thinking
mode. Configuration mode honors feature limits, bounded by the known service
capability where applicable. Local context/output/time budgets stay unchanged.

Avoid imposing the local byte-per-token conservative estimate as an exact
DeepSeek tokenizer. Automatic DeepSeek admits complete requests within a finite
16 MiB memory envelope and delegates exact token enforcement to the API. Request
the official output ceiling. If the API rejects a context/output reservation,
retry once with a small output allowance without changing input. If that produces
a length-limited response with authoritative prompt-token usage, retry with the
remaining context less overhead, bounded by the official output maximum. At most
three attempts; no retries for authentication, rate limits, arbitrary bad JSON,
unknown errors or invalid generated code. Never apply partial output.

Automatic DeepSeek transport timeout: finite two hours, cancellable. Configured
timeouts retain requested values; local maximum remains ten minutes. Response
and automatic DeepSeek generated-artifact memory ceilings are 16 MiB. This is a
memory bound, not a statement that tokens equal bytes. Preserve structural checks,
plain-text safety, meaningful-test checks, stale-state protection and explicit
acceptance/Git writes. Larger merge previews must remain fully scrollable without
u16 row overflow. Short completion remains a short completion by task contract.

Commit auto budgets use the existing Git snapshot ceiling of 4096 files, up to
128 complete files per summary batch and at most 256 analysis/aggregation calls.
Transport has at most three attempts per call. Configured/local remains at the
user's 128 files, configured input bytes, four files per batch and 64 calls.
Known auto mode overrides legacy feature size/output budgets; configured mode
provides an explicit opt-out. Retain all exclusions and incomplete-coverage review.

## Sources checked 2026-11-16

- https://api-docs.deepseek.com/quick_start/pricing/ — 1M context and 384K output;
  current public model IDs and Flash aliases.
- https://api-docs.deepseek.com/api/create-chat-completion/ — max_tokens 1..393216,
  input plus generated tokens share context; usage and finish_reason semantics.
- https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash/blob/main/config.json —
  official model max_position_embeddings 1048576.
- https://api-docs.deepseek.com/quick_start/token_usage/ — token counts vary;
  API usage is authoritative, character ratios are estimates only.

## Alternatives

Merely increasing feature config retains conflicting transport/preview caps.
Bundling a tokenizer would add model-specific artifacts and may drift from the
hosted model. A known-capability registry plus bounded API-guided allocation
keeps the policy centralized and uses server token counts for retry allocation.
Do not automatically switch providers or send real repository data for tests.
