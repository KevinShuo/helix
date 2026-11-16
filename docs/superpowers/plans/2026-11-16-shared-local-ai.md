# Shared local AI implementation plan

> **For agentic workers:** Use superpowers:executing-plans; the user approved the design and requested direct execution.

**Goal:** Multiple hx processes share local model ownership and switching without terminating each other's models.
**Architecture:** A per-user hx helper owns the existing LocalServer implementation. Authenticated, bounded loopback control connections register editors and hold FIFO request leases across inference. Existing isolated pools remain available to the integration harness and low-level lifecycle tests.
**Tech stack:** Rust 1.90, Tokio, serde JSON, OS file locks, getrandom, existing llama.cpp runtime.
**Spec:** docs/superpowers/specs/2026-11-16-shared-local-ai-design.md (approved by user: “直接做”).

## Global constraints

- Preserve existing provider configuration and HTTP endpoints; never terminate an unowned server.
- One active managed model/request at a time; cancelable FIFO scheduling; restore the requesting editor's resident after temporary inference.
- The manager survives its initiating editor; exit and release models 10 seconds after the last editor/request connection disconnects.
- Bound/authenticate local control messages, fail clearly on protocol mismatch, and serialize process startup using an OS file lock.
- Work in the existing checkout on codex/shared-local-ai; retain unrelated untracked files and back up the installed executable before replacement.

## Review focus

- Simultaneous discovery/startup must select one manager without rewriting live credentials.
- Disconnects during queueing/loading/inference/restoration must release the lease and preserve other editors.
- A reused unknown external server must never be killed to free GPU memory.
- Reloaded/unequal settings must not silently reuse an incompatible managed process.
- Client/manager crashes and stale discovery metadata must permit later reconnection without replaying generation.

## Task 1: Shared manager and process protocol

Files: create `helix-term/src/ai_manager/{mod,protocol,daemon,client}.rs`; add the module in `lib.rs` and hidden entry in `main.rs`; add direct getrandom dependency already in Cargo.lock. Add `contrib/test_shared_ai.py` process regressions using the real hx executable and existing synthetic model-server fixture.

- [x] Write and run process regressions against current hx: manager startup/lifecycle, simultaneous startup, exclusive queued leases, disconnect during loading/inference, temporary model restore, config replacement, unknown external owner preservation, stale metadata restart, authentication/frame limits.
- [x] Implement length-prefixed versioned JSON, random 256-bit discovery credential, private cache directory, persistent editor registration and per-request lease connections.
- [x] Implement helper singleton using `File::try_lock`, independent hidden process spawn, bounded handshake/startup and reconnect. No retry of user inference after acquisition failure.
- [x] Implement FIFO scheduling over existing `LocalServer`, owned-process checks, cancellation and 10-second idle cleanup.
- [x] Run regressions to GREEN and check formatting/compile; commit this task.

## Task 2: Connect editor lifetime and all managed inference

Files: `application.rs`, `handlers/ai_completion/server.rs`, `server/pool.rs`, `handlers/ai_completion.rs`, and shared client module.

- [x] Add regression demonstrating that resident requests are serialized against temporary generation and that closing the first editor preserves the second registration.
- [x] Make Application register/configure/stop the shared client. Preserve the integration feature's isolated lifecycle harness so it does not spawn detached production helpers.
- [x] Route managed `with_model` operations through the shared manager when registered. Wrap automatic completion in that same full-operation lease; check model identity after grant.
- [x] Preserve existing direct-network and isolated-pool behavior; reject stale configuration snapshots after reload.
- [x] Run library tests, relevant native AI integration tests, and process regressions; commit this task.

## Task 3: Review, real GPU validation, installation

Files: docs validation report and user-facing AI configuration documentation.

- [x] Run formatting and default package tests, report any baseline/platform failures explicitly; run focused lifecycle and multi-process checks.
- [x] Request one fresh whole-change review and resolve substantive findings with regression tests.
- [x] Build Release, verify two real hx instances share the installed 9B server and survive closing the first; verify 9B → 27B → 9B with the installed models on isolated endpoints.
- [x] Back up/replace `I:/DEV/helix/target/release/hx.exe`, verify version/config/model paths and clean temporary processes. Document the required restart of existing old hx instances.

## Execution record

- Setup: user approved direct execution; current checkout reused on a dedicated branch, preserving existing build caches and installation path.
- Design implementation detail: old low-level pool remains as the integration harness backend; shipped default builds use the shared manager. All actual cross-process behavior is tested through the real hx executable.
