# Broader local AI budgets

The user requested less restrictive settings across AI features after the
71-file commit recommendation exceeded the 4096-byte aggregation budget.

Implementation runs inline on the existing checkout, retaining earlier fixes.
Preserve review gates, complete-input handling, cancellation and explicit Git
writes. Keep max-files at the user's chosen 128.

- [x] Add regression tests for 16K explicit output allowances and HTTP requests
  that legitimately take more than 120 seconds, using Tokio's virtual clock.
- [x] Confirm existing hard caps fail those tests; raise explicit output ceiling
  to 16384 and shared HTTP timeout ceiling to 600000 ms. Keep finite limits and
  context accounting, including reasoning tokens.
- [x] Configure 9B context to 32768, 27B contexts to 65536, commit/merge input
  budgets to 49152 bytes, explicit request/run/startup timeouts to 600000 ms,
  doc/merge/commit output allowances to 8192 and test output allowance to 16384.
  Use 56 GPU layers for 27B to leave VRAM for its larger KV cache on the 24GB GPU.
  Enlarge completion prefix/suffix/prediction budgets within its conservative mode.
- [x] Verify focused tests, context envelopes and actual 27B loading before
  claiming the larger local context is usable. Back up configuration and binaries.
- [x] Build/install the new release and verify configuration parsing and hashes.
  Preserve editor buffers. Record actual validation and any limitations.

Evidence: `docs/superpowers/reports/2026-11-16-ai-generous-budgets-validation.md`.
One independent review found no material issues. Source changes remain uncommitted.
