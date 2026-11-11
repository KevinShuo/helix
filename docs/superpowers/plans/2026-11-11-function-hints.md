# Function Hints Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans for editor integration and
> superpowers:dispatching-parallel-agents for the independent Git backend.

**Goal:** Show function usage counts and the most recent Git author beside declarations.

**Architecture:** LSP document symbols identify visible functions. Asynchronous
reference requests and Git range history produce cached, view-scoped virtual text.

**Tech Stack:** Rust, existing Helix LSP client, Tokio, Git CLI, inline annotations.

**Spec:** `docs/superpowers/specs/2026-11-11-function-hints-design.md`

## Global Constraints

- Preserve existing edits and reuse the current checkout. The user subsequently
  requested committing this feature and pushing it to `KevinShuo/helix` on `master`.
- No new language-specific parser and no reference guesses from text matching.
- Optional features default off; honor workspace trust and non-Git builds.

## Review Focus

- Cross-file edits and delayed responses must not leave stale usage counts.
- Functions containing only deletions must attribute the actual latest commit.
- Unsaved insertions must not move authors onto unrelated functions.
- Unsupported/failed queries must not appear as a zero count.
- Unicode and CRLF must anchor hints at the right declaration line.

## Task 1: Git history backend

Files: `helix-vcs/src/function_authors.rs`, `helix-vcs/src/lib.rs`, `helix-vcs/Cargo.toml`.
Interface: async `function_authors(PathBuf, String, Vec<Range<usize>>, bool)` returns
ordered optional `FunctionAuthor { author, commit, modified }` records.

- [x] Write and run failing real-repository tests for authors, deletions and dirty ranges.
- [x] Implement bounded, cancellable Git history queries with safe argument handling.
- [x] Run `cargo test -p helix-vcs --features git` and check feature-disabled compilation.

## Task 2: Function models, configuration and rendering

Files: new `helix-view/src/function_hints.rs`, and `lib.rs`, `editor.rs`, `document.rs`, `view.rs`.

- [x] Test nested/flat function collection, Unicode line ends and count deduplication.
- [x] Implement symbol conversion and view-scoped annotations with two config switches.
- [x] Test annotation invalidation and that rendering leaves document text unchanged.
- [x] Run `cargo test -p helix-view function_hints`.

## Task 3: Background requests and lifecycle

Files: new `helix-term/src/handlers/function_hints.rs`, `handlers.rs`, `ui/editor.rs`.
Also: `helix-lsp/src/client.rs` for opt-in wire cancellation and hierarchical symbol
capabilities; `helix-term/src/commands/lsp.rs` for picker container compatibility.

- [x] Test stale request rejection and request parameters at the LSP boundary.
- [x] Schedule visible functions on idle, with cancellation, caching and limited concurrency.
- [x] Independently attach reference and author results; clear on edits/config/server changes.
- [x] Run term tests, standard `cargo test`, affected crate suites and formatting checks.

## Task 4: Documentation and review

Files: `book/src/editor.md`, `book/src/themes.md`, `book/src/configuration.md`.

- [x] Document switches, styles, semantics, refresh behavior and prerequisites.
- [x] Review the complete diff and run relevant final verification.

## Execution notes

The screenshot and user clarification establish the requested behavior. Routine
implementation choices above are the agent's choices. Continue within the authorized
development scope without additional approval gates. Existing branch:
`codex/ai-inline-completion`; existing user changes remain outside this feature.

Rulings: flat LSP symbols never supply Git extents; unknown author data is omitted.
Cross-file invalidation is shared with rendering and wakes in-flight work immediately.
Git range replacements crossing a function boundary are treated as ambiguous.
Validation evidence and existing environment limitations are recorded in
`docs/superpowers/2026-11-11-function-hints-validation.md`.
