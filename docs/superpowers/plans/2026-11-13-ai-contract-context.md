# Preserve distant function contracts in AI completion context

Continue the user's authorized conservative-quality work in the existing checkout.
The prior preference is direct implementation without repeated design questions.

Observed gap: `source_notes` restores distant signatures and imports but discards
the function's docstring and directly attached comments. The nearby prefix may
therefore retain parameter names while losing the behavior the function promises.

- [x] Write failing source-context regressions for distant Python docstrings and
  directly attached C++/Rust comments, including CRLF/Unicode and exclusions for
  nearby, unrelated, oversized, non-docstring and future text.
- [x] Extend existing bounded syntax-based notes; retain the 600-character source
  budget and existing prompt budget. Never scrape arbitrary surrounding comments.
- [x] Compare unchanged real-model requests on new executable long-function
  fixtures before/after. Keep the existing 30-case set as regression evidence.
- [x] Run normal/editor tests, independent review, optimized build and runtime
  checks. Install with a binary backup; preserve running editors and user servers.

No prompt-answer hardcoding, model change, global settings change, commit or push.
