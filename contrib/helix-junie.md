# Helix + Junie Local on Windows

This checkout supports grey AI suggestions while typing. Keep using Tab/Enter
for the normal completion menu; press **Shift+Right** to accept the entire grey
suggestion. Escape or cursor movement discards it. AI completion is off in the
default editor configuration.

## Start with the existing Qwen model

Requires PowerShell 7, a built `target/release/hx.exe`, and the existing Junie
installation at `%USERPROFILE%\.local\share\junie-local`.

```powershell
cd C:\dev\helix
pwsh -File .\contrib\helix-junie.ps1 example.py
```

The launcher starts a localhost-only llama.cpp server on port 19240 using the
existing `Qwen3.8-27B-UD-Q5_K_M.gguf`. It uses 60 GPU layers, an 8192-token context,
one request slot and Q4 KV cache. These settings were tested on this machine's
RTX 4090 while other applications occupied part of its VRAM. It does not change
Junie's saved inference settings. If memory is tight, pass `-GpuLayers 48`.

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

This invokes Junie's existing `start-qwen38.ps1 -Check` and reads its API key into
`HELIX_JUNIE_API_KEY` for the launched editor only. The key is never written to
the generated TOML. Junie's gateway must have enough VRAM for its saved settings;
on this machine its full offload configuration did not fit the available VRAM.

## Configure manually

Add this to your Helix config when using a server already listening on 19240:

```toml
[editor.ai-completion]
enabled = true
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
model = "Qwen3.8-27B-UD-Q5_K_M"
debounce-ms = 350
timeout-ms = 15000
max-tokens = 96
prefix-chars = 6000
suffix-chars = 1000
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
cargo integration-test ai_completion_live_local_model -- --ignored --nocapture
```
