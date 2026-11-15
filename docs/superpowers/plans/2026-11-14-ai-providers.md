# Unified AI providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Independently select local or online models for all three AI features while preserving legacy configuration.

**Architecture:** Resolve named AiProviderConfig per AiFeature. Keep prompt generation separate from provider transport and pool application-owned local servers by endpoint.

**Tech Stack:** Rust, serde/TOML, reqwest, Tokio.

**Spec:** docs/superpowers/specs/2026-11-14-ai-providers-design.md

## Global Constraints

- Existing configurations and disabled-by-default behavior remain compatible.
- Explicit unknown references fail, never fall back.
- Remote providers never start local processes.
- Configuration changes invalidate pending results.
- Work in the existing checkout; do not modify unrelated user files.

## Review Focus

- Default provider plus a local feature override must not route other features locally.
- Disabled features must not start unused managed models.
- Local providers sharing one endpoint with different model identities must fail.
- Reload must invalidate pending explicit generation and remove unused servers.
- Authentication, proxy and HTTP errors must not expose credentials or code.

### Task 1: Configuration and resolution

**Files:** new helix-view/src/ai.rs, helix-view/tests/ai_providers.rs;
modify editor.rs, lib.rs, ai_completion.rs, ai_doc.rs, ai_commit.rs,
helix-term/src/config.rs.

**Interfaces:** AiFeature::{Completion,Doc,Commit};
Config::ai_provider(AiFeature) -> anyhow::Result<AiProviderConfig>;
Config::active_ai_providers() -> anyhow::Result<Vec<AiProviderConfig>>;
Config::validate_ai() -> anyhow::Result<()>.

- [x] Write and run tests for precedence, three independent models, legacy fields,
  missing names, cloud model-path rejection and distinct local endpoints.
- [x] Implement provider types/defaults, legacy conversion, selectors and validation.
- [x] Run `cargo test -p helix-view --test ai_providers` and existing AI view tests.

### Task 2: Runtime and transport

**Files:** helix-term/src/handlers/ai.rs, ai_completion/{client,server}.rs,
ai_completion.rs, ai_doc{,/client}.rs, ai_commit{,/tests}.rs,
application.rs, ui/git/commit/tasks.rs, helix-view/src/ai_doc.rs.

**Interfaces:** Shared post consumes AiProviderConfig; completion client additionally
consumes feature settings; LocalServers::new/configure consume active providers.

- [x] Write and run request tests for DeepSeek thinking, generic parameter isolation,
  cloud readiness without processes, mixed routing, multiple local servers/reload.
- [x] Implement provider-specific wire parameters and client reuse, separate local
  server readiness from remote calls and wire all feature selectors/snapshots.
- [x] Run `cargo test -p helix-term --lib ai_`.

### Task 3: Documentation and verification

**Files:** book/src/editor.md, book/src/configuration.md; verification report.

- [x] Document all provider keys, independent feature overrides, environment key,
  local/cloud example, precedence, proxy modes and migration.
- [x] Run formatting on changed Rust files, `cargo test` and targeted view tests;
  run integration AI tests where supported; inspect diff and review lifecycle,
  reload, credential handling and legacy behavior.
- [x] Record actual results and limitations; leave changes available for user review.
