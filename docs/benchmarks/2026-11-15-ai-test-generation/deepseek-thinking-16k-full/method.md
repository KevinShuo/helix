# DeepSeek thinking, 16K total output, full six-case rerun

This arm repeats all six fixed targets and all 18 defects from the previous
comparison. New arm: `deepseek-thinking-16k`; no automatic repair or retry.
Daily user configuration and the previous observations are unchanged.

Settings: configured `deepseek-flash`, thinking enabled, API default effort
(currently high), `max_tokens=16384` total reasoning plus answer, context
reservation 32768, 120-second HTTP timeout, production JSON response format.
The thinking toggle and budget use the documented Chat Completions parameters:
[API reference](https://api-docs.deepseek.com/api/create-chat-completion/),
[thinking guide](https://api-docs.deepseek.com/guides/thinking_mode/).
The requested temperature remains 0.0 as in the previous arm; the official
thinking guide says temperature does not control thinking-mode sampling.

The production feature currently accepts at most 8192 configured final tokens.
For this experiment only, the ignored Rust harness builds the production prompt
with its unchanged 4096 setting and then overrides the API request's total output
limit. A tested guard reserves the larger allowance against configured context.
This does not change the installed feature limit or the daily provider settings.

Every request's messages are compared to the saved non-thinking request **before
dispatch**. The context reservation increases to fit the larger output, but all
six prompt messages must remain identical. Snapshot files and hashes retain the
exact evaluated prompt/transport/validator/project/runner source.

`request.json` is the body before shared transport transformation. The DeepSeek
provider JSON enables thinking; `ai.rs` injects `thinking:{type:enabled}` before
HTTP dispatch. The unchanged transport file was added to this run's snapshot
after completion, with that capture timing and its older source modification
time recorded in manifest. All six responses also contain nonempty reasoning
metadata; reasoning trace text is not retained.

Production response validation, installed-vcpkg GTest/CMake and pytest execution,
the per-source UTF-8 fix, and the same three mutants per case are used. Generated
code is never hand-edited. Complete/rejected responses and compiler/test failures
are retained. Reasoning text is removed from saved responses; only its presence/
length metadata and token usage remain.

Compare strict acceptance, original-implementation execution, detected defects
(unexecuted cases do not contribute kills), generation median/total/per-case
latency, total completion tokens and truncation count. Inspect actual assertions,
linkage, macro syntax, state contracts and buffer-safety behavior separately.
Success on the fixed defects does not prove general test adequacy or memory safety.

This is one observation per case. Enabling thinking and increasing budget are a
combined intervention; six samples do not establish a general model ranking.
