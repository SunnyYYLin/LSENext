param(
    [ValidateSet("x64", "arm64")]
    [string]$Architecture = "x64",
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetTriple = if ($Architecture -eq "arm64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" }
$profileDir = if ($Configuration -eq "release") { "release" } else { "debug" }
$dist = Join-Path $repo "dist\$Architecture"

if ($Configuration -eq "release") {
    cargo build --workspace --target $targetTriple --release
} else {
    cargo build --workspace --target $targetTriple
}
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed for $targetTriple"
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
$helper = Join-Path $repo "target\$targetTriple\$profileDir\lsenext-helper.exe"
$shell = Join-Path $repo "target\$targetTriple\$profileDir\lsenext_shell.dll"
if (-not (Test-Path $helper)) { throw "Missing build output: $helper" }
if (-not (Test-Path $shell)) { throw "Missing build output: $shell" }

Copy-Item -Force -Path $helper -Destination $dist
Copy-Item -Force -Path $shell -Destination (Join-Path $dist "lsenext-shell.dll")
Copy-Item -Force -Recurse -Path (Join-Path $repo "resources") -Destination $dist
Copy-Item -Force -Recurse -Path (Join-Path $repo "packaging") -Destination $dist

$zip = Join-Path $repo "artifacts\LSENext-v0.0.1-$Architecture.zip"
New-Item -ItemType Directory -Force -Path (Split-Path $zip) | Out-Null
if (Test-Path $zip) {
    Remove-Item -Force $zip
}
Compress-Archive -Path (Join-Path $dist "*") -DestinationPath $zip
Write-Host "Created $zip"

$msi = Join-Path $repo "artifacts\LSENext-v0.0.1-$Architecture.msi"
if (Test-Path $msi) {
    Remove-Item -Force $msi
}
dotnet tool run wix build `
    (Join-Path $repo "packaging\LSENext.wxs") `
    -arch $Architecture `
    -d "SourceDir=$dist" `
    -out $msi
if ($LASTEXITCODE -ne 0) {
    throw "wix build failed for $Architecture"
}
Write-Host "Created $msi"
