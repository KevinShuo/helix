# Configuration

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

Local-model inline suggestions can be enabled with
[`[editor.ai-completion]`](./editor.md#editorai-completion-section).
They appear as grey text and are accepted in full with `Shift-Right` in insert mode.

Function reference counts and Git authors can be enabled independently in
[`[editor.function-hints]`](./editor.md#editorfunction-hints-section).


A right-side source overview can be enabled with
[`[editor.minimap]`](./editor.md#editorminimap-section). It supports Braille or block
rendering and the session-only `:toggle-option minimap.enabled` command.
