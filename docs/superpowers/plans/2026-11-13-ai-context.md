# Grounded AI Completion Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline, then a fresh read-only review.

**Goal:** Reduce invented variables/APIs by providing bounded declarations and real member candidates.

**Architecture:** Add an AI-context module for syntax-tree notes and LSP member
metadata. The existing completion handler captures the document snapshot and
connected server, fetches metadata after debounce, and passes it to the current
chat adapter. The adapter bounds prompt context and validates authoritative names.

**Tech Stack:** Existing Rust, tree-sitter, LSP, Tokio and reqwest; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-13-ai-context-design.md`

## Global constraints

- Preserve the preceding conservative implementation and all uncommitted changes.
- Keep running editor/model processes alive; no commit, push or public release.
- Preserve snapshot/version cancellation and ordinary menu behavior.
- Prefix plus notes stay within configured `prefix-chars`; notes cap 600+600 chars.
- LSP metadata is optional, cancellable, and bounded by a 200ms wait.

## Review focus

- Missing grammar/incomplete syntax must fall back to nearby text.
- UTF-8, CRLF and long current lines must preserve the exact typed prefix.
- Imports/function declarations must be labeled as separate context, not spliced into source order.
- Incomplete/large/deprecated/non-symbol completion lists must not become false authority.
- Failed or canceled LSP requests must not block model fallback or display stale suggestions.

### Task 1: Recover missing source declarations

Files: create `helix-term/src/handlers/ai_completion/context.rs`; modify handler/client and AI integration tests.

Interface: `source_notes(text: RopeSlice, syntax: Option<&Syntax>, cursor: usize, prefix_chars: usize) -> String`.

- [x] Add editor HTTP-capture regression requiring a distant Python signature and import alias.
- [x] Observe the missing-context assertion fail.
- [x] Implement bounded ancestor/import collection, supplemental-context budgeting and prompt inclusion.
- [x] Test no syntax, Unicode/CRLF, small budgets, duplicate nearby declarations and long current lines.

### Task 2: Use connected language-server members

Files: context module, handler/client and client tests; real clangd/editor probe.

Interface: `MemberContext::from_response(CompletionResponse, stem: &str, budget: usize)` and first-member validation.

- [x] Add failing candidate-metadata, invented-member, partial-name and incomplete-list regressions.
- [x] Fetch one configured completion server after debounce with cancellable request and 200ms timeout.
- [x] Export bounded nondeprecated labels/details; validate names only for usable complete small lists.
- [x] Run AI/editor tests, normal `cargo test`, format/whitespace checks and local-model comparisons; configuration code was unchanged this round.
- [x] Perform a fresh independent review, build/stage release preserving existing executable, and verify installed health.

## Execution ledger

Ruling: keep the current checkout and execute directly — the user previously
asked for direct conservative implementation and now requests further work.
Reuse the existing local server and settings; no additional confirmation needed.
