# Conservative AI completion

The user requested the conservative approach directly on 2026-11-13: prefer
fewer unwanted suggestions and improve completion quality. Continue in the
existing checkout and preserve the running editor and model server.

## Design

Add an opt-in `conservative` setting, enabled in the user's normal configuration.
This keeps existing insertion/multiline behavior compatible when disabled.
Conservative requests use the existing current-line strategy even if insertion
mode was configured: preserve the exact typed prefix and existing suffix.

Do not request a completion on blank/comment-only lines, after statement/block
terminators, or for a single bare identifier character. These are deliberately
conservative heuristics, not estimates of model confidence. Keep requests after
assignments, member access, arguments and meaningful partial identifiers.
Also avoid extending expression-like tokens immediately before an existing
semicolon. A live probe showed the model unnecessarily changed `return value;`
into `return value > 0;` despite stronger abstention instructions. This gate
deliberately loses some partial-identifier completions at that boundary.

Ask for the shortest supported continuation, prefer existing names and patterns,
and permit an empty response when context does not distinguish alternatives.
Use deterministic sampling and limit generated output to 64 tokens (or the
user's smaller limit). Reject truncated/refused responses, multi-line output,
insertions over 160 characters and an extra statement following a top-level
semicolon. Do not truncate a rejected suggestion into apparently valid code.
Keep strings, nested calls and for-loop semicolons valid.

Do not switch models, download weights, send more project files, or change
ordinary completion/acceptance/undo behavior. This cannot prove semantic
correctness; live probes must be reported as a small sample, not an accuracy
guarantee.

## Verification and rollout

1. Observe the configuration regression fail, then add the option.
2. Observe request/response regressions fail, then implement the policy.
3. Cover skipped requests and accepted previews in the real editor harness.
4. Run the normal project tests, AI integration/configuration tests, formatting
   and release build; compare fixed local-model fixtures before and after.
5. Preserve the running executable before staging the new binary. Back up the
   user config before enabling the option and a slightly longer typing debounce.
   Existing sessions stay open; a restart loads the new executable.
