param(
    [string]$InstallRoot = "$env:LOCALAPPDATA\LSENext"
)

$ErrorActionPreference = "Stop"

Write-Host "Removing LSENext installed binaries from $InstallRoot"
if (Test-Path $InstallRoot) {
    Get-ChildItem -LiteralPath $InstallRoot -Directory | Remove-Item -Recurse -Force
}

Write-Host "User state in $env:LOCALAPPDATA\LSENext\state.json is intentionally preserved for v0.0.1."
