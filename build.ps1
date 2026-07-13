$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Write-Host "Building usageShow..." -ForegroundColor Cyan

if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    $signingKeyPath = if ($env:TAURI_SIGNING_PRIVATE_KEY_PATH) {
        $env:TAURI_SIGNING_PRIVATE_KEY_PATH
    } else {
        Join-Path $HOME ".tauri\usage-show.key"
    }

    if (-not (Test-Path -LiteralPath $signingKeyPath -PathType Leaf)) {
        throw "Updater signing key not found. Set TAURI_SIGNING_PRIVATE_KEY or point TAURI_SIGNING_PRIVATE_KEY_PATH to the key file before building."
    }

    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw -LiteralPath $signingKeyPath
}

if (-not (Test-Path Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
}

npx tauri build

$exe = Join-Path $PSScriptRoot "src-tauri\target\release\tauri-app.exe"
if (-not (Test-Path $exe)) {
    throw "Build finished but exe not found: $exe"
}

$sizeMB = [math]::Round((Get-Item $exe).Length / 1MB, 2)
Write-Host ""
Write-Host "Done: $exe ($sizeMB MB)" -ForegroundColor Green
