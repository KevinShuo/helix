# Inline completion boundary repair

> **For agentic workers:** Use superpowers:executing-plans for this repair. The existing local checkout and approved inline-completion behavior remain the scope.

**Goal:** Prevent the local 7B Instruct model from inserting a repeated identifier or an unrelated statement into the middle of the current line.

**Architecture:** Add an opt-in `prompt-mode = "line"` to the existing chat adapter. Ask for the completed current line, require the typed prefix to be preserved, and extract only the gap before the existing suffix. Enable this mode in the 7B launcher. Keep the default insertion mode and its multiline behavior compatible.

**Tech Stack:** Rust, serde/TOML, reqwest, existing editor integration harness, PowerShell launcher.

**Spec:** The existing inline completion requirements plus the user's screenshot and `stack_track.hpp` report in this conversation. This document records the small repair design and its implementation plan.

## Evidence and approach

The current insertion prompt reproduced unrelated `if` blocks, assignments copied from later code, and repeated identifiers. Raw FIM with this downloaded Instruct model also generated past the existing suffix. A current-line prompt preserved the typed code in live C++ and Python probes. Selecting the line strategy is an implementation decision for this model; it does not guarantee correct algorithmic guesses.

## Constraints and review focus

- Keep ordinary completion, full-suggestion Shift+Right acceptance, undo and cancellation intact.
- Keep Instruct chat compatibility; do not download a replacement model or change Junie's saved settings.
- Preserve partial identifiers, indentation, Unicode and CRLF at the cursor boundary.
- Reject a response that changes the typed prefix or omits a nonblank existing line suffix.
- Only the current line is suggested in line mode; ignore generated later lines and trailing text beyond the existing suffix.
- The user's editor and model server are running. Do not close them or edit the C++ source.
- Build and stage the replacement editor without pushing Git changes.

Publication follow-up (2026-11-11): after validation, the user explicitly
authorized pushing the implementation and repair to `KevinShuo/helix` on `master`.

## Steps

- [x] Add a failing configuration test for `prompt-mode = "line"`; implement the opt-in enum with `insertion` as the default.
- [x] Add failing HTTP client regressions for the screenshot's repeated-prefix case, partial words, suffix boundaries, multiline spillover, indentation and Unicode/CRLF. Implement current-line prompting and strict extraction.
- [x] Enable line mode in the 7B launcher, update the diagnostic check and documentation, and add an opt-in live C++ editor check.
- [x] Run relevant client/config/editor tests and live-model preview/acceptance. Build and stage a fresh release binary, preserving the running session. Record measured results and remaining model-quality limitations.
