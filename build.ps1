$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Write-Host "Building usageShow..." -ForegroundColor Cyan
npm run tauri build

$exe = Join-Path $PSScriptRoot "src-tauri\target\release\tauri-app.exe"
if (-not (Test-Path $exe)) {
    throw "Build finished but exe not found: $exe"
}

$sizeMB = [math]::Round((Get-Item $exe).Length / 1MB, 2)
Write-Host ""
Write-Host "Done: $exe ($sizeMB MB)" -ForegroundColor Green