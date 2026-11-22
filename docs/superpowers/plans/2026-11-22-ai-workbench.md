# Helix AI workbench implementation plan

**Goal** — Open actions and model settings from `:ai`, ask about selected code,
and preview code generated at the cursor with `Ctrl-\`.

**Architecture** — Keep the terminal panel as a caller of Helix's typed commands.
Store request state in the editor. Use the existing provider routing, task
cancellation and inline suggestion rendering.

## Implementation

- [x] Add `explain` and `generate` to the feature configuration in
  `helix-view/src/ai.rs`. Keep them off by default. Cover inherited and
  explicitly selected providers with `cargo test -p helix-view --test ai_providers`.
- [x] Register `:ai-explain`, `:ai-ask`, `:ai-explain-show` and
  `:ai-explain-cancel` in `helix-term/src/commands/typed.rs`. Capture the
  selection when starting a conversation. Exercise a follow-up question,
  reopening a result and cancellation while a reply is pending.
- [x] Register `:ai-generate` and `:ai-generate-cancel`, and bind `Ctrl-\`.
  Prompt for an instruction, capture the insertion position, and render the
  reply as a preview. Verify acceptance creates one undoable edit. Change the
  document before the reply arrives and verify the stale reply is ignored.
- [x] Extend `helix-term/src/ui/ai_settings.rs` with an actions view, a
  context-dependent initial selection and file/selection summary. Show inline
  completion's switch, the generation shortcut, active jobs, results and
  cancellation. Link disabled features to their settings and explain why a
  merge requires a conflict file. Preserve staged edits on Tab. Cover
  command dispatch, feature activation, applying settings and discarding a
  draft in the terminal integration tests.
- [x] Update `book/src/editor.md` with panel controls, configuration, command
  names, result reopening and inline preview keys. Check formatting and run
  the affected terminal tests with the `integration` feature enabled.

## Verification

The provider suite passed all 29 tests. The nine model-fixture integration
tests passed, including cancellation and stale-reply handling. A separate
live-model run explained a Python function, answered a follow-up, and produced
a two-line Python insertion that the test accepted. The existing session
settings regression passed. That test enters settings with `s`; the two Tab
transitions were reviewed in the panel event handler.

The release build succeeded. The AI integration suite passed 81 tests, with
12 opt-in or platform-specific cases skipped in that run. The local Qwen
smoke test was run separately. The broader terminal run passed 434 tests,
failed seven, and skipped 19. Five of the seven passed in isolated reruns.
Two local Git push fixtures needed Windows signal-pipe access outside the
sandbox. The compiler flood-output deadline and the declaration-cache
reopen deadline also expired with the pre-change integration binary.
