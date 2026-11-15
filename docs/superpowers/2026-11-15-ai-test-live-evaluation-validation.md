# Live test-generation comparison validation

Report: [local 27B thinking versus DeepSeek](../benchmarks/2026-11-15-ai-test-generation/report.md).

Completed six fixed targets per primary arm, two each of C/C++/Python, using
production prompts, transport, validation, project adapter and real compiler/test
execution. Independent gold controls passed originals, detected all 18 mutants,
and passed restored originals before any model requests. All six messages hashes
match between arms. Primary observations were not overwritten by diagnostic runs.

Recomputed from per-case artifacts:

| Metric | 27B thinking | DeepSeek non-thinking |
|---|---:|---:|
| Complete HTTP responses | 5/6 | 6/6 |
| Production-accepted response | 5/6 | 5/6 |
| Original deployed environment pass | 3/6 | 1/6 |
| UTF-8 replay, strict response acceptance and original test pass | 4/6 | 3/6 |
| Mutants detected by those usable suites / all 18 available | 12/18 | 9/18 |
| HTTP-response generation median | 62396 ms (n=5) | 4871.5 ms (n=6) |

The missing local response is a preserved transport failure. DeepSeek's missing
`assumptions` envelope remains rejected; diagnostic raw code passes separately,
making code-only DeepSeek results 4/6 and 12/18, without changing strict counts.

Separate supplemental calls: local copy test retry 61850 ms, rejected at compile
for conflicting C/C++ declarations; three DeepSeek thinking calls 21935/23319/
23611 ms, all truncated at 4096 completion tokens. No supplemental result replaced
a primary observation or contributed to primary latency statistics.

## Encoding regression and fix

MSVC codepage 936 misread generated UTF-8 comments. The unchanged first local C
suite passed all 21 tests with `/utf-8`. A minimal Chinese-comment regression in
the real CMake smoke suite reproduced zero collected tests without the fix.
Generated CMake now adds `/utf-8` through a SOURCE property with TARGET_DIRECTORY,
only for the generated file on MSVC. Existing handwritten source encoding and
business-library compiler settings remain intact.

- Actual installed-GTest, existing-target and FetchContent smoke tests: 3 passed.
- Final default tests: 151 unit tests and 2 doctests passed; 12 opt-in unit tests
  and 1 doctest ignored.
- Final `cargo build --release -j 2 -p helix-term --bin hx`: passed, 3m05s.
- Release `--version` and local/DeepSeek config health parsing: passed.
- Changed Rust files passed rustfmt; benchmark Python scripts passed Ruff
  lint/format and Python compile checks; `git diff --check` passed.
- Fixed fixture data remained identical after script formatting.

A fresh read-only reviewer independently recomputed the timing and strict/code-only
counts and found no material counting or per-source encoding-fix defect. Review
identified unsafe unbounded string reads in DeepSeek's copy tests under the
termination mutant; the report records this quality defect and retains observed
mutation counts because bounded assertions also independently detect the defect.

The initial final-test attempt hit Windows LNK1104 because the benchmark executable
was still running. After the evaluation exited, the sequential rerun passed; that
infrastructure error is not counted against either model.

Model servers launched by this task exited; no external editor/server was stopped.
User model defaults and thinking switches were not changed. No generated test or
business fixture was hand-repaired for scoring. Artifacts were checked for the
configured API credential value and contain no such credential. Responses retain
final answers/usage, with reasoning trace text removed. Source snapshot hashes and
matched prompt hashes passed verification.
