# Provider-aware AI Budgets Implementation Plan

**Goal:** Automatically use recognized DeepSeek capabilities, with manual limits
and unchanged configured local behavior.

**Spec:** `docs/superpowers/specs/2026-11-16-ai-provider-budgets-design.md`

**Architecture:** Shared budget methods on `AiProviderConfig`; transport enforces
memory/deadline limits and bounded API-guided output reservation. Clients use the
same profile for preparation, batching, output and preview size checks.

**Tech Stack:** Rust, Serde, Reqwest, Tokio virtual-clock tests and loopback HTTP.

## Tasks and ownership

- [x] Policy: `helix-view/src/ai.rs`, new `helix-view/src/ai/budget.rs`, provider
  tests. Add serialized auto/configured switch and pure budget methods.
- [x] Transport: `helix-term/src/handlers/ai.rs` and `ai/tests.rs`. Read complete
  large responses, honor provider timeouts and cap retry allocation at three
  attempts using finish reason / prompt-token usage. Preserve redacted errors.
- [x] Artifacts/preview: `ai_doc/format.rs`, `ui/ai_merge.rs` (and doc preview if
  needed). Provider-sized rendering with complete, safe, scrollable previews.
- [x] Integration: explicit client request/validation methods, commit UI/model
  scope and adaptive batch limits; provider-sensitive tests and documentation.
- [x] Verify policy and terminal tests, review changes, update only the existing
  DeepSeek provider's budget-mode in user config, build/install and health-check.

Write regressions before implementation, observe old failures where interfaces
already exist, and verify complete payloads, manual/local behavior, unknown IDs,
context allocation, bounded attempts, large responses and preview tails. No real
cloud requests or real repository writes in validation. No commits or pushes.
