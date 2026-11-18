# Portable Windows language configuration

`languages.windows-portable.toml` is the user's complete configuration with
machine-specific user directories replaced by `${env:LOCALAPPDATA}`,
`${env:APPDATA}`, and `${env:USERPROFILE}`. It needs an hx build containing the
language environment expansion change; stock Helix does not expand these LSP
settings.

1. Use the updated `hx.exe` from this checkout's `target/release` on each computer.
   Keep the usual Helix runtime/grammars available.
2. Back up `%APPDATA%\helix\languages.toml`, then copy
   `languages.windows-portable.toml` to that location as `languages.toml`.
3. Install the same language tools on each computer. The file expects managed web
   tools under `%LOCALAPPDATA%\helix\language-servers`, the Tailwind wrapper at
   `%APPDATA%\helix\tailwind-lsp.cjs`, and Scoop CMake under
   `%USERPROFILE%\scoop\apps\cmake\current`. Environment expansion does not
   install or copy those tools. Existing Vue and ESLint version choices are kept.
4. Restart hx. Run `hx --health cmake` to check the configured server. After
   `include(FetchContent)`, `Fetch` should offer the module functions.

For a different CMake installation layout, set a user environment variable such
as `HELIX_CMAKE_PREFIX` to that machine's CMake installation directory and change
the shared configuration once to:

```toml
[language-server.neocmakelsp]
command = "neocmakelsp"
args = ["stdio"]
environment = { MSYSTEM_PREFIX = "${env:HELIX_CMAKE_PREFIX}" }
```

Use the directory above `share` (not `share/cmake-<version>`). Restart the terminal
or desktop app that launches hx after changing Windows environment variables.
