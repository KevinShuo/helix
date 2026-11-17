# AI commit progress — 2026-11-16

The commit workspace previously displayed a fixed message throughout a sequence
of model requests. Recommendation and message generation now report model
readiness, analysis batch/current total, successfully analyzed files, files needing
manual review, aggregation/generation, and final repository validation. Batch
totals increase when a rejected batch is split; failed attempts do not advance
the analyzed-file count. File analysis completion is distinct from final-result
completion. No token percentage or estimated completion time is invented.

The workspace polls a watch channel, its operation future and a one-second timer
inside its existing cancelable task. Total and current-stage seconds continue to
update during a silent provider response. Instance/serial/busy checks reject stale
UI updates. Ctrl-c, configuration changes and panel closure retain the existing
cancellation behavior; no independent heartbeat task survives cancellation.

The original handler entry points remain test-only wrappers around
`recommend_with_progress` and `generate_message_with_progress`; each new entry
point accepts a final `&(dyn Fn(String) + Sync)` callback. The analysis planner
derives progress from its actual queue, preserving provider-specific batch policy.

## Verification

The new native test
`ai_commit_shows_batch_progress_and_ticks_while_provider_is_silent` passed in
16.73 seconds. It exercises the actual commands, Git snapshot checks, HTTP
transport, event loop and terminal buffer: five changed files, two delayed
summary batches, aggregation, timer ticks while the provider is silent and
cancellation followed by another heartbeat interval. The Git index stays intact.

Initial red attempts encountered fixture timing failures, and a queued rebuild
observed the brief interval between adding the new handler entry points and
connecting the workspace call sites. Neither is treated as a meaningful red
reproduction of the missing-progress behavior. The successful native run is
recorded in `target/ai-commit-progress-green.log`.

The handler retry/manual-review regression
`ai_commit_progress_counts_validated_files_and_expands_retried_batches` passed.
The first handler run had 25 passing tests, one ignored test and one failure in
the concurrent budget feature's new
`ai_commit_deepseek_auto_accepts_full_scope_despite_local_limits` test: that build
contained its new test and the earlier local file-limit code. After the budget
integration, the combined `--lib ai` run passed **162 tests, zero failed, twelve
ignored**, including both the progress regression and that DeepSeek regression.
See `target/ai-provider-budgets-green.log`.

An independent read-only review found no material progress-related correctness
or regression issues. It checked batch/retry totals, validated-summary counts,
manual exclusions, the silent-provider heartbeat, stage timers, and cancellation
and closure guards. Formatting of the changed Rust files and `git diff --check`
passed. Compiler warnings in the broader shared changes and the existing native
helper's unused `line_feed_handling` field remain recorded in the logs.

The native suite finished with **14 passed, one failed, one ignored** in 214.13
seconds. The new progress regression passed again within this suite. The original
eight-second deadline failed in
`ai_commit_recommendation_requires_explicit_stage_before_generating_message`
while the workspace displayed final repository validation. This test also timed
out in the earlier file-limit validation report. An unchanged isolated retry
also failed at the same stage (18.64 seconds for the complete test). Both captured
screens show one staged file and `AI 校验仓库状态并整理结果 · 已用 4s · 本步 1s`.
The original wait spans staging and automatic generation, including multiple
Git snapshots; the screenshots show generation has reached final snapshot
validation, rather than a model request waiting for a reply. This is a remaining
timing limitation, not a green suite. No production code, assertions or deadlines
were changed to hide it. Logs: `target/ai-commit-progress-native-suite.log` and
`target/ai-commit-progress-native-retry.log`.

## Installation coordination

The concurrent provider-budget chat owns the final combined release build and
installation. This progress task does not move or replace `hx.exe`, modify user
configuration, stop editor processes, or commit other chats' changes. The budget
chat received the stable progress interfaces and passing native-test evidence
under the user's verified cross-chat coordination authorization.

The combined release build and installation are now complete. The release build
succeeded in 4 minutes 20 seconds. PATH resolves to
`C:/dev/helix/target/release/hx.exe`, version `helix 25.07.1 (bc354cca)`, SHA-256
`576704683b03d71d098148a5350703aa803eaed3201d605a2cfbcd17e40a33e1`.
This progress task independently verified the installed binary hash, current
handler/workspace source hashes against the build manifest, and embedded progress
labels in the PE image. The budget chat also verified all 22 build-source hashes
and two configuration-loading health checks. Installation manifests are
`target/ai-provider-budgets-installed.json` and `target/release/BUILDINFO.json`.
The final combined budget/AI run passed 166 tests with 12 ignored, superseding
the earlier 162-test run; its evidence is in
`target/ai-provider-budgets-final-green.log`. The native timeout limitation above
remains recorded. Restart Helix to load the newly installed executable; existing
editor sessions were preserved.
