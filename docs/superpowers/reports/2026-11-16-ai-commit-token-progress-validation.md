# Live generated-token counts — 2026-11-16

The AI commit workspace samples the currently managed local llama.cpp server's
`/slots` endpoint about once per second and displays `本次 token N` alongside
batch/file counts and the total/stage timers. The value is the active request's
actual `next_token.n_decoded`, including generated reasoning tokens if enabled;
it is not a byte/chunk/character estimate. Both the installed b11081 one-element
`next_token` array and the newer object form are supported.

Sampling only uses a managed llama.cpp model on a local HTTP loopback endpoint,
with one active slot 0. Unsupported/idle/ambiguous state or failed telemetry
displays `暂不可用`. The read-only client uses no API key or proxy, rejects
redirects, and bounds each probe to 500 ms and 64 KiB. Telemetry errors never
fail a generation request. Online-provider streaming/token accounting is outside
this change; those providers display unavailable rather than a fabricated count.

The telemetry future is polled inside the same scope as the generation future,
so cancellation and completion drop it. Phase tags discard probes that complete
after the batch changes. Token-only watch updates do not reset the stage clock.
The existing instance/serial/busy guards reject queued updates after cancellation.

## Verification

The initial HTTP regression failed on the empty implementation with expected
`Some(12)` versus `None`, recorded in `target/ai-commit-token-progress-red.log`.
The real HTTP fixture then verified actual 12-to-37 count changes, both response
forms, idle and multiple-slot rejection. Additional tests verify that unmanaged,
online and non-loopback models are not probed, and that a deliberately delayed
old-phase response is discarded and a canceled monitor sends no more requests.

The focused handler/UI module run passed **31 tests, zero failed, one ignored**;
the ignored case requires a real-model evaluation environment. See
`target/ai-commit-token-progress-regression.log`. Independent read-only review
found no material issues. Its coverage caveat is retained: the unit fixtures
exercise the counter and phase lifecycle, while the native regression checks the
rendered unavailable label and existing timers/cancellation; there is not a full
native fixture asserting increasing token values in the rendered workspace.

The targeted native regression
`ai_commit_shows_batch_progress_and_ticks_while_provider_is_silent` also passed
in 16.43 seconds, including the rendered unavailable label, independent timer,
batch progression and cancellation assertions. See
`target/ai-commit-token-progress-native.log`. The existing broader native suite's
previous timeout is retained in the earlier progress report; it was not rerun or
described as green here. Formatting and whitespace checks passed.

## Installed release

The optimized release build succeeded in 4 minutes 5 seconds and is installed at
`C:/dev/helix/target/release/hx.exe`, which PATH resolves to. Version remains
`helix 25.07.1 (bc354cca)` because the changes are uncommitted. Its SHA-256 is
`7376140d3ed0130a04a632b69f57dbf4bcd44e2e02c28f6dd14a3ab83ac59f18`.
All 24 build-source/manifest hashes match the compiled snapshot, and the token
label is present in the PE image. The user configuration is byte-identical to
the pre-install hash, preserving DeepSeek auto budgets and local settings.
Loading that configuration with the installed executable's clipboard health
command passed. The previous executable/dependency images are backed up; no
editor or model process was stopped.

Evidence: `target/ai-commit-token-progress-release-build.log`,
`target/ai-commit-token-progress-health.log`,
`target/ai-commit-token-progress-installed.json` and `target/release/BUILDINFO.json`.
Restart Helix to load the new counter. Existing sessions retain their mapped
older executable.
