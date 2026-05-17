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

Write-Host "LSENext files installed to $target"
Write-Host "MSIX/sparse package registration is prepared in packaging\AppxManifest.xml."
Write-Host "For v0.0.1 dev validation, register the package manifest with Add-AppxPackage -Register after signing/package preparation."
