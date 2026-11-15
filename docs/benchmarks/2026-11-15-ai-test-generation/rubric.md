# Predeclared test-generation comparison

Six fixed synthetic targets: C unsigned-16 parsing and bounded string copy;
C++ inclusive range merging and stateful quota consumption; Python ASCII port
parsing and stable key-based deduplication. Each has an explicit visible contract,
independent gold tests and three predetermined behavioral defects (18 total).

Arms: installed Qwen3.8-27B-UD-Q5_K_M with thinking enabled, medium effort and
2048 reasoning-token cap; configured DeepSeek model with its existing thinking
setting (deepseek-flash, disabled). Matched 16384 context, 4096 final tokens,
production temperature 0.0, JSON output and 120-second request timeout. Local
reasoning receives an additional 2048 output-token reservation. Exact message
hashes must match between arms. Requests use actual production prompt builder,
HTTP transport and validator; benchmark records final responses before validation.
Model-server startup is measured separately. No automatic or manual repair,
silent output coercion or retry contributes to primary results.

Before any model requests, gold suites must pass on original implementations,
detect all 18 mutants, and pass again after restoring the originals. Every
generated suite then uses production project integration and runner with real
CMake/vcpkg GoogleTest or pytest. Mutants alter only synthetic scratch sources;
generated tests remain unchanged. Python bytecode writing is disabled to avoid
subsecond timestamp-cache artifacts. Source is restored and rerun after mutants.

Report separately:

1. Accepted outputs / 6 (complete JSON, production syntax/assertion validation).
2. Original implementation compile/run success / 6; failures remain failures.
3. Detected behavioral mutants / 18, plus actual viable/exercised denominator.
   A mutation counts as detected only after successful configure/build followed
   by test failure, excluding timeouts/environment failures.
4. Generation median/total and each case's full generation latency; local load
   time separate. Original build/test timing is separate from model generation
   and mutation-testing overhead.
5. Manual review of successful suites for independent expected values, documented
   normal/edge/error cases, unsupported assumptions, unsafe test behavior and
   fixture/API integration. Use the case-specific review notes fixed in cases.json.

One observation per case per arm. These are small handpicked samples, not a
population accuracy estimate. Claims about models must stay within this dataset.
Retain rejected model content, compiler/test logs and surviving mutants. Do not
drop failed cases or adjust defects/scoring after outputs are observed. Diagnostic
reruns, if needed, stay separate from primary observations.
