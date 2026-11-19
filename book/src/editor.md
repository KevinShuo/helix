## Editor

- [`[editor]` Section](#editor-section)
- [`[editor.clipboard-provider]` Section](#editorclipboard-provider-section)
- [`[editor.statusline]` Section](#editorstatusline-section)
- [`[editor.lsp]` Section](#editorlsp-section)
- [`[editor.ai]` Section](#editorai-section)
- [`[editor.ai-doc]` Section](#editorai-doc-section)
- [`[editor.ai-commit]` Section](#editorai-commit-section)
- [`[editor.ai-merge]` Section](#editorai-merge-section)
- [`[editor.function-hints]` Section](#editorfunction-hints-section)
- [`[editor.minimap]` Section](#editorminimap-section)
- [`[editor.cursor-shape]` Section](#editorcursor-shape-section)
- [`[editor.file-picker]` Section](#editorfile-picker-section)
- [`[editor.file-explorer]` Section](#editorfile-explorer-section)
- [`[editor.buffer-picker]` Section](#editorbuffer-picker-section)
- [`[editor.auto-pairs]` Section](#editorauto-pairs-section)
- [`[editor.auto-save]` Section](#editorauto-save-section)
- [`[editor.search]` Section](#editorsearch-section)
- [`[editor.whitespace]` Section](#editorwhitespace-section)
- [`[editor.indent-guides]` Section](#editorindent-guides-section)
- [`[editor.gutters]` Section](#editorgutters-section)
  - [Function and class folding](#function-and-class-folding)
  - [`[editor.gutters.line-numbers]` Section](#editorguttersline-numbers-section)
  - [`[editor.gutters.diagnostics]` Section](#editorguttersdiagnostics-section)
  - [`[editor.gutters.diff]` Section](#editorguttersdiff-section)
  - [`[editor.gutters.spacer]` Section](#editorguttersspacer-section)
  - [`[editor.gutters.code-action-hint]` Section](#editorgutterscode-action-hint-section)
- [`[editor.soft-wrap]` Section](#editorsoft-wrap-section)
- [`[editor.smart-tab]` Section](#editorsmart-tab-section)
- [`[editor.inline-diagnostics]` Section](#editorinline-diagnostics-section)
- [`[editor.word-completion]` Section](#editorword-completion-section)
- [`[editor.workspace-trust]` Section](#editorworkspace-trust-section)

### `[editor]` Section

| Key | Description | Default |
|--|--|---------|
| `scrolloff` | Number of lines of padding around the edge of the screen when scrolling | `5` |
| `mouse` | Enable mouse mode | `true` |
| `mouse-yank-register` | Which register to use for mouse yanks. | `*` |
| `middle-click-paste` | Middle click paste support | `true` |
| `default-yank-register` | Default register used for yank/paste | `'"'` |
| `scroll-lines` | Number of lines to scroll per scroll wheel step | `3` |
| `shell` | Shell to use when running external commands | Unix: `["sh", "-c"]`<br/>Windows: `["cmd", "/C"]` |
| `line-number` | Line number display: `absolute` simply shows each line's number, while `relative` shows the distance from the current line. When unfocused or in insert mode, `relative` will still show absolute line numbers | `"absolute"` |
| `cursorline` | Highlight all lines with a cursor | `false` |
| `cursorcolumn` | Highlight all columns with a cursor | `false` |
| `continue-comments` | if helix should automatically add a line comment token if you create a new line inside a comment. | `true` |
| `gutters` | Gutters to display: Available are `diagnostics`, `diff`, `line-numbers`, `spacer`, `code-action-hint`, `fold` and `test`. `diagnostics` also includes other features like breakpoints. A 1-width padding will be inserted if gutters is non-empty. | `["diagnostics", "fold", "line-numbers", "spacer", "diff", "test"]` |
| `auto-completion` | Enable automatic pop up of auto-completion | `true` |
| `path-completion` | Enable filepath completion. Show files and directories if an existing path at the cursor was recognized, either absolute or relative to the current opened document or current working directory (if the buffer is not yet saved). Defaults to true. | `true` |
| `auto-format` | Enable automatic formatting on save[^3] | `true` |
| `idle-timeout` | Time in milliseconds since last keypress before idle timers trigger. | `250` |
| `completion-timeout` | Time in milliseconds after typing a word character before completions are shown, set to 5 for instant.  | `250` |
| `preview-completion-insert` | Whether to apply completion item instantly when selected | `true` |
| `completion-trigger-len` | The min-length of word under cursor to trigger autocompletion | `2` |
| `completion-replace` | Whether to make completions always replace the entire word and not just the part before the cursor | `false` |
| `auto-info` | Whether to display info boxes | `true` |
| `true-color` | Whether to override automatic detection of terminal truecolor support in the event of a false negative | `false` |
| `undercurl` | Whether to override automatic detection of terminal undercurl support in the event of a false negative | `false` |
| `rulers` | List of column positions at which to display the rulers. Can be overridden by language specific `rulers` in `languages.toml` file | `[]` |
| `bufferline` | Renders a line at the top of the editor displaying open buffers. Can be `always`, `never` or `multiple` (only shown if more than one buffer is in use) | `"never"` |
| `color-modes` | Whether to color the mode indicator with different colors depending on the mode itself | `false` |
| `text-width` | Maximum line length. Used for the `:reflow` command and soft-wrapping if `soft-wrap.wrap-at-text-width` is set | `80` |
| `workspace-lsp-roots` | Directories relative to the workspace root that are treated as LSP roots. Should only be set in `.helix/config.toml` | `[]` |
| `default-line-ending` | The line ending to use for new documents. Can be `native`, `lf`, `crlf`, `ff`, `cr` or `nel`. `native` uses the platform's native line ending (`crlf` on Windows, otherwise `lf`). | `"native"` |
| `insert-final-newline` | Whether to automatically insert a trailing line-ending on write if missing | `true` |
| `atomic-save` | Whether to use atomic operations to write documents to disk. This prevents data loss if the editor is interrupted while writing the file, but may confuse some file watching/hot reloading programs. | `true` |
| `trim-final-newlines` | Whether to automatically remove line-endings after the final one on write | `false` |
| `trim-trailing-whitespace` | Whether to automatically remove whitespace preceding line endings on write | `false` |
| `popup-border` | Draw border around `popup`, `menu`, `all`, or `none` | `"none"` |
| `indent-heuristic` | How the indentation for a newly inserted line is computed: `simple` just copies the indentation level from the previous line, `tree-sitter` computes the indentation based on the syntax tree and `hybrid` combines both approaches. If the chosen heuristic is not available, a different one will be used as a fallback (the fallback order being `hybrid` -> `tree-sitter` -> `simple`). | `"hybrid"` |
| `jump-label-alphabet` | The characters that are used to generate two character jump labels. Characters at the start of the alphabet are used first. | `"abcdefghijklmnopqrstuvwxyz"` |
| `end-of-line-diagnostics` | Minimum severity of diagnostics to render at the end of the line. Set to `disable` to disable entirely. Refer to the setting about `inline-diagnostics` for more details | `"hint"` |
| `clipboard-provider` | Which API to use for clipboard interaction. One of `pasteboard` (MacOS), `wayland`, `x-clip`, `x-sel`, `win32-yank`, `termux`, `tmux`, `windows`, `termcode`, `none`, or a custom command set. | Platform and environment specific. |
| `editor-config` | Whether to read settings from [EditorConfig](https://editorconfig.org) files | `true` |
| `rainbow-brackets` | Whether to render rainbow colors for matching brackets. Requires tree-sitter `rainbows.scm` queries for the language. | `false` |
| `kitty-keyboard-protocol` | Whether to enable Kitty Keyboard Protocol. Can be `enabled`, `disabled` or `auto` | `"auto"` |

[^3]: In most cases, you also need to enable the `auto-format` setting under `languages.toml`. You can find the reasoning [here](https://github.com/helix-editor/helix/discussions/9043#discussioncomment-7811497).

### `[editor.clipboard-provider]` Section

Helix can be configured either to use a builtin clipboard configuration or to use
a provided command.

For instance, setting it to use OSC 52 termcodes, the configuration would be:
```toml
[editor]
clipboard-provider = "termcode"
```

Alternatively, Helix can be configured to use arbitrary commands for clipboard integration:

```toml
[editor.clipboard-provider.custom]
yank = { command = "cat",  args = ["test.txt"] }
paste = { command = "tee",  args = ["test.txt"] }
primary-yank = { command = "cat",  args = ["test-primary.txt"] } # optional
primary-paste = { command = "tee",  args = ["test-primary.txt"] } # optional
```

For custom commands the contents of the yank/paste is communicated over stdin/stdout.

### `[editor.statusline]` Section

Allows configuring the statusline at the bottom of the editor.

The configuration distinguishes between three areas of the status line:

`[ ... ... LEFT ... ... | ... ... ... CENTER ... ... ... | ... ... RIGHT ... ... ]`

Statusline elements can be defined as follows:

```toml
[editor.statusline]
left = ["mode", "spinner"]
center = ["file-name"]
right = ["diagnostics", "selections", "position", "file-encoding", "file-line-ending", "file-type"]
separator = "│"
mode.normal = "NORMAL"
mode.insert = "INSERT"
mode.select = "SELECT"
diagnostics = ["warning", "error"]
workspace-diagnostics = ["warning", "error"]
```
The `[editor.statusline]` key takes the following sub-keys:

| Key           | Description | Default |
| ---           | ---         | ---     |
| `left`        | A list of elements aligned to the left of the statusline | `["mode", "spinner", "file-name", "read-only-indicator", "file-modification-indicator"]` |
| `center`      | A list of elements aligned to the middle of the statusline | `[]` |
| `right`       | A list of elements aligned to the right of the statusline | `["diagnostics", "selections", "register", "position", "file-encoding"]` |
| `separator`   | The character used to separate elements in the statusline | `"│"` |
| `mode.normal` | The text shown in the `mode` element for normal mode | `"NOR"` |
| `mode.insert` | The text shown in the `mode` element for insert mode | `"INS"` |
| `mode.select` | The text shown in the `mode` element for select mode | `"SEL"` |
| `diagnostics` | A list of severities which are displayed for the current buffer | `["warning", "error"]` |
| `workspace-diagnostics` | A list of severities which are displayed for the workspace | `["warning", "error"]` |

The following statusline elements can be configured:

| Key    | Description |
| ------ | ----------- |
| `mode` | The current editor mode (`mode.normal`/`mode.insert`/`mode.select`) |
| `spinner` | A progress spinner indicating LSP activity |
| `file-name` | The path/name of the opened file |
| `file-absolute-path` | The absolute path/name of the opened file |
| `file-base-name` | The basename of the opened file |
| `current-working-directory` | The current working directory  |
| `file-modification-indicator` | The indicator to show whether the file is modified (a `[+]` appears when there are unsaved changes) |
| `file-encoding` | The encoding of the opened file if it differs from UTF-8 |
| `file-line-ending` | The file line endings (CRLF or LF) |
| `file-indent-style` | The file indentation style |
| `read-only-indicator` | An indicator that shows `[readonly]` when a file cannot be written |
| `total-line-numbers` | The total line numbers of the opened file |
| `file-type` | The type of the opened file |
| `diagnostics` | The number of warnings and/or errors |
| `workspace-diagnostics` | The number of warnings and/or errors on workspace |
| `selections` | The primary selection index out of the number of active selections |
| `primary-selection-length` | The number of characters currently in primary selection |
| `position` | The cursor position |
| `position-percentage` | The cursor position as a percentage of the total number of lines |
| `separator` | The string defined in `editor.statusline.separator` (defaults to `"│"`) |
| `spacer` | Inserts a space between elements (multiple/contiguous spacers may be specified) |
| `version-control` | The current branch name or detached commit hash of the opened workspace |
| `register` | The current selected register |
| `code-action-hint` | Indicator for when code actions are available |

### `[editor.lsp]` Section

| Key                   | Description                                                 | Default |
| ---                   | -----------                                                 | ------- |
| `enable`              | Enables LSP integration. Setting to false will completely disable language servers regardless of language settings.| `true` |
| `display-messages`    | Display LSP `window/showMessage` messages below statusline[^1] | `true` |
| `display-progress-messages` | Display LSP progress messages below statusline[^1]    | `false` |
| `auto-signature-help` | Enable automatic popup of signature help (parameter hints)  | `true`  |
| `auto-document-highlight` | Automatically highlight symbol references at the cursor | `false` |
| `display-inlay-hints` | Display inlay hints[^2]                                     | `false` |
| `inlay-hints-length-limit` | Maximum displayed length (non-zero number) of inlay hints | Unset by default  |
| `display-color-swatches` | Show color swatches next to colors | `true` |
| `display-signature-help-docs` | Display docs under signature help popup             | `true`  |
| `snippets`      | Enables snippet completions. Requires a server restart (`:lsp-restart`) to take effect after `:config-reload`/`:set`. | `true`  |
| `goto-reference-include-declaration` | Include declaration in the goto references popup. | `true`  |

[^1]: By default, a progress spinner is shown in the statusline beside the file path.

[^2]: You may also have to activate them in the language server config for them to appear, not just in Helix. Inlay hints in Helix are still being improved on and may be a little bit laggy/janky under some circumstances. Please report any bugs you see so we can fix them!

### `[editor.ai-commit]` Section

Explicit AI-assisted staging and commits select their own `provider` from
[`[editor.ai]`](#editorai-section), independently of automatic completion and
`ai-doc`. Without a provider or global default, legacy connection settings in
`[editor.ai-completion]` continue to apply.

```toml
[editor.ai-commit]
enabled = true
language = "zh-CN" # or "en"
timeout-ms = 30000
max-tokens = 768
max-input-bytes = 4096
max-files = 64
```

The feature is disabled by default. `max-input-bytes` bounds each batch's data,
not tokens; the actual configured model context is checked separately.
`max-files` defaults to 64 and can be increased up to an effective ceiling of
1024; it must be positive. Recommendation counts all changed files, while message
generation counts only staged files. Increasing this limit may also require a
larger `max-input-bytes` and model context to fit the final summaries; summaries
and diffs are never truncated to fit. Configured/local analysis uses at most 64
logical model calls. Automatic DeepSeek uses up to 256, with up to three HTTP
attempts per call for recognized input/context limits. Large diffs,
binary files, symlinks, submodules and common credential paths require manual
review; their contents are not sent to the model. These heuristics cannot
identify every secret. The configured provider receives eligible diffs when
you explicitly request analysis. Ignored files are not added.

Configured local commit `max-tokens` allows up to 16384 final-output tokens,
with reasoning reserved separately. Local HTTP timeouts are bounded to
100–600000 ms. Known DeepSeek automatic budgets use the provider policy described
below instead of these feature resource limits. Complete-input and review checks
remain in effect.

Open with `:ai-commit [optional intent]` or the Git action menu's `A` entry.
The workspace defaults to the bottom dock; F11 toggles fullscreen. Space toggles
candidate files, Tab switches file/diff navigation, and j/k or arrows move.
`s` stages the selected complete disk versions, `u` unstages the selected staged
file without changing disk contents. Existing staged files are all included;
remaining edits of partially staged files are never selected automatically.

`g` generates a message from the actual staged diff. `l` switches Chinese/English
for the next generation while preserving the draft. `e` edits the message;
Enter inserts a newline, Escape or Ctrl-s ends editing. `i` edits the intent.
`C` commits the reviewed Conventional Commits message. `v` explicitly retains a
manually checked draft after a scope change or incomplete AI analysis. `d` opens
a local-only complete diff preview, including files withheld from AI. `r`
refreshes. Ctrl-c or `:ai-commit-cancel` cancels reads/AI requests, preserving the
draft and index. `q` closes the workspace; confirmed Git writes finish in the
background and report their result. Ctrl-w releases keyboard focus to the editor;
click the workspace or invoke `:ai-commit` again to return.

During AI work, the status line shows the current stage, batch count and analyzed
file count. Elapsed and current-stage timers refresh every second even while the
model is silent. File counts advance only after valid summaries arrive; files
requiring manual review are counted separately. Batch totals adjust when rejected
requests are split for retry. The timer indicates waiting, not model token progress.
For managed local llama.cpp models, `本次 token` samples the active request's actual
decoded-token count about once per second. It resets for each batch/request stage.
Unsupported providers and unavailable telemetry show `暂不可用`; no estimated token
count is substituted. Token samples do not reset the stage timer, and stop with
the canceled or completed operation.

Save related modified editor buffers before analyzing or staging them. Stale
HEAD/index/file snapshots are rejected before writes. Git hooks and signing are
respected; errors keep the draft and actual index for inspection. Commits do not
automatically push. Manual selection, staging, editing and committing remain
available when AI is disabled or unavailable.

### `[editor.ai-merge]` Section

Generate a suggestion for the selected conflict block in the native Git Merge
workspace. Enable explicitly; it is disabled by default:

```toml
[editor.ai-merge]
enabled = true
timeout-ms = 60000
max-tokens = 1536
max-input-bytes = 16384
```

The connection uses `[editor.ai]` and the optional per-feature `provider`; legacy
`[editor.ai-completion]` connections remain supported. Automatic completion may
remain disabled. To keep a small completion model resident and load a larger
managed model only for Merge, set `local-provider = "local-27b-direct"` here and
define that provider with a distinct endpoint, model path and `thinking = false`.
See `contrib/config-ai-providers.toml` for a complete example. The resident model
is unloaded before generation and restored after success, error or cancellation.
`local-provider` applies when inheriting local mode; an explicit `provider` takes
precedence, and an inherited online provider follows the global switch.

Configured local output is bounded to 128–16384 final tokens,
and its HTTP timeout to 100–600000 ms; model startup uses its existing separate
timeout. `max-input-bytes` bounds the system prompt and serialized input, while
the actual model context is checked independently with space reserved for output
and any configured reasoning budget.

Open a conflict with `Space v r`, select a block with `Space v m n/p`, then click
`AI Merge`, press `Space v m i`, or run `:ai-merge [optional intent]`, for example
`:ai-merge preserve both validation checks`. The configured provider receives
the complete Base, Current, Incoming and current Result block, the relative file
path/language and your intent. Nearby code and file-header declarations/imports
are included only when they fit the budget. Complete required input that does
not fit is rejected rather than truncated. No other project files are read.

The read-only preview shows the current block, proposed replacement and a Chinese
explanation. Enter accepts, Escape/Ctrl-c cancels, `r` regenerates, and `j/k`,
arrows or PageUp/PageDown scroll. A `needs-manual` response displays the reason
without an accept action. While generating, use `:ai-merge-cancel` to cancel.
An existing prompt keeps keyboard focus until closed.
Configured/local decorated previews retain a 60,000-byte bound. Known DeepSeek
supports larger provider-sized previews with viewport-only rendering and `usize`
scrolling; Home/End jump to the start/end, including in narrow terminals.
Oversized candidates are rejected without editing the draft. Compact terminals
reserve visible status and controls.

Acceptance replaces only the selected Result range and is one undo/redo step.
The block remains **Needs Review**, even if the suggested text is identical.
Use the existing Review and Apply actions to write/stage the file, retaining its
original supported BOM and line endings; Finish Merge remains a separate action.
Suggestions do not establish that the code compiles or passes tests.

Editing, navigating blocks, leaving the Merge session, refreshing/rebuilding or
changing configuration invalidates a pending request/candidate. Timeout and
invalid responses leave the draft intact. Staged, stale, applying and whole-file
review results cannot generate a block suggestion. AI Merge requires a trusted
workspace and Git-enabled build. Manual AI doc/commit generation is mutually
exclusive, and automatic completion pauses while a merge suggestion is pending.
If one complete block side is deleted and the other is modified, an empty intent
shows a local needs-manual explanation without contacting the model. Choose a
side manually or rerun `:ai-merge` with the desired outcome. This structural check
does not establish semantic correctness for other conflicts; Review is required.

### `[editor.function-hints]` Section

Show reference counts and the latest Git commit author at the end of visible
function, method and constructor declarations. These are virtual annotations;
they are never inserted into the file.

```toml
[editor.function-hints]
references = true
git-author = true
```

| Key | Description | Default |
| --- | --- | --- |
| `references` | Show `1 usage` or `N usages`, excluding the declaration itself | `false` |
| `git-author` | Show the author of the latest commit changing the function | `false` |

Both hints require an active language server providing document symbols.
Reference counts additionally require reference support and reflect the scope
indexed by that server. Unsupported or failed requests show no count. Use `gr`
on the function name to open the existing references picker.

Git authors require a trusted Git workspace, the `git` executable, and a build
with Git support. The author comes from the most recent commit touching the
function's full range, including changes that only delete code. `*` marks local
changes to that function relative to HEAD. New/untracked functions and ambiguous
unsaved range mappings have no author hint. Servers returning only flat symbol
locations cannot provide full function ranges, so they show reference counts only.
Author hints are omitted for Git blobs that are not UTF-8 text.

Requests run in the background after the editor becomes idle outside insert mode,
for up to 64 visible declarations per view, with at most four reference queries
in flight per view. Edits invalidate results. While idle, hints refresh every
15 seconds to pick up external commits and changes indexed by the language server.
Git queries are limited to 16 MiB files and time out after 15 seconds.

These settings are independent of `editor.lsp.display-inlay-hints`. They can also
be changed with `:toggle function-hints.references` and
`:toggle function-hints.git-author`.

### `[editor.minimap]` Section

The optional minimap shows a source overview at the right of each editing split.
It uses colored Braille dots to preserve indentation and code structure in a
terminal. It is disabled by default. Enable it in `config.toml`:

```toml
[editor.minimap]
enabled = true
```

| Key | Description | Default |
| --- | --- | --- |
| `enabled` | Show the source minimap in editing splits. | `false` |
| `width` | Total terminal columns, including the separator and two marker lanes; 8–40. | `16` |
| `render` | `"braille"` for 2×4 dots, or `"blocks"` for 2×2 blocks when Braille glyphs are unsuitable. | `"braille"` |
| `min-editor-width` | Minimum remaining text width after gutters and minimap; at least 20. | `60` |
| `diagnostics` | Show existing diagnostic ranges in the right marker lane. | `true` |
| `git-diff` | Show existing Git additions, modifications and deletions in the other lane. | `true` |

The minimap hides automatically when a split cannot retain `min-editor-width`
text columns or has fewer than four text rows. Marker lanes retain their width
when switched off. Short files stay at the top; long files fit the minimap's
height. The first 160 display columns of source are shown. Virtual annotations,
AI suggestions and soft wrapping do not add source lines to the map.

With `editor.mouse` enabled, click outside the highlighted viewport to center
that source region; drag the viewport to scroll. A hidden fold destination is
expanded even when it lies inside the viewport highlight. Wheel events over a
minimap scroll its split while preserving keyboard focus. Navigation follows
Helix's scrolling rules: a cursor leaving the viewport is moved into it, the
select-mode anchor is preserved, and other selections are retained.

Use `:toggle-option minimap.enabled` or `:set-option minimap.enabled false` for
a session-only change. Edit `config.toml` and run `:config-reload` to persist a
change. You can switch the rendering mode with `:set-option minimap.render blocks`.
No default key binding is assigned.

Source summaries are cached and built in small, debounced jobs. Files larger
than 50,000 lines or 4MiB use a bounded monochrome sample; short code fragments
can be omitted in that mode. Missing syntax, diagnostics or Git data leave
those colors/markers unavailable. No extra LSP or Git requests are made.
Optional theme scopes are `ui.minimap`, `ui.minimap.viewport` and
`ui.minimap.cursor`; existing themes use fallback colors.

### `[editor.cursor-shape]` Section

Defines the shape of cursor in each mode.
Valid values for these options are `block`, `bar`, `underline`, or `hidden`.

> 💡 Due to limitations of the terminal environment, only the primary cursor can
> change shape.

| Key      | Description                                | Default   |
| ---      | -----------                                | -------   |
| `normal` | Cursor shape in [normal mode][normal mode] | `"block"` |
| `insert` | Cursor shape in [insert mode][insert mode] | `"block"` |
| `select` | Cursor shape in [select mode][select mode] | `"block"` |

[normal mode]: ./keymap.md#normal-mode
[insert mode]: ./keymap.md#insert-mode
[select mode]: ./keymap.md#select--extend-mode

### `[editor.file-picker]` Section

Set options for file picker and global search. Ignoring a file means it is
not visible in the Helix file picker and global search.

All git related options are only enabled in a git repository.

| Key | Description | Default |
|--|--|---------|
|`hidden` | Enables ignoring hidden files | `true`
|`follow-symlinks` | Follow symlinks instead of ignoring them | `true`
|`deduplicate-links` | Ignore symlinks that point at files already shown in the picker | `true`
|`parents` | Enables reading ignore files from parent directories | `true`
|`ignore` | Enables reading `.ignore` files | `true`
|`git-ignore` | Enables reading `.gitignore` files | `true`
|`git-global` | Enables reading global `.gitignore`, whose path is specified in git's config: `core.excludesfile` option | `true`
|`git-exclude` | Enables reading `.git/info/exclude` files | `true`
|`max-depth` | Set with an integer value for maximum depth to recurse | Unset by default

Ignore files can be placed locally as `.ignore` or put in your home directory as `~/.ignore`. They support the usual ignore and negative ignore (unignore) rules used in `.gitignore` files.

Additionally, you can use Helix-specific ignore files by creating a local `.helix/ignore` file in the current workspace or a global `ignore` file located in your Helix config directory:
- Linux and Mac: `~/.config/helix/ignore`
- Windows: `%AppData%\helix\ignore`

Example:

```ini
# unignore in file picker and global search
!.github/
!.gitignore
!.gitattributes
```

### `[editor.file-explorer]` Section

In addition to the options for the file picker and global search, a similar set of options is presented to configure the file explorer separately. However, unlike the file picker, the defaults are set to avoid ignoring most files.

Inside the explorer, press `F4` to open **File actions**.

| Key | Action |
| --- | --- |
| `n` | Create an empty file in the displayed directory |
| `m` | Create a directory in the displayed directory |
| `r` | Rename the selected entry. Typing replaces the selected file stem. |
| `d` | Permanently delete the selected file or empty directory |

Press `Enter` to submit a name. The explorer selects the new entry so you can
press `Enter` again to open it. Deletion requires an explicit `y`. Pressing `Enter` or
`Esc` cancels. Save or discard pending edits before deleting an open file.
Exclusive rename is supported on Windows, Linux, Android, Apple platforms, and Redox
when the filesystem supports it.

Note that the ignore files consulted by the file explorer when `ignore` is set to true are the same ones used by the file picker, including the aforementioned Helix-specific ignore files.


| Key | Description | Default |
|--|--|---------|
|`hidden` | Enables ignoring hidden files | `false`
|`follow-symlinks` | Follow symlinks instead of ignoring them | `false`
|`parents` | Enables reading ignore files from parent directories | `false`
|`ignore` | Enables reading `.ignore` files | `false`
|`git-ignore` | Enables reading `.gitignore` files | `false`
|`git-global` | Enables reading global `.gitignore`, whose path is specified in git's config: `core.excludesfile` option | `false`
|`git-exclude` | Enables reading `.git/info/exclude` files | `false`
|`flatten-dirs` | Enables flattening single child directories | `true`

### `[editor.buffer-picker]` Section

Set options for buffer picker.

| Key | Description | Default |
|--|--|---------|
|`start-position` | Controls behavior for which buffer is initially selected | `current` |

Example

```toml
[editor.buffer-picker]
start-position = "previous"
```

### `[editor.auto-pairs]` Section

Enables automatic insertion of pairs to parentheses, brackets, etc. Can be a
simple boolean value, or a specific mapping of pairs of single characters.

To disable auto-pairs altogether, set `auto-pairs` to `false`:

```toml
[editor]
auto-pairs = false # defaults to `true`
```

The default pairs are <code>(){}[]''""``</code>, but these can be customized by
setting `auto-pairs` to a TOML table:

```toml
[editor.auto-pairs]
'(' = ')'
'{' = '}'
'[' = ']'
'"' = '"'
'`' = '`'
'<' = '>'
```

Additionally, this setting can be used in a language config. Unless
the editor setting is `false`, this will override the editor config in
documents with this language.

Example `languages.toml` that adds `<>` and removes `''`

```toml
[[language]]
name = "rust"

[language.auto-pairs]
'(' = ')'
'{' = '}'
'[' = ']'
'"' = '"'
'`' = '`'
'<' = '>'
```

### `[editor.auto-save]` Section

Control auto save behavior.

| Key | Description | Default |
|--|--|---------|
| `focus-lost` | Enable automatic saving on the focus moving away from Helix. Requires [focus event support](https://github.com/helix-editor/helix/wiki/Terminal-Support) from your terminal | `false` |
| `after-delay.enable` | Enable automatic saving after `auto-save.after-delay.timeout` milliseconds have passed since last edit. | `false` |
| `after-delay.timeout` | Time in milliseconds since last edit before auto save timer triggers. | `3000` |

### `[editor.search]` Section

Search specific options.

| Key | Description | Default |
|--|--|---------|
| `smart-case` | Enable smart case regex searching (case-insensitive unless pattern contains upper case characters) | `true` |
| `wrap-around`| Whether the search should wrap after depleting the matches | `true` |

### `[editor.whitespace]` Section

Options for rendering whitespace with visible characters. Use `:set whitespace.render all` to temporarily enable visible whitespace.

| Key | Description | Default |
|-----|-------------|---------|
| `render` | Whether to render whitespace. May either be `all` or `none`, or a table with sub-keys `space`, `nbsp`, `nnbsp`, `tab`, and `newline` | `"none"` |
| `characters` | Literal characters to use when rendering whitespace. Sub-keys may be any of `tab`, `space`, `nbsp`, `nnbsp`, `newline` or `tabpad` | See example below |

Example

```toml
[editor.whitespace]
render = "all"
# or control each character
[editor.whitespace.render]
space = "all"
tab = "all"
nbsp = "none"
nnbsp = "none"
newline = "none"

[editor.whitespace.characters]
space = "·"
nbsp = "⍽"
nnbsp = "␣"
tab = "→"
newline = "⏎"
tabpad = "·" # Tabs will look like "→···" (depending on tab width)
```

### `[editor.indent-guides]` Section

Options for rendering vertical indent guides.

| Key           | Description                                             | Default |
| ---           | ---                                                     | ---     |
| `render`      | Whether to render indent guides                         | `false` |
| `character`   | Literal character to use for rendering the indent guide | `"│"`   |
| `skip-levels` | Number of indent levels to skip                         | `0`     |

Example:

```toml
[editor.indent-guides]
render = true
character = "╎" # Some characters that work well: "▏", "┆", "┊", "⸽"
skip-levels = 1
```

### `[editor.gutters]` Section

#### Function and class folding

Python functions/classes, C functions/structs and C++ functions/classes/structs
can be folded with `za` (toggle), `zf`
(close), `zo` (open), `zM` (close all) and `zR` (open all). The same keys work
inside sticky `Z` mode. Definitions start expanded. Tree-sitter grammars and the
matching `folds.scm` queries must be available; other block types, single-line
definitions and indentation fallback are not supported.

Folding preserves decorators and complete multiline signatures. The body becomes
`…`, and C/C++ closing braces remain visible. Fold state belongs to each view and
survives buffer switching; a new split starts expanded. Directional motions skip
closed bodies. Search, word motions and navigation into a body reveal it. Folding
from inside a body moves the primary cursor to the declaration; a second cursor
inside prevents folding. Fold-all skips definitions containing editing cursors.
Closing a class preserves any nested method folds for when the class is reopened.

Selections, copying, editing, saving, LSP and undo still operate on the real text.
Edits affecting a definition unfold it; edits elsewhere shift its range and discard
folds whose syntax is no longer valid. Explicit reload and language changes clear
folds. Git refreshes retain folds when the file's text is unchanged.
Folding itself does not modify the file or create undo entries.

To show clickable fold arrows beside line numbers, opt in to the `fold` gutter:

```toml
[editor.gutters]
layout = ["diagnostics", "spacer", "line-numbers", "fold", "spacer", "diff"]

[editor.gutters.fold]
foldable = "▾"
folded = "▸"
```

Use `foldable = "-"` and `folded = "+"` for ASCII symbols. The arrow appears on
the first declaration line, including when the signature spans multiple lines;
decorators and template wrappers stay above it. Click `▾` to fold and `▸` to
expand. Mouse support (`editor.mouse`, enabled by default) must be enabled.
Blank rows in this gutter do nothing.
Function reference/author hints remain visible at declarations; hidden nested
hints and diagnostics are omitted. Changing folds clears any pending AI preview.

#### Gutter layout

For simplicity, `editor.gutters` accepts an array of gutter types, which will
use default settings for all gutter components.

```toml
[editor]
gutters = ["diff", "diagnostics", "line-numbers", "spacer"]
```

To customize the behavior of gutters, the `[editor.gutters]` section must
be used. This section contains top level settings, as well as settings for
specific gutter components as subsections.

| Key      | Description                    | Default                                                       |
| ---      | ---                            | ---                                                           |
| `layout` | A vector of gutters to display | `["diagnostics", "fold", "line-numbers", "spacer", "diff", "test"]` |

Example:

```toml
[editor.gutters]
layout = ["diff", "diagnostics", "line-numbers", "spacer"]
```

#### `[editor.gutters.line-numbers]` Section

Options for the line number gutter

| Key         | Description                             | Default |
| ---         | ---                                     | ---     |
| `min-width` | The minimum number of characters to use | `3`     |

Example:

```toml
[editor.gutters.line-numbers]
min-width = 1
```

#### `[editor.gutters.diagnostics]` Section

Currently unused

#### `[editor.gutters.diff]` Section

The `diff` gutter option displays colored bars indicating whether a `git` diff represents that a line was added, removed or changed.
These colors are controlled by the theme attributes `diff.plus`, `diff.minus` and `diff.delta`.

Other diff providers will eventually be supported by a future plugin system.

There are currently no options for this section.

#### `[editor.gutters.spacer]` Section

Currently unused

#### `[editor.gutters.code-action-hint]` Section

The `code-action-hint` gutter option displays an indicator for whether a code action is available at current selection.

There are currently no options for this section.

### `[editor.soft-wrap]` Section

Options for soft wrapping lines that exceed the view width:

| Key                  | Description                                                  | Default |
| ---                  | ---                                                          | ---     |
| `enable`             | Whether soft wrapping is enabled.                            | `false` |
| `max-wrap`           | Maximum free space left at the end of the line.              | `20`    |
| `max-indent-retain`  | Maximum indentation to carry over when soft wrapping a line. | `40`    |
| `wrap-indicator`     | Text inserted before soft wrapped lines, highlighted with `ui.virtual.wrap` | `"↪ "`    |
| `wrap-at-text-width` | Soft wrap at `text-width` instead of using the full viewport size. | `false` |

Example:

```toml
[editor.soft-wrap]
enable = true
max-wrap = 25 # increase value to reduce forced mid-word wrapping
max-indent-retain = 0
wrap-indicator = ""  # set wrap-indicator to "" to hide it
```

### `[editor.smart-tab]` Section

Options for navigating and editing using tab key.

| Key        | Description | Default |
|------------|-------------|---------|
| `enable` | If set to true, then when the cursor is in a position with non-whitespace to its left, instead of inserting a tab, it will run `move_parent_node_end`. If there is only whitespace to the left, then it inserts a tab as normal. With the default bindings, to explicitly insert a tab character, press Shift-tab. | `true` |
| `supersede-menu` | Normally, when a menu is on screen, such as when auto complete is triggered, the tab key is bound to cycling through the items. This means when menus are on screen, one cannot use the tab key to trigger the `smart-tab` command. If this option is set to true, the `smart-tab` command always takes precedence, which means one cannot use the tab key to cycle through menu items. One of the other bindings must be used instead, such as arrow keys or `C-n`/`C-p`. | `false` |


Due to lack of support for S-tab in some terminals, the default keybindings don't fully embrace smart-tab editing experience. If you enjoy smart-tab navigation and a terminal that supports the [Enhanced Keyboard protocol](https://github.com/helix-editor/helix/wiki/Terminal-Support#enhanced-keyboard-protocol), consider setting extra keybindings:

```toml
[keys.normal]
tab = "move_parent_node_end"
S-tab = "move_parent_node_start"

[keys.insert]
S-tab = "move_parent_node_start"

[keys.select]
tab = "extend_parent_node_end"
S-tab = "extend_parent_node_start"
```

### `[editor.inline-diagnostics]` Section

Options for rendering diagnostics inside the text like shown below

```text
fn main() {
  let foo = bar;
            └─ no such value in this scope
}
```

| Key        | Description | Default |
|------------|-------------|---------|
| `cursor-line` | The minimum severity that a diagnostic must have to be shown inline on the line that contains the primary cursor. Set to `disable` to not show any diagnostics inline. This option does not have any effect when in insert-mode and will only take effect 350ms after moving the cursor to a different line. | `"warning"` |
| `other-lines` | The minimum severity that a diagnostic must have to be shown inline on a line that does not contain the cursor-line. Set to `disable` to not show any diagnostics inline. | `"disable"` |
| `prefix-len` | How many horizontal bars `─` are rendered before the diagnostic text.  | `1` |
| `max-wrap` | Equivalent of the `editor.soft-wrap.max-wrap` option for diagnostics.  | `20` |
| `max-diagnostics` | Maximum number of diagnostics to render inline for a given line  | `10` |

The allowed values for `cursor-line` and `other-lines` are: `error`, `warning`, `info`, `hint`.

The (first) diagnostic with the highest severity that is not shown inline is rendered at the end of the line (as long as its severity is higher than the `end-of-line-diagnostics` config option):

```text
fn main() {
  let baz = 1;
  let foo = bar; a local variable with a similar name exists: baz
            └─ no such value in this scope
}
```

### `[editor.ai]` Section

Configure named models once and select them independently for inline completion,
function documentation and commit assistance. Providers use OpenAI-compatible
Chat Completions; their `endpoint` is the complete URL, including any API path.

```toml
[editor.ai]
default-provider = "deepseek"

[editor.ai.providers.deepseek]
type = "deepseek"
endpoint = "https://api.deepseek.com/chat/completions"
model = "deepseek-flash"
api-key-env = "DEEPSEEK_API_KEY"
context-size = 32768
thinking = false
proxy = "system"

[editor.ai.providers.local]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19239/v1/chat/completions"
model = "local-coder"
model-path = 'D:\models\coder.gguf'
server-path = "llama-server"
context-size = 8192
gpu-layers = 99

[editor.ai-completion]
enabled = true
provider = "local"
debounce-ms = 700
timeout-ms = 10000
max-tokens = 128

[editor.ai-doc]
enabled = true
provider = "deepseek"
timeout-ms = 60000
max-tokens = 1024

[editor.ai-commit]
enabled = true
provider = "deepseek"
timeout-ms = 60000
max-tokens = 1024
```

Replace the local model path with an installed GGUF. To use online completion
as well, change its `provider` to `"deepseek"`. A provider entry represents one
model configuration; add another name to use a different model at the same API.
No AI feature is enabled merely by defining a provider.

Selection order is the feature's `provider`, then `editor.ai.default-provider`,
then the legacy connection fields in `[editor.ai-completion]`. Unknown names
are configuration errors, including references on disabled features. Existing
configurations need no migration; local startup and API authentication remain
available through the legacy fields. When a named provider is selected, its
connection settings are used in full, without inheriting legacy connection fields.
Feature settings such as prompt mode, debounce, language and token limits remain
in their respective sections.

| Provider key | Description | Default |
| --- | --- | --- |
| `type` | `deepseek`, `llama-cpp`, or `openai-compatible` | `"openai-compatible"` |
| `endpoint` | Full HTTP(S) chat completions URL, without embedded credentials | required |
| `model` | Model identifier or local server alias | required |
| `api-key-env` | Environment variable containing the bearer token | unset |
| `budget-mode` | `auto` uses a known provider/model capability profile; `configured` uses feature limits. Unknown models use configured limits in either mode | `"auto"` |
| `context-size` | Configured context budget; local/unknown models cap at 131072. Known DeepSeek auto mode uses its official context instead | `4096` |
| `thinking` | Enable provider-specific thinking for DeepSeek or llama.cpp; generic endpoints receive no thinking extensions | `false` |
| `thinking-budget` | llama.cpp reasoning token budget, 1–8192; does not enable thinking by itself | unset |
| `reasoning-effort` | Model-specific reasoning depth: `low`, `medium`, `high`, `xhigh`, or `max`; sent to llama.cpp when thinking is enabled | unset |
| `proxy` | `system` uses reqwest's proxy discovery; `none` connects directly | `"system"` |
| `model-path` | GGUF to start and manage; only for `llama-cpp`; omit for an existing server | unset |
| `server-path` | Executable for a managed local model | `"llama-server"` |
| `gpu-layers` | Managed local GPU layers, capped at 999; `0` uses CPU | `99` |
| `startup-timeout-ms` | Managed local loading timeout, clamped to 100–600000 ms | `120000` |
| `runtime` | Typed managed llama.cpp performance settings (see below); only non-default settings on `llama-cpp` providers | defaults below |

Managed llama.cpp servers already enable FlashAttention and use one inference
slot. Tune their memory footprint and speculative decoding per provider:

```toml
[editor.ai.providers.local-27b.runtime]
cache-type-k = "q8_0"
cache-type-v = "q8_0"
speculative-decoding = "mtp"
draft-tokens = 3
threads = 8
threads-batch = 8
ubatch-size = 256
```

Both cache types accept `f16` (default), `q8_0` and `q4_0`. Quantized KV caches
reduce cache memory, with model-dependent precision/performance tradeoffs.
`speculative-decoding` accepts `none` (default), `mtp`, `ngram-simple` and
`ngram-mod`. MTP requires a compatible model with prediction heads and a runtime
supporting `--spec-type draft-mtp`; it does not enable thinking or use another
downloaded model. `draft-tokens` is 1–64 (default 3).

`backend-sampling = true` enables llama.cpp's experimental GPU sampler; it is
disabled by default and requires a supporting runtime and sampler chain.

`load-mode` accepts `auto` (default), `mmap`, `none` (ordinary reads), or `dio`
(Direct I/O). Explicit modes require a supporting llama.cpp runtime; benchmark
startup on the actual model/storage before overriding `auto`. This controls
loading only and does not change model weights or the context budget.

Optional `threads` and `threads-batch` accept 1–1024. Optional `batch-size` and
`ubatch-size` accept 1–65536; the microbatch must not exceed the logical batch
(runtime defaults 512 and 2048 respectively). Omitted optional settings use
llama.cpp defaults. Runtime changes reload the managed process; thinking and
output-budget edits keep it loaded. These settings apply to processes Helix
starts, not to an externally managed server. Defaults keep the previous F16
caches and omit speculative flags for compatibility with older runtimes.

Measure prompt processing and generation separately. If weights or caches exceed
dedicated GPU memory, CPU offload or Windows shared-memory paging can outweigh
kernel optimizations. Reducing cache memory can permit full GPU offload without
reducing model weight precision or the context window. A performance comparison
runner is available at `contrib/ai-local-performance.py`.

Known DeepSeek automatic budgets currently recognize `deepseek-flash`,
`deepseek-v4-pro` and the documented Flash aliases `deepseek-v4-flash` and
`deepseek-v4-flash-vision-exp`. The profile verified on 2026-11-16 allows 1048576
context tokens and up to 393216 generated tokens. The API, not a character or byte
estimate, enforces exact token accounting. These are ceilings, not requested
response lengths; the task instructions still determine what to generate.

```toml
[editor.ai.providers.deepseek]
type = "deepseek"
endpoint = "https://api.deepseek.com/chat/completions"
model = "deepseek-flash"
budget-mode = "auto" # "configured" restores explicit feature resource limits
api-key-env = "DEEPSEEK_API_KEY"
```

In auto mode, legacy `max-input-bytes`, `max-tokens` and commit `max-files` do not
silently impose local-model limits. Complete requests/responses/generated
artifacts retain a finite 16 MiB memory envelope; commit snapshots retain their
4096-file ceiling, with batches up to 128 files. A two-hour finite request deadline
includes all retries and remains cancellable. On a recognized context reservation
error, the client retries with a small output reservation, retaining identical
input. If a length-limited response reports actual prompt-token usage, one final
attempt can use the remaining model context. At most three attempts are made;
unrelated errors do not trigger automatic retries or input truncation.

Automatic inline completion retains its short output/deadline contract, even
with a DeepSeek auto provider. Unknown model IDs and local providers retain
configured behavior. In configured mode, known DeepSeek still permits manually
chosen limits up to its official capacity and accepts provider-sized responses.
Provider selection, credentials, thinking mode and explicit acceptance do not
change merely by selecting a budget mode.

Set the API key in the environment inherited by Helix, then launch Helix from
that environment. For example, in PowerShell:

```powershell
$env:DEEPSEEK_API_KEY = "your-api-key"
hx
```

Keys are read when sending requests. Raw keys are not accepted in TOML. Legacy
connections retain their direct-connection behavior. Managed local models always
connect directly and require `http://127.0.0.1:PORT/v1/chat/completions`.
Only managed models selected by an enabled feature are started. Features can
share one local process; distinct active local model configurations must use
different endpoints. Reloading removes unused processes and invalidates pending
results. No online request automatically falls back to another provider.

The selected online service receives the feature's existing bounded source or
diff context. Inline requests are debounced and canceled when editing continues;
they do not retry automatically. Explicit generation retains preview and result
validation. Requests are non-streaming. DeepSeek uses its own `thinking` parameter;
llama.cpp uses its template parameters; generic endpoints receive neither.

To use a larger model only for explicit local generation while preserving the
single global switch, configure a second managed provider and use `local-provider`:

```toml
[editor.ai.providers.local-27b]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19241/v1/chat/completions"
model = "Qwen3.8-27B-UD-Q5_K_M"
model-path = '${USERPROFILE}\.local\share\junie-local\models\Qwen3.8-27B-UD-Q5_K_M.gguf'
server-path = "llama-server"
context-size = 16384
gpu-layers = 99
startup-timeout-ms = 180000
thinking = true
thinking-budget = 2048
reasoning-effort = "medium"

[editor.ai-doc]
enabled = true
local-provider = "local-27b"
timeout-ms = 120000
max-tokens = 1024

[editor.ai-commit]
enabled = true
local-provider = "local-27b"
timeout-ms = 120000
max-tokens = 1024
```

Merge these keys into existing sections. With `default-provider = "local"`,
completion uses the ordinary local provider while these explicit tasks acquire
the larger model temporarily. With `default-provider = "deepseek"`, all three
features use DeepSeek. An explicit feature `provider` always wins; omit it to
follow global mode. Conditional providers must be managed `llama-cpp` models with
a `model-path`. Omitted thinking budgets/efforts on conditional thinking providers
default to 2048/`medium`.

The larger model is not loaded at startup. On Windows and Linux, editor instances
share a user-local background model manager. Requests queue for one model lease;
the manager unloads the ordinary model before loading a temporary generation
model, then restores the requesting editor's ordinary model. One lease covers the
whole documentation request or multi-call commit analysis. Other windows' inline
completion waits during generation and remains cancelable. Matching model and
runtime settings reuse one process; differing settings reload it between requests.

Closing the first editor does not stop models used by other instances. Ten seconds
after the last editor and request disconnect, the manager exits and releases its
models, including a model still loading. Configuration reload cancels requests
using the previous configuration. Restart all existing editors after upgrading
from the older editor-owned model process. A live server started outside the
shared manager can be reused for matching completion, but cannot be unloaded for
model switching; close its owner first. If that external owner exits, a later
request can take over its now-free endpoint. macOS/BSD retain the original
editor-owned lifecycle until equivalent process-crash cleanup is available.

For these bounded-thinking requests, `max-tokens` reserves final-output capacity;
the HTTP generation cap adds `thinking-budget`, and context validation includes
both. llama.cpp receives `reasoning_budget_tokens` and separates reasoning from
final content. A truncated final response is rejected. Each HTTP request is
still capped at 120 seconds; model loading has its own startup timeout.

### `[editor.ai-completion]` Section

Optional automatic inline completion using an OpenAI-compatible chat completion
endpoint, including a local llama.cpp/Junie server. This is disabled by default.
After you pause typing in insert mode, a suggestion appears as grey virtual text.
`Shift-Right` inserts the entire suggestion in one undo step. Moving the cursor,
editing, changing buffers, or leaving insert mode discards it. Dot repeat and
recorded macros replay the accepted text without requesting another suggestion.

The existing LSP, path and word completion menus keep their Tab/Enter behavior.
Accepting an AI suggestion closes an open completion menu without accepting its
candidate. Selecting a menu candidate dismisses the AI preview. With no AI
suggestion, `Shift-Right` inserts nothing and cancels any menu preview.

Set `predict-next = true` to also predict a next line or a short coherent block
after a block opener or on an indented blank line with useful preceding context.
Predictions appear automatically after the same debounce; `Shift-Right` accepts
the whole preview as one undo step. Typing or moving discards pending predictions.
Empty files and ambiguous top-level blank lines do not trigger block predictions.
The separate `prediction-lines` and `prediction-tokens` limits leave ordinary
partial-line completion limits unchanged. Overlong/truncated blocks are rejected
whole. Existing code after the cursor and document line endings are preserved.

Conservative suggestions also receive a bounded background syntax check when a
grammar is available and the document is at most 128 KiB. An ordinary suggestion
cannot turn previously valid syntax into a parse error; next-line/block predictions
must parse without errors. The check includes affected embedded language layers.
Unavailable/oversized syntax checks fall back to output heuristics. It cannot
establish type or semantic correctness, and may hide useful incomplete blocks.

| Key | Description | Default |
| --- | --- | --- |
| `enabled` | Enable automatic AI suggestions | `false` |
| `provider` | Named model from `[editor.ai.providers]`; otherwise use the global default or legacy connection fields | unset |
| `conservative` | Prefer short current-line suggestions and suppress weak contexts and overlong responses; see below | `false` |
| `predict-next` | Automatically predict the next line or a short block at a block opener or an indented blank line with usable context | `false` |
| `prediction-lines` | Maximum lines in one prediction; clamped to 1–12, overlong responses are rejected whole | `6` |
| `prediction-tokens` | Separate output token limit for next-line/block prediction, clamped to 1–256 | `192` |
| `endpoint` | Full HTTP(S) chat completions URL | `"http://127.0.0.1:19239/v1/chat/completions"` |
| `model` | Model name sent to the server | `"Qwen3.8-27B-UD-Q5_K_M"` |
| `model-path` | Optional local GGUF file; when set, Helix starts and manages llama.cpp | unset |
| `server-path` | llama.cpp server executable, used with `model-path` | `"llama-server"` (on PATH) |
| `context-size` | Local server context in tokens, clamped to 512–131072 | `4096` |
| `gpu-layers` | Local server GPU layers, capped at 999; use `0` for CPU inference | `99` |
| `startup-timeout-ms` | Local model loading timeout, clamped to 100–600000 ms | `120000` |
| `prompt-mode` | `insertion` requests missing code directly; `line` requests a complete current line; `gap` requests a short missing fragment with format examples and echo/boundary checks | `"insertion"` |
| `api-key-env` | Optional environment variable containing the bearer token | unset |
| `debounce-ms` | Pause after editing before a request, capped at 5000 ms | `350` |
| `timeout-ms` | Request timeout, clamped to 100–600000 ms | `5000` |
| `max-tokens` | Maximum generated tokens, clamped to 1–512 | `128` |
| `prefix-chars` | Characters before the cursor sent as context, capped at 64000 | `8000` |
| `suffix-chars` | Characters after the cursor sent as context, capped at 16000 | `2000` |

```toml
[editor.ai-completion]
enabled = true
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
model = "Qwen3.8-27B-UD-Q5_K_M"
timeout-ms = 15000
# api-key-env = "HELIX_JUNIE_API_KEY"
```

To load a GGUF directly, configure its path and the installed llama.cpp executable
in your normal `config.toml`. For example, on Windows (single-quoted TOML strings
preserve backslashes):

```toml
[editor.ai-completion]
enabled = true
model = "Qwen3.5-9B-Q6_K"
conservative = true
predict-next = true
prediction-lines = 6
prediction-tokens = 192
model-path = '${USERPROFILE}\.local\share\junie-local\models\Qwen3.5-9B-Q6_K.gguf'
server-path = '${USERPROFILE}\.local\share\junie-local\versions\0.3.5\runtime\llama.cpp-b11081\llama-server.exe'
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
prompt-mode = "gap"
debounce-ms = 350
timeout-ms = 5000
max-tokens = 48
prefix-chars = 3000
suffix-chars = 500
```

Start `hx` normally; a separate launcher is unnecessary. Loading runs in the
background, so editing remains available while the first model loads. Paths
support `~`, `$VAR` and `${VAR}` expansion, including on Windows.
`${USERPROFILE}` uses the current Windows user's home directory, so this example
does not need a different username on each computer. `~/.local/share/...` is
another supported form. Use an existing environment variable: an unset variable
expands to an empty string. `model` is the alias assigned to the local server;
`model-path` selects the actual weights. Adjust `server-path` if your installed
Junie/llama.cpp version differs, or omit it if `llama-server` is on PATH.

Managed local servers require an `http://127.0.0.1:PORT/v1/chat/completions`
endpoint. They use one request slot, Flash Attention and F16 KV cache. A running
server is reused only if it advertises the configured model alias. An occupied
port serving another model is reported without stopping that service. Startup
failures appear in the status line; process output is saved as
`ai-completion/server-PID-PORT.log` under Helix's cache directory.

Set `enabled = false` and run `:config-reload` to stop suggestions and release a
server started by this editor. Changing the model/server settings restarts the
owned server; unrelated editor settings do not. Exiting also stops the owned
server. Reused external servers remain running. With no `model-path`, the
existing external HTTP endpoint behavior is preserved.

Requests send the filename, language and bounded text around the cursor to the
configured endpoint; conservative mode can also include the supporting declarations
and language-server member metadata described below. Requests disable thinking,
use no tools, ignore HTTP proxy
settings and redirects, and stop when the edit context changes. Connection errors
silently leave regular editing available; debug logs contain a short error, not
credentials or model response bodies. Choose context limits that fit your model.
Selections spanning more than one grapheme, multiple cursors, read-only documents
and active snippets suppress suggestions.

`prompt-mode = "line"` can help small chat models that repeat existing code when
asked for an insertion. It suggests at most the remainder of the current line;
the default `insertion` mode supports multiline suggestions. Line mode preserves
the exact text already typed and leaves text to the right of the cursor in place.
These boundary checks do not verify the suggested code's logic.

In conservative mode, Helix also recovers missing enclosing declarations,
Python function docstrings, directly attached declaration comments and imports
from the current file's syntax tree. Unrelated comments, oversized documentation
and documentation already within the nearby prefix are omitted.
For member access (`.`, `->`, `::`,
including partly typed names), it asks the first connected completion language
server for available members after debounce. It waits up to 200ms, cancels with
the edit, and falls back if metadata is unavailable. Only bounded labels/type
details are included; completion snippets, commands and edits are not used.
No additional project files are opened. These source/LSP notes replace older
prefix text within `prefix-chars`, with caps of 600 characters each and 1200
combined; the exact typed current line takes priority. A usable complete small
member list can reject an invented first member name. Incomplete/large/unfamiliar
lists are hints only, and the model can still make incorrect logic/argument choices.

For fewer speculative suggestions, set `conservative = true`. This uses current-
line prompting for `insertion`/`line`, or the explicitly selected `gap` strategy.
Gap requests also use current-line prompting when authoritative language-server
member names are available, while retaining member-name validation. Both strategies
ask the model to abstain when context is ambiguous; gap prompts prioritize the
surrounding function's behavior and edge cases. Conservative mode skips
blank/comment-only lines, single bare identifier characters, punctuation-only
lines and lines ending in a statement/block terminator. C++ namespace access
ending in `::` remains eligible. Comment detection covers leading `//` and leading
`#` in Python, shell, Ruby, PowerShell, TOML and YAML.
It also skips an expression ending directly at the cursor before an existing
semicolon, reducing unwanted extensions of code such as `return value;`.
Consequently some partial identifiers at that boundary will receive no suggestion.
Python block-header colons suppress requests; lambda, slice and dictionary colons
remain eligible. Gap mode rejects repeated typed prefixes/partial words, extra
lines and unbalanced fragments, so it can omit valid complex continuations.
Line mode tolerates omitted leading indentation when all typed code matches.
The recommended profile uses a downloaded [Qwen3.5-9B Q6_K](https://huggingface.co/unsloth/Qwen3.5-9B-GGUF).
The repository includes an executable evaluation recipe in `contrib/ai-completion-eval.md`.

Conservative responses use temperature zero and at most 64 output tokens (or
your smaller `max-tokens` value). Truncated responses, extra nonblank lines,
insertions longer than 160 characters and extra statements after an unquoted
top-level semicolon are discarded. These are conservative heuristics and may
hide useful suggestions; they do not establish semantic correctness or measure
model confidence. Set `conservative = false` to restore the existing prompting
and multiline behavior. A `debounce-ms` of 350 also gives you more time to type
before a request begins.

The theme scope is `ui.virtual.ai-completion`, with a grey fallback. To remap
acceptance if your terminal does not transmit `Shift-Right`:

```toml
[keys.insert]
S-right = "move_char_right"
F4 = "ai_completion_accept"
```

For Windows and an existing Junie Local installation, see
`contrib/helix-junie.md` in the source checkout for a launcher using the installed
model files.

### `[editor.word-completion]` Section

Options for controlling completion of words from open buffers.

| Key                  | Description                                                    | Default  |
| ---                  | ---                                                            | ---      |
| `enable`             | Whether word completion is enabled                             | `true`   |
| `trigger-length`     | Number of word characters to type before triggering completion | `7`      |

Example:

```toml
[editor.word-completion]
enable = true
# Set the trigger length lower so that words are completed more often
trigger-length = 4
```

### `[editor.workspace-trust]` Section

Controls implicit workspace trust. See the [workspace
trust](./workspace-trust.md) chapter for the full feature.

| Key       | Description                                                              | Default     |
| ---       | ---                                                                      | ---         |
| `level`   | The default level of trust for every workspace.                         | `"servers"` |
| `prompt`  | Whether to show a modal when opening a file in an untrusted workspace.   | `true`      |
| `trusted` | Glob patterns whose matching workspaces are trusted without a grant.     | `[]`        |

Example:

```toml
[editor.workspace-trust]
# Even if `false`, the statusline `[⚠]` indicator is still shown.
prompt = false

# "none":     prompt for every workspace.
# "servers":  trust LSP and DAP launches but still gate local config and git;
#             .helix/config.toml, .helix/languages.toml, etc. need :workspace-trust.
# "insecure": trust everything (discouraged).
level = "servers"

# Discouraged: skips .helix/ change detection and trusts anything that lands
# under a matching path. `~` and environment variables are expanded.
trusted = ["~/src/github.com/me/*"]
```


### `[editor.ai-doc]` Section

Generate Chinese reStructuredText documentation for the function at the cursor
with `:ai-doc`. Supports C/C++ definitions and multiline Python functions,
including methods, async functions, decorators and templates. Enable explicitly:

```toml
[editor.ai-doc]
enabled = true
language = "zh-CN"
style = "rest"
timeout-ms = 30000
max-tokens = 768
```

Select a named model with `provider`, or use the default from `[editor.ai]`.
Without either setting, model connection and local server settings continue to
come from `[editor.ai-completion]`. Its `enabled` flag controls automatic
completion only: documentation works with completion disabled. Features sharing
a local provider share its process. No model is downloaded automatically.

| Key | Description | Default |
| --- | --- | --- |
| `enabled` | Allow explicit documentation requests | `false` |
| `provider` | Named model from `[editor.ai.providers]`; otherwise use the global default or legacy connection fields | unset |
| `local-provider` | Managed model to load temporarily when inheriting local mode; ignored online or with an explicit `provider` | unset |
| `language` | Description language: `zh-CN` or `en` | `"zh-CN"` |
| `style` | Documentation style; currently only `rest` | `"rest"` |
| `timeout-ms` | Combined generation and repair timeout; model startup has its separate existing timeout | `30000` |
| `max-tokens` | Configured local output budget, bounded to 128–16384 tokens; known DeepSeek auto uses its provider profile | `768` |

Move into a function and run `:ai-doc`. The preview shows the exact insertion;
Enter accepts, Escape/Ctrl-C cancels, `r` regenerates, and arrows/`j`/`k` or
PageUp/PageDown scroll. Other editing keys do not reach the document. Before the
preview appears, use `:ai-doc-cancel` to cancel an in-flight request. `u` undoes
the whole insertion and `U` redoes the accepted text without another request.
The command does not save the file. A custom normal-mode binding is optional:

```toml
[keys.normal.space]
D = ":ai-doc"
```

The request uses the complete current function, including unsaved changes, and
bounded declarations from the same buffer. Relevant constants, types and helper
definitions are included as complete blocks when they fit the context budget.
Callable facts distinguish ordinary, async and generator behavior. Each generated
description must quote supporting source; quotations are checked against the
source actually sent and are omitted from the final comment. This establishes
source traceability, not proof that a natural-language claim is correct.

Successful generation uses one model request. A complete response that fails
validation can be repaired once using the diagnostic and the same source, within
the combined timeout. Transport failures, tool calls and truncated completions
are not retried automatically. The operation holds one model lease across both
requests. It does not read other project files
or execute model output. A function exceeding the model context budget is
rejected rather than silently truncated. Generated descriptions still need review.
Moving the cursor while generating preserves the original target; editing the
buffer, changing views/buffers or configuration invalidates the request. A result
waits while another prompt/picker is active. Automatic AI suggestions pause until
the operation ends; ordinary completion settings remain independent.

Python receives a first-statement docstring with `:param:`, source-derived
`:type:`/`:rtype:`, `:returns:` and justified `:raises:` fields. C/C++ receives a
`/* ... */` comment containing reST fields; C++ uses `:tparam:` and `:throws:`.
C/C++ documentation extraction depends on your documentation tool configuration;
this feature does not set up a Sphinx pipeline.

Existing Python docstrings or adjacent C/C++ comments are left in place and the
request is skipped. Initial limitations include declarations without bodies,
macro-generated functions, anonymous lambdas, unnamed C/C++ parameters, single-line
Python bodies, and functions sharing a line with other code. Multi-cursor,
active snippets and macro recording/replay are unsupported.

### `[editor.test]` Section

Run existing or AI-generated tests without a model. Python uses pytest (including
`unittest.TestCase`); C/C++ uses a CMake project with tests registered in CTest.
Save project buffers before running. Opening a file only identifies declarations;
it never imports Python modules or builds a CMake project.

The default gutter includes `test`. If you configured your own gutter list, add
it explicitly, keeping folding and diagnostics independent:

```toml
[editor]
gutters = ["diagnostics", "spacer", "line-numbers", "test", "fold", "spacer", "diff"]

[editor.test]
# python = ".venv/Scripts/python.exe" # Unix: .venv/bin/python
# build-directory = "build"
# cmake-preset = "windows-debug"
# build-configuration = "Debug"
# target = "unit_tests"
# timeout-ms = 180000
# cmake-args = ["-DCMAKE_PREFIX_PATH=C:/dependencies"]
```

Unspecified interpreter, build-directory, preset, build-configuration, timeout and
CMake arguments inherit `[editor.ai-test]` for compatibility. Ordinary runs do
not inherit the AI generator's test target. Python selection otherwise checks
project `.venv`/`venv`, `VIRTUAL_ENV`, then `python` on PATH; pytest must be installed
in that interpreter. Existing CMake settings are reused, with `VCPKG_ROOT` used for
new unconfigured builds unless a preset/toolchain overrides it. Ordinary runs do
not edit `CMakeLists.txt` or add dependency downloads. Dependency actions already
configured by the project's CMake scripts still apply.

| Command | Action |
| --- | --- |
| `:test` | Run the test or class at the cursor |
| `:test-file` | Run tests in the current file |
| `:test-project` | Run the current language project's tests |
| `:test-show` | Open/focus the results panel |
| `:test-rerun` | Repeat the last run with its original environment |
| `:test-rerun-failed` | Run only failed/error cases from the previous run |
| `:test-stop` | Cancel execution and stop its owned process tree |

Click `▶` beside a Python test/class or GoogleTest declaration to run it. A
parameterized declaration runs its instances; individual instances can be selected
in the results panel. Status symbols distinguish passed `✓`, failed `✗`, skipped
`−`, error `!`, running `◷` and cancelled `■`. Source edits mark results stale `?`.
Collection/build failures and zero matching tests are reported as errors.

The docked panel has expandable file/suite groups, individual durations and output.
Use `j`/`k` or arrow keys to select, Enter to expand a group or jump to a failure,
`h`/`l` to collapse/expand, Tab to switch tree/details, `r` to rerun the selection,
`f` to rerun failures, `R` to repeat the last scope, `s` to stop, Esc to edit while
keeping results visible, and `q` to close. Mouse toolbar actions are equivalent.
Closing the panel preserves results and does not stop execution. The Git and test
docks replace one another. Narrow terminals use Tab to switch between tree/output.

CMake requires CTest JSON discovery and JUnit output support (CMake 3.21+).
Use `gtest_discover_tests(your_target)` for per-case and per-file actions. CTest
entries that register an entire executable can be run with `:test-project`; a
single-case click never silently expands to the whole executable. CTest environment,
fixtures and working-directory properties remain in effect. When executable targets
have overlapping test names, give each `gtest_discover_tests` a distinct
`TEST_PREFIX`; ambiguous CTest names are rejected before execution. Custom test naming
rules/macros may require file/project commands. V1 retains only the latest run;
debugging, coverage and automatic reruns are deferred.

### `[editor.ai-test]` Section

Generate unit tests for the function or method at the cursor with `:ai-test`.
C/C++ uses GoogleTest; Python uses pytest. Existing docstrings/comments and
single-line function bodies are supported. The source must be saved first.

```toml
[editor.ai-test]
enabled = true
local-provider = "local-27b" # Optional named managed llama.cpp provider.
max-tokens = 4096
timeout-ms = 120000
run-timeout-ms = 180000
allow-fetch-content = true
# provider = "deepseek" # Exact override; omit to follow the global AI switch.
# python = 'D:\project\.venv\Scripts\python.exe'
# cmake-preset = "windows-debug"
# build-directory = "out/build/windows-debug"
# build-configuration = "Debug"
# library-target = "my_library"
# test-target = "existing_tests"
# cmake-args = ["-DVCPKG_TARGET_TRIPLET=x64-windows"]
```

The feature is disabled by default. It inherits `editor.ai.default-provider`;
`local-provider` substitutes a managed local model only when the inherited
provider is llama.cpp. An explicit `provider` wins. Thus completion can remain
on a small resident model while test generation temporarily uses a 27B model
with bounded thinking; switching the global provider to DeepSeek also switches
test generation. A local failure never uploads code to another provider.

Configured local test `max-tokens` accepts 512–16384 final-output tokens. The provider context
must also fit the input, chat overhead and any separate reasoning budget.
Generation requests and individual run stages can wait up to 600000 ms when
configured through `timeout-ms` and `run-timeout-ms` respectively.

Preview shows the exact test and CMake diffs plus inferred behavior assumptions.
Enter accepts all changes into editable buffers; Esc cancels; `r` regenerates.
The source remains unchanged. Save with `:wa!` (the bang allows new test
directories), then use `:ai-test-run`. Each changed file can be undone with `u`
in its buffer. `:ai-test-show` reopens the latest preview/report;
`:ai-test-cancel` cancels generation or the running process tree and preserves
accepted modifications. Changes to source, configuration, supporting files or
unsaved buffers prevent applying/running stale results.

Generated Python files are named `tests/test_helix_<function>_<identity>.py`;
the interpreter comes from `python`, the project's `.venv`/`venv`, the active
virtual environment, or `python` on PATH, in that order. pytest must already be
installed in that environment. Helix does not install Python packages or alter
`sys.path`. Existing fixtures and nearby tests are included when context permits.

CMake integration reuses a discovered library and, when unambiguous, an existing
GoogleTest executable. Set `library-target`/`test-target` for custom macros or
ambiguous ownership. Otherwise a new test target is created under
`tests/helix-ai/<identity>/`, with a guarded root `add_subdirectory` and CTest
enablement. C sources remain compiled as C; the separate test file uses C++17
and must declare C interfaces with correct linkage. Existing GTest targets and
installed packages are preferred; a new build automatically uses the toolchain
under `VCPKG_ROOT` if no preset/toolchain override is supplied. If GTest cannot
be found, `allow-fetch-content = true` permits downloading GoogleTest v1.17.0
with a verified SHA256. Set it to false for installed dependencies only.

Builds default to `.helix-ai/build` and `Debug`, with `BUILD_TESTING=ON`.
Simple inherited configure presets retain their `binaryDir`; presets using
includes/complex macros need an explicit `build-directory`. Build directories
must stay within the project. Only the selected target is built and only the
generated suite is run, including multi-config builds. Missing tools/dependencies,
CMake configure failures and test failures are reported separately. A successful
run requires test collection; it does not establish sufficient coverage.

One automatic repair is allowed for test compile/link or pytest collection/import
errors. It returns to preview, must preserve the original assertions exactly,
and needs acceptance/save before running again. Failed assertions, timeouts,
missing tools and existing-project CMake configuration errors are not silently
rewritten. Repair never modifies business code. Header/linkage setup, public API
accessibility and behavior assumptions still require review.

Initial limitations: repository-wide generation, inaccessible private/static or
anonymous-namespace functions, restructuring executable targets into libraries,
automatic fixture/plugin installation and complex cross-compilation. Non-CMake
C/C++ projects receive a test draft without automatic build integration.
