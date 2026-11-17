# AI Commit File Limit Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this small correction inline, with test-driven-development and verification-before-completion.

**Goal:** Make configured AI commit file limits consistent and generation scope accurate.

**Architecture:** Share `validate_file_count(config: &AiCommitConfig, count: usize) -> Result<()>` between UI preparation and handler input validation. Count the IDs that the operation actually analyzes.

**Tech Stack:** Rust, Tokio, native Git integration tests, local HTTP fixtures.

**Spec:** `docs/superpowers/specs/2026-11-16-ai-commit-file-limit-design.md`

## Global Constraints

- Default 64 files; configurable positive limit capped at 1024.
- Keep 64 model calls, complete diffs, byte/context budgets and explicit writes.
- Reuse the clean existing feature checkout; leave changes uncommitted for review.

## Review Focus

- Zero configuration must fail consistently before HTTP.
- Smaller configured limits and the 1024 ceiling remain enforced.
- All 65 files must survive batching and final aggregation.
- Unstaged files cannot block a valid staged-only generation.
- Recommendation/generation must leave index and HEAD unchanged.

### Task 1: Correct the shared limit and operation scope

**Files:** handler `helix-term/src/handlers/ai_commit.rs`, its `tests.rs`, UI
`helix-term/src/ui/git/commit/tasks.rs`, native tests
`helix-term/tests/test/ai_commit.rs`, and `book/src/editor.md`.

- [x] Add failing boundary and HTTP tests for configured 65-file operations.
- [x] Add a native regression with two changed files, one staged file and `max-files = 1`.
- [x] Run the new tests and record the original failures.
- [x] Implement the shared validator and call it with the filtered ID count.
- [x] Document the effective file ceiling and scope.
- [x] Run focused client/native suites, formatting and whitespace checks.
- [x] Review the final diff, record results, and leave changes for user review.

## Execution record

Reused the clean existing feature checkout on `codex/git-merge-conflicts`.
Implemented inline and left changes uncommitted. One read-only reviewer found
no material issues. Results and test-environment observations are recorded in
`docs/superpowers/reports/2026-11-16-ai-commit-file-limit-validation.md`.
