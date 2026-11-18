# Portable language paths

**Goal:** Let the same Windows languages.toml resolve the current user's directories at load time, including LSP options and formatter arguments.

**Design:** Expand explicit `${env:NAME}` references after merging trusted language configuration and before deserialization. Limit expansion to language-server settings and language formatters. Use the process environment, not sibling `environment` entries. Preserve ordinary `${INPUT}`, snippets, and `%{buffer_name}`. Substitute once after TOML parsing so spaces, backslashes, and quotes remain data. Reject missing variables or malformed references with the variable name, without printing values. `$${env:NAME}` escapes a literal reference.

**Approaches:** Shell wrappers can expand launch arguments but cannot consistently expand nested LSP options. Generating a machine-specific TOML would leave absolute paths on disk. Native expansion handles both and keeps the shared file portable; use it for this request.

**Constraints:** No shell execution, new dependencies, unrelated language changes, or automatic tool installation. Existing workspace trust rules apply before expansion. Reuse the current checkout on a task branch. Preserve the user's other settings and make a backup before activation.

## Work

- [x] Add regression tests covering LSP command/args/environment, nested config arrays, formatters, two different user directories, missing/malformed variables, escaped/literal references, and nonrecursive replacement.
- [x] Implement the expansion in helix-loader/src/config.rs and document the syntax in book/src/languages.md.
- [x] Run loader tests and formatting checks; build the release hx binary.
- [x] Convert all pasted and active user-directory paths to Windows environment references; export a reusable copy and back up the active configuration.
- [x] Verify the built hx loads the portable configuration and confirm FetchContent completions using the resolved saved settings.

## Review focus

- TOML strings with backslashes, Unicode, spaces, or quotes must remain valid.
- Existing shell, CMake, and formatter placeholders must remain unchanged.
- Missing variables must not silently form an incorrect path.
- Untrusted workspace configuration must remain excluded.
- A second machine must resolve its own environment, without a compiled-in user path.

## Validation

- `cargo test -p helix-loader --profile integration --offline`: all 15 tests pass.
- Formatter runtime regression passes using the actual `expansion::expand` path.
  Rust 1.90 initially crashed compiling the integration library with incremental
  optimization; retry passed with `CARGO_INCREMENTAL=0`, one job, and the
  helix-term package's integration opt-level/debug set to zero.
- `cargo build --release --locked --offline -p helix-term --bin hx -j 1`: passed.
- `rustfmt --check` on changed Rust files and `git diff --check`: passed.
- Real-binary comparison reproduced unresolved references in old hx and confirmed
  LSP/formatter command expansion and missing-variable errors in new hx.
- The saved portable configuration loads in hx health checks for CMake,
  TypeScript, Vue, and JavaScript. Direct LSP requests return FetchContent,
  ExternalProject, GoogleTest, and CMakePackageConfigHelpers completions.
- Independent review's formatter percent-escaping issue was reproduced by a
  failing test, fixed, and re-reviewed without remaining important findings.
- The active configuration and old executable were backed up. Running old hx
  processes were retained; new launches use the rebuilt executable.
