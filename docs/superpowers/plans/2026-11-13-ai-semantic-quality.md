# Semantic AI completion quality

The active user goal is further AI completion quality improvement. Continue
directly in the existing checkout and preserve all prior changes and sessions.

## Evidence and decision gates

Use 30 fixed Python/C++ completion fixtures with executable checks, split evenly
into development/holdout sets before changing generation. All reference programs
must pass first. Count correct, wrong and withheld suggestions separately.
After independent fixture audit, the installed 7B client's development result
is 6 correct, 7 wrong, 2 withheld. The audit added large-modulus and positive
overlap checks, corrected the modulo reference and separated backend failures.
An additional 7B verification pass copied the errors, so do not ship it.
Prompted scratchpad reasoning introduced regressions and is also rejected.

The existing 27B model was isolated on port 19241 without stopping user services.
It needs several seconds per line with partial GPU offload and emits lines with
omitted leading indentation. Do not switch the user to that slow configuration.
Fix the safe formatting mismatch only when the nonblank typed code matches
exactly and the model omitted all leading indentation; retain strict checks for
changed code, interior whitespace, suffixes and blank-prefix responses.

Evaluate Qwen3.5-9B Q6_K from a pinned, SHA-256 verified Unsloth quantization
against the official Qwen model card. Download is an isolated new file; keep the
current 7B service running. Change the normal configuration only if executable
development and held-out checks show a meaningful correctness gain with useful
coverage and interactive latency. Preserve the old model/config as rollback.

## Execution

- [x] Validate 30 reference programs and record the installed baseline.
- [x] Reject ineffective 7B review/reasoning strategies from measured results.
- [x] Observe failing indentation regressions, implement the narrow normalization,
  and retain original checks when conservative mode is disabled.
- [x] Benchmark the isolated 9B candidate using the real Rust client, then score
  unchanged holdout programs and compare latency before choosing a default.
- [x] Keep a reusable benchmark/score tool, run normal and AI/editor tests,
  perform independent review, build/install and validate the actual managed model.
- [x] Stop only owned probe processes and record final evidence and limitations.

No commit, push, public release, or change to Junie's global settings is required.

## Selected implementation

Use Qwen3.5-9B Q6_K without native reasoning. Ordinary expressions use explicit
gap prompts and two format examples; authoritative member metadata uses the
existing strict line prompt and member-name validation. The real-model five-case
grounded-context regression passes with this dispatch. Gap-only member prompting
invented a method call, so it is not used for that context.

Reject repeated typed prefixes, partial-word echoes and unbalanced fragments;
trim copied closing suffixes only when the residual stays balanced. Python
request suppression recognizes block-header colons without suppressing lambda,
slice, dictionary or soft-keyword variable contexts. Unit/editor regressions
were observed failing before the corresponding fixes and passing afterward.

Fixed 30-case results: baseline 15 correct / 11 wrong / 4 withheld; selected
profile 24 correct / 6 wrong / 0 withheld. Holdout results improve from 9/15 to
13/15 correct. This targeted set does not establish general semantic accuracy.
