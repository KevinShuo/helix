#requires -Version 7.0
<#
.SYNOPSIS
Starts this checkout's Helix with local Qwen inline completion.
.EXAMPLE
pwsh -File .\contrib\helix-junie.ps1 example.py
.EXAMPLE
pwsh -File .\contrib\helix-junie.ps1 -Check
.EXAMPLE
pwsh -File .\contrib\helix-junie.ps1 -UseGateway example.py
#>
[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$JunieRoot = (Join-Path $env:USERPROFILE '.local\share\junie-local'),
    [string]$HelixPath = (Join-Path $PSScriptRoot '..\target\release\hx.exe'),
    [ValidatePattern('^[A-Za-z0-9_.-]+$')][string]$Model = 'Qwen3.8-27B-UD-Q5_K_M',
    [ValidateRange(1024, 65535)][int]$Port = 19240,
    [ValidateRange(0, 200)][int]$GpuLayers = 60,
    [ValidateRange(2048, 32768)][int]$ContextSize = 8192,
    [string]$ConfigPath,
    [switch]$UseGateway,
    [switch]$Check,
    [Parameter(Position = 0, ValueFromRemainingArguments = $true)][string[]]$Files
)

$ErrorActionPreference = 'Stop'
$ownedServer = $null
$temporaryConfig = $null
$previousRuntime = $env:HELIX_RUNTIME
$previousKey = $env:HELIX_JUNIE_API_KEY
$logDirectory = Join-Path $env:LOCALAPPDATA 'helix\ai-completion'
$headers = @{}

function Get-LocalModels([string]$BaseUrl) {
    Invoke-RestMethod -Uri "$BaseUrl/v1/models" -Headers $headers -TimeoutSec 2 -NoProxy
}

try {
    if (-not $Check -and -not (Test-Path -LiteralPath $HelixPath -PathType Leaf)) {
        throw 'Build the editor first: cargo build --release --locked -p helix-term'
    }
    if ($UseGateway) {
        $settings = Get-Content -Raw -LiteralPath (Join-Path $JunieRoot 'server-config.json') | ConvertFrom-Json
        $baseUrl = "http://127.0.0.1:$($settings.port)"
        $env:HELIX_JUNIE_API_KEY = $settings.api_key
        $headers.Authorization = "Bearer $($settings.api_key)"
        # Use the existing Junie startup command without changing its configuration.
        & (Join-Path $PSHOME 'pwsh.exe') -NoProfile -File (Join-Path $JunieRoot 'start-qwen38.ps1') -Check
        if ($LASTEXITCODE -ne 0) { throw 'Junie gateway startup failed.' }
    } else {
        $baseUrl = "http://127.0.0.1:$Port"
        $models = $null
        try { $models = Get-LocalModels $baseUrl } catch { }
        if ($null -eq $models) {
            # Check the socket first: never try to replace another service on this port.
            $socket = [System.Net.Sockets.TcpClient]::new()
            try {
                $portOccupied = $socket.ConnectAsync('127.0.0.1', $Port).Wait(500)
            } catch { $portOccupied = $false } finally { $socket.Dispose() }
            if ($portOccupied) { throw "Port $Port is in use but is not a ready model server. Choose -Port." }

            $modelPath = Join-Path $JunieRoot "models\$Model.gguf"
            if (-not (Test-Path -LiteralPath $modelPath -PathType Leaf)) { throw "Model not found: $modelPath" }
            $engine = Get-ChildItem -LiteralPath (Join-Path $JunieRoot 'versions') -Filter 'llama-server.exe' -File -Recurse |
                Sort-Object LastWriteTime -Descending | Select-Object -First 1
            if (-not $engine) { throw 'No llama-server.exe found in the Junie installation.' }
            $null = New-Item -ItemType Directory -Path $logDirectory -Force
            $arguments = @('--model', "`"$modelPath`"", '--alias', "`"$Model`"", '--host', '127.0.0.1',
                '--port', $Port, '--ctx-size', $ContextSize, '--n-gpu-layers', $GpuLayers,
                '--parallel', '1', '--flash-attn', 'on', '--cache-type-k', 'q4_0', '--cache-type-v', 'q4_0', '--jinja')
            $logName = "model-$PID"
            $ownedServer = Start-Process -FilePath $engine.FullName -ArgumentList $arguments -WindowStyle Hidden -PassThru `
                -RedirectStandardOutput (Join-Path $logDirectory "$logName.out.log") `
                -RedirectStandardError (Join-Path $logDirectory "$logName.err.log")
            Write-Host "Loading $Model on localhost:$Port..."
            $deadline = [DateTime]::UtcNow.AddSeconds(120)
            do {
                if ($ownedServer.HasExited) { throw "Model startup failed. See $logDirectory\$logName.err.log; try a lower -GpuLayers value." }
                try { $models = Get-LocalModels $baseUrl } catch { }
                if ($models) { break }
                Start-Sleep -Milliseconds 500
            } while ([DateTime]::UtcNow -lt $deadline)
            if (-not $models) { throw "Model startup timed out. See $logDirectory\$logName.err.log" }
        }
        if ($Model -notin @($models.data.id)) { throw "The service on port $Port does not serve model $Model. Choose -Port." }
    }

    $endpoint = "$baseUrl/v1/chat/completions"
    if ($Check) {
        $body = @{
            model = $Model; stream = $false; max_tokens = 48; temperature = 0.1
            enable_thinking = $false; chat_template_kwargs = @{ enable_thinking = $false }
            messages = @(
                @{ role = 'system'; content = 'Complete source code. Return only the missing insertion, no Markdown or explanation. Do not repeat the prefix or suffix.' },
                @{ role = 'user'; content = "<PREFIX>`ndef add(a, b):`n    <CURSOR>`n<SUFFIX>`n`n</SUFFIX>" }
            )
        } | ConvertTo-Json -Depth 6
        $timer = [Diagnostics.Stopwatch]::StartNew()
        $reply = Invoke-RestMethod -Uri $endpoint -Headers $headers -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 30 -NoProxy
        if (-not $reply.choices[0].message.content) { throw 'The model returned no completion.' }
        Write-Host ("Model check passed in {0:N2}s: {1}" -f $timer.Elapsed.TotalSeconds, $reply.choices[0].message.content)
        return
    }

    if (-not $ConfigPath) {
        $userConfig = Join-Path $env:APPDATA 'helix\config.toml'
        $configText = if (Test-Path -LiteralPath $userConfig) { Get-Content -Raw -LiteralPath $userConfig } else { '' }
        if ($configText -match 'ai-completion\s*(?:[.\]"'']|=)') {
            # A custom AI configuration is authoritative; don't rewrite TOML with regexes.
            $ConfigPath = $userConfig
            Write-Host "Using your existing AI configuration in $userConfig (server: $endpoint)."
        } else {
            $keySetting = if ($UseGateway) { 'api-key-env = "HELIX_JUNIE_API_KEY"' } else { '' }
            $configText += @"

[editor.ai-completion]
enabled = true
endpoint = "$endpoint"
model = "$Model"
$keySetting
debounce-ms = 350
timeout-ms = 15000
max-tokens = 96
prefix-chars = 6000
suffix-chars = 1000
"@
            $temporaryConfig = Join-Path ([IO.Path]::GetTempPath()) "helix-ai-$([Guid]::NewGuid()).toml"
            [IO.File]::WriteAllText($temporaryConfig, $configText)
            $ConfigPath = $temporaryConfig
        }
    }
    $env:HELIX_RUNTIME = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\runtime'))
    & $HelixPath -c $ConfigPath @Files
    if ($LASTEXITCODE -ne 0) { throw "Helix exited with code $LASTEXITCODE." }
} finally {
    # Only stop the process this invocation created. Existing servers remain running.
    if ($ownedServer -and -not $ownedServer.HasExited) { $ownedServer.Kill(); $ownedServer.WaitForExit() }
    if ($temporaryConfig -and (Test-Path -LiteralPath $temporaryConfig)) { Remove-Item -LiteralPath $temporaryConfig }
    $env:HELIX_RUNTIME = $previousRuntime
    $env:HELIX_JUNIE_API_KEY = $previousKey
}
