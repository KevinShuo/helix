# DeepSeek thinking 16K evaluation validation

User requested increasing DeepSeek's output budget and rerunning all six fixed
C/C++/Python test-generation cases. Report:
[full six-case result](../benchmarks/2026-11-15-ai-test-generation/deepseek-thinking-16k-full/report.md).

## Configuration and scope

- Actual configured `deepseek-flash` endpoint/API credential environment variable.
- Thinking enabled; API default high effort; total max_tokens 16384; context
  reservation 32768; original 120-second request timeout.
- No retries, automatic repairs or manual generated-code edits.
- Fixed six cases and 18 mutations unchanged. Every messages array matched its
  saved previous non-thinking counterpart before HTTP dispatch and afterward.
- Daily user configuration hash unchanged across the run.
- The 16K override is in the cfg(test) evaluation harness only. Production feature
  max_tokens validation still caps at 8192. No production code/release change.

## Measured outcomes

- 6/6 HTTP responses, all finish_reason stop, nonempty reasoning metadata.
- 6/6 production-accepted structured outputs; 6/6 real original-implementation
  configure/build/test successes using CMake/vcpkg GoogleTest or pytest.
- 18/18 defects reached the Test stage and failed; no compile failures/timeouts
  counted as detected defects. All six restored originals passed afterward.
- Generation times: 24847 / 26994 / 11362 / 21524 / 17399 / 28312 ms.
- Median 23185.5 ms; total 130438 ms; completion tokens total 29316, range
  2382..6265, no truncation. Compile/execution times are separate in the CSV.

## Verification and review

- Budget-override regression first failed at 4096 versus expected 16384, then
  passed after implementation. It checks unchanged messages and failure without
  mutation when context reservation is insufficient.
- Final default tests: 152 unit tests and 2 doctests passed; 12 opt-in unit tests
  and 1 doctest ignored. Evaluation finished before Cargo was run, avoiding the
  prior task's Windows executable-lock issue.
- Changed Rust harness passed rustfmt; Python runner passed Ruff lint/format and
  Python compilation; git diff --check passed.
- Saved generated code bytes match unmodified model final-output code values.
- Snapshot hashes, six request budgets, thinking metadata, prompt equality,
  mutation stage/viability and restoration results were independently checked.
- Fresh read-only review independently confirmed counts and latency arithmetic.
  It identified a remaining model-generated copy-test safety issue: EXPECT_STREQ
  failure diagnostics may read outside all-nonzero buffers when NUL is missing.
  Other bounded assertions also detect that mutation; observed counts remain,
  while the report explicitly avoids claiming memory-safe or sufficient coverage.
- Saved request.json is the pre-transport body. Provider settings and shared
  ai.rs insert thinking enabled. Transport snapshot/hash was added after the run;
  source modification time predates the initial manifest and it was not modified
  during evaluation. Capture timing is recorded explicitly.
- No reasoning trace text retained and no configured API credential value found
  in artifacts. Only synthetic fixtures were transmitted.

Old observations are preserved. This is a combined change in thinking and output
budget with one sample per case, not a causal ablation or population accuracy
estimate. No claim that a separate 8192-budget rerun would produce the same result.
