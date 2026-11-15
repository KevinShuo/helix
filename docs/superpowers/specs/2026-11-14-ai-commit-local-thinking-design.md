# On-demand local documentation and commit analysis

Approved behavior: global `default-provider = "local"` keeps 9B completion,
while AI documentation and commit analysis temporarily use the installed Qwen3.8
27B GGUF with bounded thinking. Global `deepseek` routes every feature online.
Explicit feature `provider` remains an exact override. Optional
`ai-doc.local-provider` and `ai-commit.local-provider` apply only when the inherited
provider is llama-cpp. Both reference the same 27B configuration.

Thinking providers expose `thinking-budget` and `reasoning-effort`; commit output
limits reserve both reasoning and final JSON tokens. Requests extract reasoning
separately and validate final JSON as before. The local setup uses medium effort,
2048 reasoning tokens, 1024 final tokens, 16384 context and a 120-second HTTP
timeout. Other providers keep existing limits and parameters.

The conditional commit provider is resolved as on-demand at runtime and is not
loaded at editor startup. A cancellable lease suspends resident managed models,
awaits their process exit, loads 27B for the whole multi-request operation, then
unloads it and restores the latest configured residents. Success, provider error,
cancel, config reload and editor shutdown must all release the model. Never kill
an externally owned server; report an actionable error if it prevents switching.

Do not modify staging/commit semantics. Preserve legacy configuration, global
switching and explicit provider overrides. Preserve user settings outside AI.

Alternatives: pinning commit.provider breaks global switching; loading 9B and 27B
simultaneously risks exhausting the user's 24-GiB GPU. The user selected temporary
model switching and restoration.
