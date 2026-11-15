# Local explicit-generation validation

Implemented user-approved local routing: completion remains on 9B, while both
documentation and commit analysis use the shared managed 27B provider on demand.
The global default-provider still switches all three features to DeepSeek.

## Behavioral evidence

- Conditional routing and finite-thinking configuration failed before support
  was added, then passed for local/online and explicit feature overrides.
- Reasoning transport/budget tests failed on missing effort and combined token
  allowances before implementation. Both generation clients now include bounded
  reasoning in output/context reservations and keep final-content validation.
- A real lightweight child-process regression reproduced resident 9B remaining
  alive during temporary startup. Leases now await owned process exit before
  temporary startup and restore latest residents after completion or cancellation.
- Seven lifecycle tests cover success/error, loading/inference cancellation,
  startup failure, restore cancellation, reload/shutdown, concurrent leases and
  external resident/temporary ownership.
- Independent read-only review found temporary external-server reuse could
  prevent unloading. Its regression failed, then passed after adding preflight
  port checks, exclusive temporary startup and ownership validation.

## Actual GPU validation

The user confirmed the editor was saved/closed. One orphaned 9B process was
identified by executable/model/port and absent parent, then stopped for the
authorized model rotation. No editor process was terminated.

On the RTX 4090, the actual production commit pipeline successfully performed
9B -> 27B medium thinking (2048 reasoning tokens) -> 9B, producing a valid message.
The temporary endpoint was released and the baseline ready before return.
See `docs/benchmarks/2026-11-14-local-generation/model-switch-smoke.json`.

## Quality comparison

36 primary observations: 6 documentation + 6 commit tasks for each of 9B direct,
27B direct and 27B bounded-thinking. Production prompts, batching and validators
were used with synthetic inputs. Report, fixed rubric, hash-bound reviewed
grades, source snapshots, original results/errors, latencies and scripts are
provided in `docs/benchmarks/2026-11-14-local-generation/`.

This small set does not establish population accuracy. Documentation rubric
scores were 75.0 / 91.7 / 100.0; commit scores 95.8 / 100.0 / 83.3. Bounded thinking
failed one commit selection by returning a string file ID instead of an integer;
an extra diagnostic reproduced it and is excluded from primary metrics.

The installed configuration retains bounded thinking as explicitly requested;
the report recommends considering non-thinking 27B for commit reliability and
latency on this evidence. No silent model-output coercion or scoring-set changes
were introduced to turn the failed case into a pass.

## Installation and verification

- Release built with `cargo build --release -j 4 -p helix-term --bin hx`.
- Actual user config backed up and updated with Chinese comments. Both local
  and single-switch DeepSeek configurations passed release-binary validation.
- Original completion tuning, 9B profile, DeepSeek profile, non-AI editor
  settings and keybindings were preserved by parsed-value equality checks.
- Final `cargo test --config 'profile.test.package.helix-term.debug=0'`: 137 unit
  tests and 2 doctests passed; 7 opt-in unit tests and 1 existing doctest skipped.
- View AI configuration/completion/documentation tests: 17 passed.
- AI editor integration tests: 57 passed, 7 opt-in tests skipped.
- Actual GPU switch test: 1 passed; the two explicit quality evaluations ran
  through the separate controlled comparison (36 primary observations).
- Changed Rust files passed rustfmt checking; both Python scripts passed Ruff
  lint/format checking; `git diff --check` passed. Existing non-Git VCS unused
  variables and integration helper dead-field warnings remain unchanged.
