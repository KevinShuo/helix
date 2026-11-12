# Function folding validation — 2026-11-12

## Source and scope

- Pulled `KevinShuo/helix` master with a fast-forward to
  `9cc212a80be769330887c95464ecb42fec8e6e97` before development.
- Implemented on `codex/code-folding`, following
  [the design](specs/2026-11-12-code-folding-design.md) and
  [implementation plan](plans/2026-11-12-code-folding.md).
- Partially adapted formatter/movement/UI work by A2ureIm4la from
  [upstream PR #16453](https://github.com/helix-editor/helix/pull/16453),
  commit `0cfe8f9bae685e88a631ead0b4880b6ac93afb61`, under MPL-2.0.
  The PR remained open when checked. Function queries, conservative edit
  invalidation and visible-source highlighting replace its generic block logic.
- Four pre-existing local AI-completion edits were preserved unchanged. They
  are outside this change. Backup patches are in ignored
  `target/folding-local-backup/`.

## Behavior covered

Python/C/C++ functions and methods fold to `…`. Decorators, complete multiline
signatures, constructor initializers, C/C++ closing braces and their trailing
text remain visible. Empty/single-line functions, incomplete boundaries, lambdas
and arbitrary blocks are excluded. Candidates come from cached syntax queries,
with no indentation fallback.

The five commands are `toggle_fold`, `fold`, `unfold`, `fold_all`, and
`unfold_all`, mapped to `za`, `zf`, `zo`, `zM`, and `zR` (also in sticky `Z`).
The optional `fold` gutter shows configurable `▾`/`▸` symbols and supports clicks.

Tests cover nested restoration, independent splits, buffer switching, cursor
exclusion/relocation, search and selection-transaction reveal, editing through
another view, undo/redo, stale syntax, language changes, view cleanup, Unicode,
CRLF, EOF, wrapping, coordinates, hidden annotations, projected declaration
hints, canceled AI previews, and Git dock/fullscreen/resize behavior. Copying,
editing and saving continue to use the real source text.

## Automated checks

Commands were run from `C:\dev\helix` on Windows x64. Term builds use the
process-local `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1` after a normal build attempted
to fetch grammars and failed for unrelated Hare/SLisp/Metamath repositories with
`CRYPT_E_REVOCATION_OFFLINE`. No TLS checks were disabled. The cached Python/C++
grammars are exercised by real syntax and terminal integration tests; this is
not a claim that every grammar was freshly fetched and rebuilt.

| Check | Result |
| --- | --- |
| `cargo test` | 42 term library and 2 doc tests passed; 1 doc test ignored. Integration tests require the separate feature. |
| `cargo test -p helix-core -p helix-view` | 297 tests passed across libraries, integration targets and doc tests; 4 ignored (including the separately run measurement). |
| `cargo test -p helix-term --features integration --test integration folding -- --test-threads=2` | 7 passed, including the Git compatibility regression; 213 filtered out. |
| `cargo check -p helix-term --no-default-features` | Passed; existing unused-parameter warnings in disabled VCS branches. |
| `cargo build -p helix-term --locked` | Passed. |
| Scoped `rustfmt --check` and `git diff --check` | Passed. |
| `cargo fmt --all -- --check` | Existing unrelated formatting difference at `helix-core/src/indent.rs:903`; left unchanged. |

The complete editor integration run (`cargo test -p helix-term --features
integration --profile integration --test integration -- --test-threads=2`)
finished in 584.28 seconds: **212 passed, 5 failed, 3 ignored**. Git Bash was
placed first on the test process PATH only. The failures were:

- `test_open_below_at_block_end_stays_in_block`: the same C++ indentation
  assertion reproduces with the pre-folding integration executable built on
  November 11. This source area is unchanged by folding.
- Three `test_symlink_write*` cases: Windows privilege error 1314.
- `git_tools_native_dock_fullscreen_and_diff_flow`: a five-second deadline
  elapsed during the loaded run. The identical compiled test passed when rerun
  alone in 6.18 seconds total. Other Git tests, including the folding regression,
  passed in the full run.

The previously reported injection-motion and Bash formatting tests passed in
this run. No new folding failure remains; the full suite is not claimed green.
Logs are in ignored `target/folding-*.log` files.

## Native terminal acceptance

Ran the built debug `hx.exe` in a Windows ConPTY terminal with temporary
Python/C++ files and an opt-in fold gutter. Verified decorators, multiline
signatures, Chinese/emoji source, closing braces and a trailing comment; `za`,
`zM`, vertical motion past a hidden body, search reveal, buffer return and
`:reload` clearing fold state. The process exited normally.

Saving while folded using `:write --no-format` preserved both SHA-256 hashes:

- Python: `BB22118B0E39B14E0A7FD860CABDAAD0A7E8A77CEBBF93825B38B6AD1641D325`
- C++: `F23EADCF405BBF198117075561F94C824ACD046D7E93F7AF9EA681F19A1B0C78`

The first ordinary Python write invoked the user's configured Ruff formatter,
which added a module-level blank line. Restored the fixture and disabled
formatting for the byte-invariance check. This normal save behavior is separate
from folding. Raw terminal output is saved in ignored
`target/folding-terminal-smoke.json`.

## Fixed-size layout measurement

Ran the ignored `folding_large_body_layout_measurement` test explicitly:

```powershell
cargo test -p helix-core --test folding folding_large_body_layout_measurement -- --ignored --nocapture
```

Input: one C++ function with 50,000 body lines plus a following declaration,
500,023 source characters. One unoptimized local run measured:

| Layout | Emitted graphemes | Time | Collected output vector capacity |
| --- | ---: | ---: | ---: |
| Expanded | 500,024 | 206,924 µs | 37,748,736 bytes |
| Folded | 24 | 72 µs | 2,304 bytes |

The vector capacity measures the test's collected output, **not total process
memory or syntax-tree memory**. Timings are a single-machine diagnostic, not a
general benchmark claim. A separate 3,000-line terminal integration verifies
that following-source syntax colors remain identical and visible highlight
ranges exclude the hidden body.

## Independent review

A read-only reviewer examined the whole folding change. The review found one
important boundary bug: moving down from a folded Python function at EOF without
a trailing newline could loop forever. A five-second timeout reproduced it.
The movement loop now bounds the next line before converting it to a character
position; the regression passes. No additional concrete production defects
were identified in the final review.

## Release and local installation

The user subsequently requested a GitHub Release, Git push, removal of the Scoop
Helix package and adding the compiled directory to the user PATH. Target release
tag: `code-folding-2026.11.12`; target executable directory:
`C:\dev\helix\target\release`. Publication and rollout results will be recorded
after verification.
