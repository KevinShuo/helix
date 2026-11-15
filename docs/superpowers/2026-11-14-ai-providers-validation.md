# Unified AI providers validation

## Scope

Independent local/online provider selection for inline completion, function
documentation and commit assistance. Legacy connection fields remain supported.
The ready-to-edit example is `contrib/config-ai-providers.toml`.

## Regression evidence

- Initial unified configuration test failed on the unsupported `ai` section.
- DeepSeek wire-format test failed because the `thinking` field was absent;
  HTTP error test failed because status-only errors were not actionable.
- Project provider override failed because the ordinary merge depth replaced
  the provider table and lost its endpoint.
- Independent review found unvalidated runtime config updates, continued commit
  analysis after config changes and local listener aliases bypassing conflict
  checks. Dedicated integration/view regressions reproduced all three before fixes.
- Runtime updates now validate before publishing, config changes cancel commit
  analysis while preserving confirmed Git writes/drafts, and conflicts compare
  parsed hosts/effective ports (including managed/existing-server combinations).

## Automated checks

- `cargo test`: 127 unit tests and 2 doctests passed; 4 opt-in unit tests and
  1 pre-existing ignored doctest were skipped.
- `cargo test -p helix-view --test ai_providers --test ai_completion --test ai_doc`:
  16 tests passed, including the shipped TOML example and all provider routing,
  legacy fallback, conflict and configuration tests.
- `cargo test -p helix-term --features integration --test integration ai_`:
  57 tests passed; 7 opt-in live-model tests skipped. All three named-provider
  command paths and the two runtime configuration regressions passed.
- `rustfmt --check --edition 2021 --config skip_children=true` on changed/new
  Rust files: passed. `git diff --check`: passed.

Independent read-only review covered precedence, runtime ownership, invalid
updates, cancellation and error sanitization. Its three actionable findings were
fixed with regressions observed failing and then passing. Existing unused-variable
warnings in the standalone non-Git view build and the integration helper's unused
field remain unchanged.

No real online API or paid model requests were made. External HTTP behavior is
tested with local fixtures, including an isolated subprocess for system-proxy
discovery and synthetic bearer authentication. Explicit live-model tests remain
opt-in. One intermediate default test build exhausted compiler memory; serial
compilation completed successfully afterward.
