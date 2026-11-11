# Configure local AI completion in config.toml

The user wants the existing inline AI completion to be enabled/disabled and its
local GGUF model selected in the normal Helix `config.toml`. The supplied model is
`C:\Users\wangshuo\.local\share\junie-local\models\Qwen2.5-Coder-7B-Instruct-Q6_K.gguf`.

Extend `[editor.ai-completion]` with optional `model-path` and `server-path`
(defaults to `llama-server` on PATH), `context-size = 4096`, `gpu-layers = 99`, and
`startup-timeout-ms = 120000`. `model` remains the HTTP model alias. An absent
`model-path` preserves the current external-endpoint behavior. `enabled = false`
starts no process. With a model path, Helix starts llama.cpp asynchronously at
startup, binds only to the configured IPv4 loopback endpoint, waits for readiness,
and then permits completion requests. Do not block typing while loading.

## Approaches

- Extend only the launcher: smaller change but still requires a separate startup
  command, so it does not fulfill ordinary `hx` startup from `config.toml`.
- Embed inference: requires new native dependencies and duplicates the installed
  llama.cpp runtime.
- Manage the installed llama.cpp server in Helix: selected implementation choice;
  reuse the HTTP client, do not add dependencies, and keep process ownership clear.

The application owns the supervisor task. Configuration changes cancel loading
and clear stale suggestions; disabling, changing the model, or exiting releases
the owned server. Reuse a compatible external server without stopping it; reject
occupied ports serving other models. Record server output in the Helix cache.
Errors must leave regular completion/editing usable and provide a useful status.

Preserve the existing line/insertion modes and Shift+Right behavior. Configure the
user's model with line mode and 48 output tokens. Preserve other user preferences.
The user additionally requested uninstalling Scoop Helix, putting the new build
directory on their user PATH, preserving the current theme and runtime assets,
and pushing the finished changes to `https://github.com/KevinShuo/helix`.

Back up the Scoop package, current config and user PATH before migration. Bundle
the complete runtime beside `target/release/hx.exe`; preserve `gruvbox_dark_soft`
and its `gruvbox` parent as user themes. Verify the new build before uninstalling
Scoop. Update the normal user config with the supplied Q6_K model, then verify
PATH, runtime discovery and theme/config loading. Reuse this checkout and publish
the verified source changes to the user's repository after local setup succeeds.
