# Predeclared local generation comparison rubric

Arms: Qwen3.5 9B Q6_K without thinking; Qwen3.8 27B Q5_K_M without thinking;
the same 27B with medium thinking and a 2048-token reasoning cap. All use 16384
context, production temperature 0.0, a 1024-token final allowance and a 120-second
per-request timeout. One run per case. Loading time is recorded separately.

The ignored Rust tests call production prompt builders, HTTP transport,
batch analysis, formatters and output validators. Inputs are synthetic, never
the user's actual repository. Six documentation cases and six commit cases run
per arm (36 feature-level observations). Commit cases use multiple HTTP requests.

Each observation has four binary criteria worth 25 points each. Failure to
produce a result accepted by production validation scores zero. A perfect-case
pass requires all four criteria. No scoring criteria may be changed after results.

## Documentation criteria

1. Valid structure: production renderer accepts JSON, parameters/templates and
   output text; source is not edited by the model.
2. Core behavior: accurately describes the `core` fact saved with the case.
3. Edge behavior: includes the case-specific `edge` condition (all substantive
   parts); no requirement for a particular phrasing.
4. Grounded claims: no extra behavior, guarantees, exceptions or validation
   unsupported by the visible function body.

Cases: mean/empty zero; port conversion and inclusive bounds/ValueError;
awaited item count; positive generator preserving order; nullable C pointer
mutation; C++ templated clamp and its three branches.

## Commit selection criteria

1. Production accepts the complete structured result.
2. Includes exactly the intended candidate, ID 1.
3. Excludes every unrelated/staged/partial/sensitive candidate as specified.
4. Every reason is grounded in provided changes or explicit intent; it does not
   invent changes or claim sensitive/unavailable contents were analyzed.

## Commit message criteria

1. Production accepts Conventional Commits text and no ungrounded test claims.
2. Type equals the case's `expected_type`.
3. Message accurately covers the case's `core` change.
4. No extra changes, guarantees, test outcomes or behavior absent from the diff.

Report mean rubric score, perfect-case pass count, accepted-output count and
median feature latency, separately by feature and arm. Compare percentage-point
deltas; do not treat this small handpicked set as a population accuracy estimate.
Provide raw outputs and per-case grades so the judgment is auditable. This
compares two model generations/quantizations as deployed, not parameter count alone.
