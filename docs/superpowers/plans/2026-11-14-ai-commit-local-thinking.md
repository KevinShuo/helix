# Local documentation and commit thinking implementation plan

> **For agentic workers:** Use superpowers:executing-plans and TDD in this checkout.

**Goal:** Local documentation and commit analysis use bounded 27B reasoning on
demand and restore 9B, while one global setting continues to select online mode.

**Spec:** docs/superpowers/specs/2026-11-14-ai-commit-local-thinking-design.md

**Architecture:** Conditional provider resolution, provider-specific reasoning
parameters and a shared asynchronous model lease owning temporary transitions.

**Constraints:** Preserve explicit/legacy routing, never kill external processes,
never auto-stage/commit, restore latest configuration after cancellation, and keep
user configuration outside AI unchanged.

**Review focus:** canceled loading; reload/shutdown during restoration; baseline
external server ownership; concurrent acquisitions; reasoning tokens exhausting
JSON output/context.

- [x] Configuration: test inherited-local commit override, online fallback,
  explicit override, invalid references and on-demand residency; implement in
  helix-view/src/ai.rs and ai_commit.rs. Run view AI configuration tests.
- [x] Thinking: test llama.cpp wire fields and combined output/context budgets;
  implement provider budget/effort handling in handlers/ai.rs and ai_commit.rs.
  Verify against installed b11081 API.
- [x] Lifecycle: test using a real lightweight child process (no GPU/model) that
  resident servers exit before temporary startup and return after success,
  failure, canceled loading, canceled inference, reload and shutdown. Implement
  leases and process ownership in handlers/ai_completion/server.rs and wrap both
  multi-request commit operations.
- [x] Verify default and AI integration suites, format/diff checks and independent
  read-only review. Address findings with regressions.
- [x] Update docs/example and actual local config with Chinese comments; build
  installed release once verified, validate actual config and run a bounded local
  model smoke test if feasible without disturbing an existing session.

User steering also required real quality measurements: completed three arms x twelve tasks, recorded fixed-rubric grades and raw data under docs/benchmarks/2026-11-14-local-generation/. Actual GPU switching, installed release and local/online config validation passed. Full final test results are recorded in docs/superpowers/2026-11-15-ai-local-generation-validation.md.
