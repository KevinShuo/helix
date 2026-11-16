# Local inference performance implementation plan

> **For agentic workers:** Use superpowers:executing-plans for inline execution.

**Goal:** Maximize verified local inference performance on this RTX 4090.
**Architecture:** Measure one model process at a time; expose typed runtime
settings; install only configurations that pass latency and correctness checks.
**Tech Stack:** Rust, Tokio, serde/TOML, Python standard library, CUDA llama.cpp.
**Spec:** `docs/superpowers/specs/2026-11-16-local-inference-performance-design.md`

## Global constraints

Preserve Q5 model, 64K context and existing thinking choices in the primary arm.
Back up installed config; preserve unrelated working changes and editor buffers.
Synthetic local requests only. No arbitrary extra process arguments.

## Review focus

- Invalid runtime settings fail before a process starts.
- Reloading runtime settings replaces a server; request-only edits keep it.
- Defaults preserve old command-line behavior.
- Failed experimental model processes are reaped before subsequent trials.
- Runtime-specific options cannot silently affect remote model requests.

## Tasks

- [x] 1. Add failing TOML/validation tests for provider runtime settings. Implement
  typed cache/speculation enums and bounded optional numeric settings in
  `helix-view/src/ai/runtime.rs`; integrate with provider validation. Verify with
  `cargo test --profile integration -p helix-view --test ai_providers`.
- [x] 2. Add a real child-process argument/reload regression. Implement runtime
  argument construction and identity comparison in managed server code; verify
  server and lease tests using the existing model-server fixture.
- [ ] 3. Add `contrib/ai-local-performance.py`: explicit arms, proxy-free HTTP,
  process cleanup, immutable JSON observations, reproducible synthetic workloads.
  Measure current baseline, KV/offload, speculation and batch/thread candidates.
  Reject OOM, CPU offload, invalid output and latency regressions.
- [ ] 4. Repeat finalists including long-context and production structured-output
  checks. Document tested limits and actual speedups, update provider docs and
  examples. Run package/workspace checks, build release, back up and update
  installed config, verify live model switching and executable/config parsing.
- [ ] 5. Independent final review and completion audit against this spec.

## Execution ledger

- Initial inspection: 27B current logs show 8–9 t/s at 64K/56 GPU layers; 9B
  resident consumes about 10.8 GiB total GPU memory. Waiting for editor shutdown
  before exclusive GPU trials; code and fixture work proceeds independently.
- Use existing `codex/git-merge-conflicts` checkout: it contains necessary local
  AI changes and user work. No branch reset or unrelated commit is authorized.
- Runtime parser RED: unknown `runtime` field; GREEN: 19 provider tests pass.
- Real process RED: child receives F16 instead of Q8; GREEN: 18 server/lifecycle
  tests pass (one explicit GPU test ignored). Reloading threads restarts; changing
  thinking does not. Stub records actual argv, including model paths with spaces.
- Screening: Q8 KV permits 66/66 GPU layers; 36.1 t/s versus 7.9 baseline.
  MTP3 reaches 93.0 t/s and passes the same 18 generated-code assertions. These
  are screening observations; repetitions and production workloads remain.
- Official b12428 CUDA 12.4 and CUDA dependency archives downloaded with SHA256
  checks against the GitHub release asset digests. Comparison pending; no runtime
  or installed configuration has been replaced yet.
- Backend sampler opt-in: TOML parser RED (unknown field), child argv RED
  (missing GPU sampling flag), GREEN: final terminal library 227 passed / 13
  ignored. Earlier workspace run: 885 passed / 17 ignored / 4 existing Windows
  path/symlink/hook filters. The final source will receive targeted checks too.
- Independent runtime/harness review found two P2 issues: metadata write could
  leak a newly started process, and production schema acceptance was called
  semantic passing. Both reproduced by Python tests (2 failures), then fixed
  (2 passes): metadata is inside the cleanup block; expected file selections are
  checked and text output requires a separate recorded-output review.
- Final review rulings: benchmark winners, installation and live behavior remain
  evidence gates below; universal runtime/model compatibility is intentionally
  not claimed, with explicit MTP compatibility errors preserved; quantized caches
  are not claimed numerically equivalent; unrelated budget/gitignore changes are
  outside this task and remain untouched.
- Long-input evidence: 55026-token decode and 55077-token retrieval in the full
  65536 window. First Q8 production run has 6 accepted doc results and 5/6 commit
  results; `commit_select_tests` fails the existing every-file explanation check.
  Preserve this first failure; compare profiles and baseline before installation.
- 9B MTP is unsupported by the installed GGUF (no prediction heads). Q8+ngram
  matches plain Q8 output bytes on the controlled prompt and JSON task; repeated
  source generation improves from ~109 to ~175 t/s, non-repetitive output ~109.

## Follow-up: complete switching and UI regressions

The user explicitly requested completing actual model switching verification and
fixing the three remaining UI regressions after clearing the standing goal.

- [x] Reproduce Merge Apply failure on an otherwise idle machine.
- [x] Diagnose before changing waits: normal PATH invokes Scoop's Git launcher;
  67 completed Git calls take 12.585s total, median 186.86ms. The fixed 100-poll
  wait expires during a still-running optimistic validation. The same unchanged
  UI test passes with direct Git (10.54s), confirming a timing-dependent wait.
- [x] Actual installed 9B -> 27B generation -> 9B readiness: passed in 18.83s.
- [x] Replace poll-count waiting with bounded transaction completion, preserve
  exact disk/index/BOM/CRLF/undo assertions, and rerun all three failures.
- [x] Probe actual contract-defined completion before and after the real switch:
  both return `b` for add(a,b); installed profile switches/restores successfully.
  Enhanced opt-in test passes in 20.04s (16.59s recorded switch/post-completion).
- [x] Run AI UI regression group and package checks, independently review fixes,
  and update the validation report with final source/current installed state.
- Merge Apply with bounded state completion passes under the original Scoop Git
  PATH in 21.50s; exact staged text, unrelated conflict count, undo and disk/index
  assertions are retained. Temporary Git tracing removed; production VCS diff empty.
- Independent review accepts the completion waits and probes; P2 finding: preview
  startup allowance must resolve the named Completion provider, not legacy fields.
  Added a provider-precedence regression before fixing this helper.
- An attempted `--features integration --lib` run has six synchronous function-hint
  tests failing at RuntimeLocal::deref (`no reactor running`); that feature is used
  for native UI tests. Re-running the standard default-feature library command;
  the failed feature-combination attempt remains in its diagnostic log.
- Final gates: AI UI 77 passed / 10 ignored; after the reviewer P2 fix,
  Completion subset 33 passed / 6 ignored, startup budget RED 17s vs62s then GREEN.
  Standard library serial 227 passed / 13 ignored. A preceding parallel attempt
  had one loopback HTTP connection failure; it remains in its diagnostic log.
- Actual managed-model TUI fixture using installed Completion provider: passed
  9.20s; preview sum(items), acceptance, disable/re-enable, loading cancellation,
  reopening and final port release all verified. No model processes remain.
- Final report: docs/superpowers/reports/2026-11-16-local-ai-switch-ui-validation.md.
