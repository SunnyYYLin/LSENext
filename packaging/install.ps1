param(
    [ValidateSet("x64", "arm64")]
    [string]$Architecture = "x64",
    [string]$InstallRoot = "$env:LOCALAPPDATA\LSENext"
)

$ErrorActionPreference = "Stop"
$source = Join-Path $PSScriptRoot "..\dist\$Architecture"
$target = Join-Path $InstallRoot $Architecture

if (-not (Test-Path $source)) {
    throw "Build output not found: $source"
}

New-Item -ItemType Directory -Force -Path $target | Out-Null
Copy-Item -Force -Path (Join-Path $source "*") -Destination $target

& (Join-Path $target "lsenext-helper.exe") register-package

Write-Host "LSENext files installed to $target"
Write-Host "LSENext sparse package identity registered for Windows 11 native context menus."
