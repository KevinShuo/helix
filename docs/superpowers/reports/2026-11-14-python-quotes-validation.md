# Python quote pairing validation — 2026-11-14

## Change

Python string prefixes may precede automatically paired single and double quotes.
The insert command passes the language at each cursor, including embedded
languages, into the pairing hook. Prefixes must start at an identifier boundary;
the boundary uses Unicode XID continuation rules. Existing next-character guards,
closing-quote skipping, multi-cursor behavior, and disabled auto-pairs are retained.

The fork's existing application version remains 25.07.1; the release is identified
by its feature/date tag and the source revision in `hx --version` and BUILDINFO.

## Regression verification

- Before implementation, all four initial Python-prefix integration tests failed.
  The closing-quote test reproduced `r"""` instead of `r""`.
- Full auto-pair integration run after the initial fix: 67 passed, 0 failed.
- Review found a combining-mark boundary regression. The new core test failed
  before replacing the editor's ordinary word-character check with
  `unicode_ident::is_xid_continue`.
- Final prefix integration run: 6 passed, 0 failed. Covers single/double quotes,
  all supported prefix case combinations, closing quotes, identifier boundaries,
  following words, multiple cursors, non-Python buffers, and disabled auto-pairs.
- Final workspace run includes the passing Unicode regression for combining
  marks, middle dots, and connector punctuation.
- Independent code review after the boundary correction: no actionable findings.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

## Commands and environment

Tests use `HELIX_RUNTIME=I:\DEV\helix\runtime`. Builds use the existing grammar
DLLs with `HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1`.

```text
cargo test -p helix-term --features integration --test integration test::auto_pairs:: -- --test-threads=1
cargo test -p helix-term --features integration --test integration string_prefix -j 1 -- --test-threads=1
cargo test --workspace -j 1 -- --skip test_normalize_path --skip git::test::symlink
```

The first command preceded the final Unicode-only correction. The second and
third commands were run against the final implementation. The entire terminal
integration suite outside auto-pairing was not rerun for this patch.

Three unfiltered workspace cases fail with Windows privilege error 1314 and are
excluded by the final workspace command:

- `helix-stdx`: `test_normalize_path`.
- `helix-vcs`: `git::test::symlink`.
- `helix-vcs`: `git::test::symlink_to_git_repo`.

Initial default-concurrency builds encountered an automatic grammar build-script
BTree invariant abort and rustc access violations. Using cached grammars and one
build job completed successfully without changing repository compiler settings.
The existing integration helper emits a dead-code warning for
`TestCase::line_feed_handling`.

A prepublication rerun encountered a stack overflow in the unrelated
`helix-view` folding test `language_change_and_view_removal_clear_state` under
default test concurrency. The publication check reruns the workspace serially
with an 8 MiB test-thread stack (`RUST_MIN_STACK=8388608`):

```text
cargo test --workspace --locked -j 1 -- --test-threads=1 --skip test_normalize_path --skip git::test::symlink
```

## Release procedure

The release build uses:

```text
HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1 cargo build --release --locked --jobs 1 -p helix-term --bin hx
```

The archive contains tracked runtime files plus 302 cached Windows grammar DLLs
from the installed runtime. Packaging checks the x64 PE header and source revision,
verifies ZIP integrity and extracted-file SHA256 hashes, and runs isolated parser
and query health checks for Python, C++, Rust, and Go. Per-build results, hashes,
and source revision are recorded in the release's BUILDINFO and local packaging
metadata. Existing running editor processes are retained during local replacement.
