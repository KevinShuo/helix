# Unified AI providers

Approved direction: named providers under `[editor.ai.providers.NAME]`, with
`editor.ai.default-provider` and an optional `provider` on ai-completion, ai-doc,
and ai-commit. Selection precedence is feature override, global default, legacy
ai-completion connection settings. Explicit unknown references fail, never fall
back. Existing configurations and disabled-by-default behavior remain compatible.

Provider types are `llama-cpp`, `deepseek`, and `openai-compatible`. Connection,
authentication, context and managed-process settings belong to AiProviderConfig;
feature prompts and limits remain feature-specific. DeepSeek disables thinking
using its own API field; generic endpoints receive no llama.cpp extension fields.
Keys are read from environment variables, never stored in TOML or error output.
Proxy modes are `system` and `none`; legacy connections retain `none`.

Keep the existing bounded, non-streaming transport and response validation.
Reuse clients by proxy mode. Managed local servers are owned by the application,
shared by features selecting the same provider, and stopped on reload/shutdown.
Multiple active local providers require distinct endpoints. Remote providers
never start local processes. Configuration changes invalidate pending results.

Alternatives: a minimal shared endpoint change is smaller but cannot select a
model per feature; separate clients per feature duplicate authentication and
transport. The user selected unified provider configuration.

Verification covers legacy defaults, precedence and bad references, local/cloud
mixing, DeepSeek/generic/local wire formats, errors and bounded responses,
multiple local servers and configuration reload. Real paid API calls require a
user-provided key and are outside automated verification.
