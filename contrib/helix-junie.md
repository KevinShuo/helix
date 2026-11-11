# Helix + Junie Local on Windows

This checkout supports grey AI suggestions while typing. Keep using Tab/Enter
for the normal completion menu; press **Shift+Right** to accept the entire grey
suggestion. Escape or cursor movement discards it. AI completion is off in the
default editor configuration.

## Start with Qwen2.5-Coder 7B Instruct

Requires PowerShell 7, a built `target/release/hx.exe`, and the existing Junie
installation at `%USERPROFILE%\.local\share\junie-local`.

```powershell
cd C:\dev\helix
pwsh -File .\contrib\helix-junie.ps1 example.py
```

The launcher starts a localhost-only llama.cpp server on port 19240 using
`models\Qwen2.5-Coder-7B-Instruct-Q5_K_M.gguf` in the Junie installation. It uses
`-GpuLayers 99` to offload all layers, a 4096-token context, one request slot and
F16 KV cache. On this machine's RTX 4090, the previous Q4 KV cache produced empty
or malformed suggestions with this model; the same prompts returned correct
code with F16. The model weights remain Q5_K_M. Junie's saved inference settings
are unchanged.

Suggestions wait for a 200 ms typing pause, use up to 3000 characters before the
cursor and 500 after it, and generate at most 48 tokens. The request timeout is
5 seconds. The Instruct model uses the existing chat completion interface with
`prompt-mode = "line"`. It generates a completed current line, and Helix extracts
only the missing text. Replies that rewrite the typed prefix or omit existing
text to the right are discarded. This avoids inserting repeated identifiers or
an unrelated multiline block inside a condition. It does not verify the logic of
the proposed condition. Use `-PromptMode insertion` for direct, potentially
multiline suggestions; the 27B profile keeps that mode by default.

The launcher stops a server it started when Helix exits. A compatible server
already running on the selected port is reused and left running. A conflicting
service is never stopped; choose another `-Port` instead. Startup logs are under
`%LOCALAPPDATA%\helix\ai-completion`.

Your normal `%APPDATA%\helix\config.toml` is copied into a temporary session
configuration, with AI completion enabled. The original file is not modified.
If it already contains an AI configuration, that configuration is used unchanged;
ensure its endpoint/model match the server. `-ConfigPath path\config.toml` also
uses your chosen configuration unchanged. To keep settings across launches, edit
your original configuration; edits to the temporary session copy are discarded.

Check model connectivity with a synthetic Python prompt:

```powershell
pwsh -File .\contrib\helix-junie.ps1 -Check
```

To use Junie's existing gateway instead of a separate inference process:

```powershell
pwsh -File .\contrib\helix-junie.ps1 -UseGateway example.py
```

This continues to use `Qwen3.8-27B-UD-Q5_K_M` by default. It invokes Junie's
existing `start-qwen38.ps1 -Check` and reads its API key into
`HELIX_JUNIE_API_KEY` for the launched editor only. The key is never written to
the generated TOML. Junie's gateway must have enough VRAM for its saved settings;
on this machine its full offload configuration did not fit the available VRAM.

To use the original 27B model with the earlier direct-server settings:

```powershell
pwsh -File .\contrib\helix-junie.ps1 -Model Qwen3.8-27B-UD-Q5_K_M -GpuLayers 60 -ContextSize 8192 -CacheType q4_0 example.py
```

## Configure manually

Add this to your Helix config when using a server already listening on 19240:

```toml
[editor.ai-completion]
enabled = true
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
model = "Qwen2.5-Coder-7B-Instruct-Q5_K_M"
prompt-mode = "line"
debounce-ms = 200
timeout-ms = 5000
max-tokens = 48
prefix-chars = 3000
suffix-chars = 500
```

For the gateway, use its configured port (normally 19239), set
`api-key-env = "HELIX_JUNIE_API_KEY"`, and supply that environment variable.
Never put the key in this repository. See the book's `[editor.ai-completion]`
section for all options and key remapping.

## Build and verify

```powershell
cargo build --release --locked -p helix-term
cargo integration-test ai_completion
```

If grammar fetching fails because a grammar host is unavailable, use the
existing build opt-out and separately build already-fetched grammars:

```powershell
$env:HELIX_DISABLE_AUTO_GRAMMAR_BUILD = '1'
cargo build --release --locked -p helix-term
# Run from the workspace root so Cargo supplies the grammar runtime directory.
cargo run --release --locked -p helix-term -- --grammar build
```

The optional live-model editor test uses a running model and exercises automatic
preview plus Shift+Right through the editor event loop:

```powershell
$env:HELIX_TEST_AI_ENDPOINT = 'http://127.0.0.1:19240/v1/chat/completions'
$env:HELIX_TEST_AI_MODEL = 'Qwen2.5-Coder-7B-Instruct-Q5_K_M'
$env:HELIX_TEST_AI_PROMPT_MODE = 'line'
cargo integration-test ai_completion_live_local_model -- --ignored --nocapture
```

For the reported C++ condition case, set `HELIX_TEST_AI_CPP_FILE` to a local
header containing `while (smallestCarOfNumber...)` and run the ignored
`ai_completion_live_cpp_line` test. It operates on a disposable copy and checks
that accepting the suggestion preserves both sides of the cursor.
