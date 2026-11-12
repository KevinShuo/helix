## Editor

- [`[editor]` Section](#editor-section)
- [`[editor.clipboard-provider]` Section](#editorclipboard-provider-section)
- [`[editor.statusline]` Section](#editorstatusline-section)
- [`[editor.lsp]` Section](#editorlsp-section)
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
| `gutters` | Gutters to display: Available are `diagnostics`, `diff`, `line-numbers`, `spacer`, `code-action-hint` and `fold`. `diagnostics` also includes other features like breakpoints. A 1-width padding will be inserted if gutters is non-empty. | `["diagnostics", "spacer", "line-numbers", "spacer", "diff"]` |
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
folds whose syntax is no longer valid. Reload and language changes clear folds.
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
| `layout` | A vector of gutters to display | `["diagnostics", "spacer", "line-numbers", "spacer", "diff"]` |

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

| Key | Description | Default |
| --- | --- | --- |
| `enabled` | Enable automatic AI suggestions | `false` |
| `endpoint` | Full HTTP(S) chat completions URL | `"http://127.0.0.1:19239/v1/chat/completions"` |
| `model` | Model name sent to the server | `"Qwen3.8-27B-UD-Q5_K_M"` |
| `model-path` | Optional local GGUF file; when set, Helix starts and manages llama.cpp | unset |
| `server-path` | llama.cpp server executable, used with `model-path` | `"llama-server"` (on PATH) |
| `context-size` | Local server context in tokens, clamped to 512–131072 | `4096` |
| `gpu-layers` | Local server GPU layers, capped at 999; use `0` for CPU inference | `99` |
| `startup-timeout-ms` | Local model loading timeout, clamped to 100–600000 ms | `120000` |
| `prompt-mode` | `insertion` requests missing code directly; `line` requests a complete current line and extracts only the missing part, rejecting responses that change the typed prefix or omit the existing line suffix | `"insertion"` |
| `api-key-env` | Optional environment variable containing the bearer token | unset |
| `debounce-ms` | Pause after editing before a request, capped at 5000 ms | `350` |
| `timeout-ms` | Request timeout, clamped to 100–120000 ms | `5000` |
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
model = "Qwen2.5-Coder-7B-Instruct-Q6_K"
model-path = '${USERPROFILE}\.local\share\junie-local\models\Qwen2.5-Coder-7B-Instruct-Q6_K.gguf'
server-path = '${USERPROFILE}\.local\share\junie-local\versions\0.3.5\runtime\llama.cpp-b11081\llama-server.exe'
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
prompt-mode = "line"
debounce-ms = 200
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

Only the filename, language and bounded text around the cursor are sent to the
configured endpoint. Requests disable thinking, use no tools, ignore HTTP proxy
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
