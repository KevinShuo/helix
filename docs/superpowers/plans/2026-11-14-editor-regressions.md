# Editor regression fixes implementation plan

> **For agentic workers:** Use systematic-debugging and test-driven-development; independent indentation and Windows fixture investigations may run in parallel.

**Goal:** Resolve the five recorded editor test failures, verify the workspace, and deliver an installed Windows release.

**Architecture:** Investigate each failure against real editor behavior. Fix CRLF indentation at its source; make formatter fixtures independent of an installed shell; distinguish unavailable Windows symlink privileges from editor failures while retaining explicit capable-environment coverage.

**Tech Stack:** Rust, Tokio integration tests, Windows x64, GitHub releases.

**Spec:** User request in this conversation: fix the five failures, build release, merge/push master, publish, replace the current Helix.

## Global constraints

- Preserve user configuration, existing AI features, and untracked `.idea/`.
- Do not change machine symlink privileges or weaken file-content assertions.
- Use the installed grammar runtime for tests and serial compilation to avoid memory exhaustion.
- Merge, push, publish, and installation are explicitly authorized.

## Review focus

- LF and CRLF comments must produce the same indentation decisions.
- Formatter tests must execute a real subprocess and verify saved bytes after exit.
- Symlink cases must continue to exercise real file symlinks where supported.
- Missing Windows privilege must be reported as ignored rather than passed.
- Release binary, source commit, tag, runtime, and installed version must agree.

## Tasks

- [x] Reproduce the five failures and record root causes.
- [x] Fix indentation and verify focused LF/CRLF regressions.
- [x] Replace shell-dependent formatter fixtures and verify success/failure saves.
- [x] Make privileged symlink coverage explicit and runnable on capable Windows CI.
- [x] Run formatting, full workspace and full editor integration checks; obtain a fresh review.
- [ ] Commit, compile optimized release, package and verify isolated runtime health.
- [ ] Merge and push master, publish a new immutable tag and assets, verify remote hashes.
- [ ] Back up and replace the installed executable; verify version and unchanged configuration.
