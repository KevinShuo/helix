# Local AI inline completion validation

Original implementation environment: Windows, Rust 1.90.0, Junie Local Qwen3.8
27B Q5 model.
Implementation commit: `06d76288e`. The results below record local development
and validation on `codex/ai-inline-completion`, before publication to the
user-owned fork.

## Qwen2.5-Coder 7B configuration follow-up

The launcher now defaults to the user's downloaded
`Qwen2.5-Coder-7B-Instruct-Q5_K_M.gguf` (5,444,831,200 bytes), a 4096-token
context, all GPU layers and F16 KV cache. Server logs confirmed 29/29 layers
offloaded to the RTX 4090. The previous Q4 KV settings produced empty or
malformed code; changing only the cache type to F16 produced `sum(items)` and
`return a + b` for the same prompts. Model weights remain Q5_K_M.

The generated session config uses a 200 ms debounce, 5-second timeout, 48 output
tokens, 3000 prefix characters and 500 suffix characters. Existing user settings
and key bindings were compared as parsed TOML and preserved exactly; the
original config's SHA-256 was unchanged after the session. File arguments with
spaces were preserved, and the model process stopped when the session ended.
The existing gateway option still defaults to its original 27B model.

Fresh verification passed:

- PowerShell parsing, changed-file rustfmt and `git diff --check`.
- All 18 AI editor integration tests; the opt-in live test passed separately
  with `HELIX_TEST_AI_MODEL` set to the new 7B alias.
- Live editor preview was `sum(items)` and Shift+Right inserted it. The test's
  input-through-acceptance time was 1.46 seconds, including test event-loop
  waits; this is not an isolated inference measurement.
- A cold server start followed by `-Check` returned `return a + b` in 0.08 seconds
  of HTTP request time. Subsequent short synthetic requests using the editor's
  prompt returned `sum(items)` in 49 ms and `return a + b` in 51 ms. These times
  exclude model loading, typing debounce and rendering, and do not predict
  performance for longer contexts or suggestions.

At this initial 7B configuration stage, only the launcher, documentation and
live-test model selection changed. The subsequent boundary repair below requires
the rebuilt editor.

## C++ insertion boundary repair

The user's `while (smallestCarOfNumber...)` screenshot exposed a missing case:
the Instruct model could return an assignment or an entire statement when asked
for only an insertion. Live probes using the actual header reproduced the
problem. Raw FIM and alternate hole prompts with this Instruct checkpoint did
not reliably respect the suffix either.

The 7B launcher now enables `prompt-mode = "line"`. The model completes the
current line; the client requires the exact typed prefix and extracts only the
gap before the existing suffix. Suffix candidates inside an unmatched nested
call or string are rejected. Later generated lines are discarded, and LF/CRLF
code fences are normalized consistently. The default `insertion` mode retains
the original multiline behavior and remains the gateway/27B default.

Fresh checks for the repair:

- 21 `helix-term` library tests passed, including the new boundary and CRLF
  regressions. The tests failed on the old behavior before implementation.
- Both AI configuration tests passed, including insertion-mode compatibility.
- 19 AI editor integration tests passed, covering existing popup completion and
  a new complete-line-to-gap acceptance case. Two opt-in live tests passed
  separately against the running 7B server.
- The actual C++ file was read into a disposable editor fixture. The preview
  ` <= numberOfCar` was accepted without repeating the identifier or altering
  the right-hand suffix. The original source file was never opened for writing.
- An independent reviewer found a rightmost-suffix ambiguity and fenced CRLF
  mismatch. New regressions reproduced both; the final relevant suites passed
  after the fixes.
- Release build passed in 1m17s. The replacement is staged at
  `target/release/hx.exe`; the running image was renamed and preserved so the
  user's session did not close. The launcher-generated line-mode TOML preserved
  all original preferences, the original config hash stayed unchanged, and the
  new release's C++ health checks passed. Both existing editor and model server
  processes remained alive. Restarting through the launcher activates the fix.

The C++ preview above is **not** proof of the correct algorithmic condition: it
does not recover the source's original `== nextCarToOutput` comparison. This
repair fixes insertion boundaries; the 7B model can still make incorrect code
guesses. Conservative checks may also omit otherwise valid ambiguous fragments.
Line mode deliberately suggests only the current line.

## Sorting completion quality follow-up

The line prompt previously moved the current line out of its original location:
it sent the preceding code, then the following code, then the typed line as a
separate quoted fragment. A controlled local-model experiment kept the source
in order with `<CURSOR>` in the incomplete line. The model, system instruction,
sampling temperature (0.1), output limit (48 tokens), and seed (42) stayed fixed.

Ten completion holes covered insertion sort, bubble sort, quicksort, heap sort,
and descending insertion sort. Every fixture already fit the launcher's
3000-character prefix / 500-character suffix window (largest prefix: 640;
largest suffix: 462). Raising those limits to 12000/4000 therefore sent exactly
the same code in this experiment.

| Prompt | Programs passing all inputs |
| --- | --- |
| Previous line prompt, 3000/500 | 8/10 |
| Previous line prompt, 12000/4000 | 8/10 |
| Source kept in order with a cursor marker | 10/10 |

Each completed program was compiled as C++17 and run on 162 arrays: empty,
singleton, ascending, descending, duplicate and negative values, plus seeded
random inputs. Reference implementations passed those same checks first.
Boundary canaries and a recursion budget detected corrupt writes and recursion
that did not shrink its input. The two regressions were:

- Insertion sort's shift: `a[j + 1] = key;` became `a[j + 1] = a[j];`.
- Quicksort's right recursion: `quick_sort(a, low, high);` became
  `quick_sort(a, pivotIndex + 1, high);`.

The editor now uses the tested source ordering, and the launcher's `-Check`
prompt follows it. A regression captures the real HTTP request and requires
the incomplete assignment to remain inside its loop, before the final key
assignment. It failed with the previous prompt and passed with the change.
Context limits and insertion-boundary handling are unchanged.

Fresh verification for this change:

- 22 `helix-term` library tests and 19 AI editor integration tests passed.
- The opt-in live Python editor test displayed `sum(items)` and accepted it
  with Shift+Right; the launcher's updated `-Check` returned the same insertion.
- The optimized release build passed in 1m16s. The replacement executable is
  `target/release/hx.exe`; the running editor image was preserved before the
  build so existing sessions were not closed.
- Changed-file rustfmt, PowerShell parsing, and `git diff --check` passed.
  The integration build still emits the existing unused `line_feed_handling`
  test-helper warning. The unrelated broad-suite baseline failures described
  below were not rerun for this prompt-only change.

This is a small synthetic comparison, not a general accuracy estimate. It
shows that prompt organization can cause wrong code even when all relevant
source is present. Large functions and cross-file types can still need context
that this character-window implementation does not collect. No model switch or
larger context window was needed for these ten examples.

## Original 27B implementation results

- Release build passed: `cargo build --release --locked -p helix-term` with
  `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1`. Output: `target/release/hx.exe`, reporting
  `helix 25.07.1 (06d76288)`.
- 18 AI editor integration tests passed. Coverage includes grey rendering,
  cursor and multiline Unicode positioning, entire-suggestion acceptance,
  undo, ordinary popup Tab/Enter, empty acceptance, key remapping, dot repeat,
  macros, command sequences, old-response rejection, timeout and CRLF.
- 407 workspace library/integration tests passed with three tests requiring
  unavailable Windows symlink privileges excluded (see below).
- An opt-in real-model editor test passed: automatic preview `sum(items)`,
  document unchanged until Shift+Right, then insertion through the event loop.
  Time from input through acceptance was approximately 1.70 seconds in the final
  post-review check (2.43 seconds in the earlier check).
- Launcher validation passed: PowerShell parsing, preserving the user's
  original config, file arguments containing spaces, temporary-config cleanup,
  cold model startup, synthetic completion, and stopping its own model process.
  A cold-start check returned `return a + b` in 1.14 seconds of inference.
- Python release health checks passed for parser, highlights, text objects,
  indents, tags and rainbow queries.
- Changed Rust files pass rustfmt; `git diff --check` passes.

The broad editor integration run completed: **191 passed, 1 baseline failure,
1 optional live-model test ignored, 3 symlink tests excluded**. The baseline
failure is described below. The ignored live-model test was run separately and
passed. The full run took approximately 12 minutes on this Windows machine.

## Existing environment/baseline limitations

- Windows reports privilege error 1314 when creating symlinks. The workspace
  tests `helix-stdx::test_normalize_path`, `helix-vcs::git::test::symlink` and
  `helix-vcs::git::test::symlink_to_git_repo` could not run successfully.
  The editor tests `test_symlink_write`, `test_symlink_write_fail` and
  `test_symlink_write_relative` have the same restriction. No permission or
  operating-system security setting was changed.
- `test_open_below_at_block_end_stays_in_block` fails in an existing C++
  comment/indent case on this machine. It expects the newly opened line to be
  unindented; the actual line is indented four spaces. This was reproduced with
  the same failure using an isolated source archive of pre-feature commit
  `13bd6aa1a`. No indentation production code was changed for this feature.
- Repository-wide `cargo fmt --all -- --check` flags a pre-existing tuple
  formatting difference in `helix-core/src/indent.rs`. That unrelated file was
  left unchanged; the files changed by this feature pass formatting checks.
- 298 grammar libraries were built. Sources for `hare`, `slisp` and `metamath`
  could not be fetched because their upstream host's certificate revocation
  check failed in this environment. TLS verification was not disabled.
- The first broad integration run with default parallelism did not finish
  promptly. The final run uses two test threads and Git Bash on the child
  process PATH, avoiding Windows' WSL `bash` shim for formatter fixtures.

## Review and fixes

A fresh whole-change reviewer found three important issues, all fixed with
regression tests that failed before the fixes and passed afterwards:

1. Closing the final view must not access a removed current document.
2. A multi-key acceptance mapping must remove pending prefix keys from repeat
   recording before recording the accepted literal text.
3. Acceptance must execute in order inside a command-sequence binding; repeat
   and macros preserve companion movement/mode commands as well as literal text.

There were no critical issues, deferred minor issues or declined judgments.

## Implementation decisions

- Reused the initially clean checkout on a local feature branch rather than
  creating another worktree. This keeps the requested project directly usable;
  the tradeoff is less filesystem isolation from concurrent user edits.
- Implemented the tightly coupled editor changes in one execution context and
  used one fresh reviewer for the whole feature. Integration and regression
  tests cover the interfaces where a late review could otherwise miss a bug.
- Used the repository's existing grammar-download build opt-out after upstream
  download failures, then built available grammars. The limitation is the three
  unavailable languages above, rather than weakened TLS verification.
- Used a separate loopback server on port 19240 with 60 GPU layers, an 8192-token
  context and Q4 KV cache. The saved Junie gateway's full-offload settings did not
  fit the available VRAM. This leaves Junie's settings intact; compared with its
  normal setup the completion server has a shorter context and CPU-offloaded
  layers. `-UseGateway` remains available when the saved setup fits in memory.
- Kept default editor behavior opt-in: the launcher adds AI settings to a
  temporary copy of the normal config. Launching a different installed `hx`
  does not enable this feature. Use `contrib/helix-junie.ps1` for this build.
