# Grounded AI completion validation

2026-11-13, continued directly in the existing Windows checkout after the user
requested further AI completion quality improvements. The earlier conservative
changes remain in place. No model switch, download or additional dependencies.

## What changed

- Recover missing enclosing function/class/impl signatures and current-file
  imports from the existing syntax tree. Preserve source order around the cursor;
  declarations are separate labeled context, not inserted into the source.
- For member access, ask the first configured connected completion language
  server after debounce. Wait at most 200ms using a cancellable request and
  fall back without interrupting editing.
- Export labels/type details only. Complete usable member lists of at most
  128 items can reject an invented first member name. Unknown, incomplete and
  oversized lists remain hints. Enum/interface/type-parameter members are valid.
- Supplementary source/LSP notes cap at 600 characters each, 1200 combined,
  replacing older prefix text within `prefix-chars`. The typed current line
  takes priority. Missing grammars and small budgets fall back to existing text.
- Conservative-disabled behavior, ordinary completion menus, acceptance, undo,
  document/version checks and the managed local-model lifecycle are preserved.

## Fresh verification

| Check | Result |
| --- | --- |
| Distant source-context editor regression | Observed failure before implementation (`enclosing parameters were omitted`), passes afterward |
| Invented-member HTTP regression | Observed acceptance of `imagined(id)` before validation, passes afterward |
| Mixed enum/function authority regression | Observed false rejection before fix, passes afterward |
| `cargo test --locked --jobs 2` | 65 library and 2 doc tests pass; two opt-in live tests and one doc example ignored |
| AI integration group, integration profile | 24 pass, 4 opt-in tests ignored; existing unused test-helper field warning |
| Opt-in actual clangd/editor test | Pass: captured outgoing HTTP prompt includes real candidate notes and `payload_size`; controlled suggestion accepted |
| Actual local-model tests | Both previous conservative comparison and new grounded-context comparison pass |
| Independent read-only review | Mixed-kind authority issue was reproduced/fixed; final follow-up found no remaining material findings |
| `cargo fmt --all -- --check` and `git diff --check` | Pass |
| Optimized release build | Pass, 3m 47s |
| Installed Python/C++ parser and query health | Pass |

Logs: `target/ai-context-{standard,integration,clangd,live}.log`.
The clangd test uses a controlled HTTP model reply to prove collection and
forwarding, while the separate comparison uses the real Rust client and local
model. This is the normal project suite plus relevant editor checks, not a
complete workspace/all-editor-suite run.

## Paired local-model comparison

Same running `Qwen2.5-Coder-7B-Instruct-Q6_K`, temperature zero, 48 output tokens,
3000/500 prefix/suffix budgets and conservative checks. Each fixture places
the needed declaration outside the old prefix window. The new path uses the
actual syntax-tree note collector; member fixtures supply representative LSP
items. One paired final run:

| Fixture | Previous context | Grounded context |
| --- | --- | --- |
| C++ long-function array parameter | `values[i]` (undefined) | `amounts[i]` |
| Python long-function collection parameter | `items` (undefined) | `cart_entries` |
| Packet field | `category` (nonexistent) | `category_code` |
| Store method/argument | `record` (nonexistent) | `lookup_by_id(record_id)` |
| Imported path alias | `"/config.json"` (ignores base) | `FilePath(base) / "config.json"` |

All five grounded outputs match the fixture's expected continuation. These
synthetic examples demonstrate the targeted missing-context failure; they are
not a general accuracy estimate. The member guard checks the first member name,
not the validity of the whole expression or its arguments. Limited context,
unsupported syntax grammars and slow/unavailable language servers remain limits.

## Installation

Installed executable: `target/release/hx.exe`. The preceding executable is
preserved as `target/release/hx-before-grounded-ai-context-20261113-142516.exe`.
Build and installed hash/backup records are in `target/ai-context-release-build.log`
and `target/ai-context-installed.json`. The existing `conservative = true` setting
enables these improvements; no additional user configuration is needed. The
original editor PID 50644 and model-server PID 68944 remained running. Restart
Helix to load the replacement executable. No commit, push or public release is
part of this request.
