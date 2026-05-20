param(
    [string]$InstallRoot = "$env:LOCALAPPDATA\LSENext"
)

$ErrorActionPreference = "Stop"

Get-ChildItem -LiteralPath $InstallRoot -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $helper = Join-Path $_.FullName "lsenext-helper.exe"
    if (Test-Path $helper) {
        & $helper unregister-package
    }
}

Write-Host "Removing LSENext installed binaries from $InstallRoot"
if (Test-Path $InstallRoot) {
    Get-ChildItem -LiteralPath $InstallRoot -Directory | Remove-Item -Recurse -Force
}

Write-Host "User state in $env:LOCALAPPDATA\LSENext\state.json is intentionally preserved."
