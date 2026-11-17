# Broader AI budgets validation — 2026-11-16

The user requested broader settings across features after a 71-file commit
recommendation exceeded the existing 4096-byte aggregation budget. The local
repository's metadata with maximum 160-character Chinese summaries was measured
at 43037 serialized UTF-8 bytes, including selectable IDs. A 49152-byte input
allowance fits this envelope inside 65536 context with 8192 output tokens and
the existing system/control overhead. This is a bound for this fixture, not a
claim that arbitrary 128-file inputs or model responses always fit.

## Source correction and tests

Shared HTTP timeout ceiling: 120000 → 600000 ms. Explicit final-output ceiling:
16384 tokens, shared across commit, documentation, merge and test generation.
Test generation previously rejected more than 8192; the other explicit clients
silently limited final output to 4096. Defaults, reasoning reservations,
context checks, complete-input handling, response-size bounds, cancellation and
explicit acceptance/Git-write requirements remain in place.

Five regressions failed on the prior implementation: long HTTP deadline,
commit output allowance, documentation output plus reasoning reservation,
merge output allowance, and test output allowance. The HTTP regression uses
Tokio's virtual clock after the real local server receives the HTTP request;
it proves a 180-second allowance survives 121 seconds and checks both the short
minimum and finite ten-minute maximum. Tokio `test-util` is a dev-only feature.

Final focused command passed **141 tests, 0 failures, 12 ignored**. Ignored tests
require model-evaluation settings or external compiler/GTest/pytest environments.
No existing test was weakened or filtered because of a failure. One independent
read-only review found no material issues. Formatting and whitespace checks passed.

```text
HELIX_RUNTIME=C:/dev/helix/runtime
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib generous_ -- --test-threads=1 --nocapture
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib ai -- --test-threads=1 --nocapture
cargo fmt --all -- --check
git diff --check
```

Logs: ignored `target/ai-generous-budgets-red.log` and
`target/ai-generous-budgets-green.log`.

## Real GPU/context verification

Read the 27B GGUF metadata: trained context 262144, 65 blocks and a full-attention
interval of four. On the local RTX 4090 (24564 MiB), actually loaded that model
with context 65536, 56 GPU layers, one slot and f16 K/V caches. `/props` reported
65536 context. A local JSON smoke request returned `{"ready":true}`. At readiness,
GPU usage was 22731 MiB with 1412 MiB free. This is the observed ready-state memory,
not a comprehensive peak measurement or a guarantee with other GPU workloads.

The probe used an isolated port, 19252. The current editor-owned 9B service was
stopped for GPU capacity; editor processes/buffers were preserved. The probe
terminated and waited for only its own child after successful verification, then
released its port and GPU memory. The recorded child exit code 1 reflects that
explicit termination, not a failed load or request. No model server was left
running by the probe. New editor startup/reload owns normal model services.

Evidence: ignored `target/ai-generous-context-probe-result.json` and its logs.

## Requested configuration

| Feature | Local context | Input / surrounding source | Final output | Request timeout |
| --- | --- | --- | --- | --- |
| Completion | 32768 | 16000 prefix / 4000 suffix characters | 64, or 256 for up to 12 predicted lines | 30000 ms |
| Documentation | 65536 | complete function, provider context bound | 8192 | 600000 ms |
| Tests | 65536 | complete target and available dependencies | 16384 | 600000 ms |
| Merge | 65536 | 49152 UTF-8 bytes | 8192 | 600000 ms |
| Commit | 65536 | 49152 UTF-8 bytes, 128 files | 8192 | 600000 ms |

Keep conservative completion mode and all existing feature/provider selections.
Both 27B providers use 56 GPU layers to make room for the larger cache; the
thinking provider has a separate 4096-token reasoning allowance. Startup timeouts
and test-run stage timeouts are 600000 ms. The inactive online provider and its
credentials are not changed. Configuration backup, installation hashes and final
health validation are recorded after installation below.

## Applied configuration and installed release

Applied the table above to `C:/Users/wangs/AppData/Roaming/helix/config.toml`.
Compared the complete parsed TOML against the original plus only the enumerated
budget changes; feature enablement, conservative mode, provider choices,
credentials and all unrelated settings are identical. Kept max-files at 128.
Backup: `config-before-ai-generous-budgets-20261116-114348.toml` in the same folder.

Built `cargo build --release --locked --jobs 2 -p helix-term` successfully in
3m 44s, reusing existing grammar assets. Installed
`C:/dev/helix/target/release/hx.exe`, version `helix 25.07.1 (bc354cca)`, SHA-256
`0d80b8da5b3118a91c7aa7bd2ef7b8c8c8b13419fc1a72d4f3005cb0fc0e66d6`.
The unchanged base revision in the version string does not identify the
uncommitted patch; current source-file hashes and the binary hash are recorded
in `target/ai-generous-budgets-installed.json` and `target/release/BUILDINFO.json`.
Verified the installed image contains the new 16384-token test-generator
diagnostic and not the old 8192-token diagnostic, and that both release/dependency
executable hashes match. The previous image is retained as
`target/release/hx-before-ai-generous-budgets-20261116-114206.exe`, with a matching
backup under `target/release/deps`.

Installed-image `--health clipboard` parsed the modified configuration without
error; `--health python` retained the parser, highlight, textobject, indent, tags
and rainbow queries. The PATH-resolved `hx --version` starts the installed image.
Both editor processes and buffers were left intact; they need restart to load
the new HTTP/output code and take ownership of new model services. No local
model server or private probe was left running by this task. No user-repository
staging, commits, remote provider requests or real 71-file recommendation were
performed as part of verification. Source changes remain uncommitted.
