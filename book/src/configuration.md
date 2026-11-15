# Configuration

## Errors, warnings and compilation

Use `]q` / `[q` to visit the next/previous problem across files, or `Space E`
to search the problems list with source previews. Navigation includes errors and
warnings by default, preserves jump history (`Ctrl-o` to return), and reveals
folded targets. It stops at the end instead of wrapping. Existing `]d` / `[d`
and `Space d` / `Space D` retain their original LSP behavior.

Filters persist across these new commands and their key bindings:

```text
:problems --source lsp --severity error --scope workspace
:next-problem --source all --severity warning --scope workspace
:previous-problem --scope document
```

`--source` accepts `all`, `lsp`, `build`; `--severity` accepts `error` (errors
only), `warning` (warnings and errors), `all` (including hints/information);
`--scope` accepts `workspace` or `document`. LSP navigation uses diagnostics
already reported by language servers; an empty list does not prove the whole
project has been checked.

Run an explicit build command using the configured shell and current working
directory (set with `:cd` if necessary):

```text
:compile cmake --build build
:compile cargo check --message-format=json
:recompile
:compile-log
:compile-cancel
```

Builds run asynchronously and never save buffers or move the editing cursor
automatically. Use `:compile-log` for a read-only split containing streamed stdout,
stderr, command, directory and exit status. Cancellation stops the owned process
tree while retaining received output. Recompile remembers both command and working
directory; a second concurrent build is rejected. Failed shell startup preserves
the previous results; after successful startup a new run replaces them.

GCC/Clang `file:line[:column]`, MSVC `file(line[,column])`, Rust human diagnostics
and Cargo JSON primary spans become navigable entries. Unlocated failures such
as linker errors remain in the log. Relative paths are resolved against build
cwd; Cargo JSON also supports package-relative paths via `manifest_path` when
the invocation-relative path does not exist. Commands that change their own
directory should emit absolute diagnostic paths.

Build positions describe files on disk. Navigation flags modified buffers or
changed files as potentially stale; save and rebuild to refresh them. Generic
text formats cannot identify column units consistently across compilers: lines
containing tabs or non-ASCII text are visited at their start with a "line only"
notice. Cargo JSON and Rust diagnostics preserve character columns. Prefer Cargo
JSON for precise Rust ranges. Retained log, individual input line, and problem
count limits are 1 MiB, 64 KiB, and 10,000 respectively; truncation is shown in the
log and completion status. Stdout/stderr each preserve their own order; their
interleaving reflects receipt order.

## General configuration

For function/class folding controls and the optional clickable `fold` gutter, see
[function and class folding](editor.md#function-and-class-folding).

To override global configuration parameters, create a `config.toml` file located in your config directory:

- Linux and Mac: `~/.config/helix/config.toml`
- Windows: `%AppData%\helix\config.toml`

> 💡 You can easily open the config file by typing `:config-open` within Helix normal mode.

Example config:

```toml
theme = "onedark"

[editor]
line-number = "relative"
mouse = false

[editor.cursor-shape]
insert = "bar"
normal = "block"
select = "underline"

[editor.file-picker]
hidden = false
```

You can use a custom configuration file by specifying it with the `-c` or
`--config` command line argument, for example `hx -c path/to/custom-config.toml`.
You can reload the config file by issuing the `:config-reload` command. Alternatively, on Unix operating systems, you can reload it by sending the USR1
signal to the Helix process, such as by using the command `pkill -USR1 hx`.

Finally, you can have a `config.toml` and a `languages.toml` local to a project by putting it under a `.helix` directory in your repository.
Its settings will be merged with the configuration directory and the built-in configuration.

Local and online models (including DeepSeek) can be configured in
[`[editor.ai]`](./editor.md#editorai-section). Inline completion, function
documentation and commit assistance can each select a different named provider.
Existing connection settings in `[editor.ai-completion]` remain compatible.

Inline suggestions can be enabled with
[`[editor.ai-completion]`](./editor.md#editorai-completion-section).
They appear as grey text and are accepted in full with `Shift-Right` in insert mode.

On-demand Chinese reST function documentation can be enabled with
[`[editor.ai-doc]`](./editor.md#editorai-doc-section), then requested with `:ai-doc`.

AI-assisted file selection and Conventional Commits are available with
[`[editor.ai-commit]`](./editor.md#editorai-commit-section) and `:ai-commit`.
Chinese is the default; English can be selected in the commit workspace.

AI suggestions for the selected native Merge conflict block can be enabled with
[`[editor.ai-merge]`](./editor.md#editorai-merge-section) and requested with
`:ai-merge [optional intent]` or `Space v m i` in the Merge result.

Unit test generation for C/C++ (GoogleTest with CMake) and Python (pytest) is
configured under [`[editor.ai-test]`](./editor.md#editorai-test-section).
Use `:ai-test` to preview tests and build changes, save accepted buffers, then
`:ai-test-run` to execute the generated tests.

Function reference counts and Git authors can be enabled independently in
[`[editor.function-hints]`](./editor.md#editorfunction-hints-section).


A right-side source overview can be enabled with
[`[editor.minimap]`](./editor.md#editorminimap-section). It supports Braille or block
rendering and the session-only `:toggle-option minimap.enabled` command.
