param(
    [ValidateSet("x64", "arm64")]
    [string]$Architecture = "x64",
    [string]$Configuration = "release",
    [string]$ReleaseTag = "v0.0.1"
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
Copy-Item -Force -Path (Join-Path $repo "packaging\AppxManifest.xml") -Destination $dist

$assets = Join-Path $dist "Assets"
New-Item -ItemType Directory -Force -Path $assets | Out-Null
Add-Type -AssemblyName System.Drawing

function Invoke-WindowsSdkTool {
    param(
        [string]$Name,
        [string[]]$Arguments
    )

    $tool = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $tool) {
        $tool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter $Name -ErrorAction SilentlyContinue |
            Select-Object -First 1
    }
    if (-not $tool) {
        throw "Unable to locate $Name in the Windows SDK"
    }

    & $tool.FullName @Arguments
}

function New-LSENextLogo {
    param(
        [string]$Path,
        [int]$Size
    )
    $bitmap = New-Object System.Drawing.Bitmap $Size, $Size
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.Clear([System.Drawing.Color]::FromArgb(0, 0, 0, 0))
        $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(0, 120, 212))
        $graphics.FillRectangle($brush, 0, 0, $Size, $Size)
        $brush.Dispose()
        $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), ([Math]::Max(2, [int]($Size / 11)))
        $margin = [int]($Size * 0.24)
        $graphics.DrawLine($pen, $margin, [int]($Size * 0.42), [int]($Size * 0.58), [int]($Size * 0.42))
        $graphics.DrawLine($pen, [int]($Size * 0.42), [int]($Size * 0.58), [int]($Size - $margin), [int]($Size * 0.58))
        $pen.Dispose()
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}
New-LSENextLogo -Path (Join-Path $assets "StoreLogo.png") -Size 50
New-LSENextLogo -Path (Join-Path $assets "Square150x150Logo.png") -Size 150
New-LSENextLogo -Path (Join-Path $assets "Square44x44Logo.png") -Size 44

$identityRoot = Join-Path $dist "identity-package"
if (Test-Path $identityRoot) {
    Remove-Item -Recurse -Force $identityRoot
}
New-Item -ItemType Directory -Force -Path $identityRoot | Out-Null
Copy-Item -Force -Path (Join-Path $repo "packaging\AppxManifest.xml") -Destination $identityRoot

$identityPackage = Join-Path $dist "LSENext.identity.msix"
if (Test-Path $identityPackage) {
    Remove-Item -Force $identityPackage
}
Invoke-WindowsSdkTool "MakeAppx.exe" @("pack", "/o", "/nv", "/d", $identityRoot, "/p", $identityPackage)
if ($LASTEXITCODE -ne 0) {
    throw "MakeAppx failed for LSENext identity package"
}

$cert = New-SelfSignedCertificate `
    -Type Custom `
    -Subject "CN=LSENext" `
    -KeyUsage DigitalSignature `
    -KeyAlgorithm RSA `
    -KeyLength 2048 `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -KeyExportPolicy Exportable
$certPath = Join-Path $dist "LSENext.cer"
$pfxPath = Join-Path $env:TEMP "LSENext-$Architecture.pfx"
$password = ConvertTo-SecureString "LSENextPackageSigning" -AsPlainText -Force
Export-Certificate -Cert $cert -FilePath $certPath | Out-Null
Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $password | Out-Null
Invoke-WindowsSdkTool "SignTool.exe" @("sign", "/fd", "SHA256", "/f", $pfxPath, "/p", "LSENextPackageSigning", $identityPackage)
if ($LASTEXITCODE -ne 0) {
    throw "SignTool failed for LSENext identity package"
}
Remove-Item -Force $pfxPath
Remove-Item -Recurse -Force $identityRoot
Remove-Item -Path "Cert:\CurrentUser\My\$($cert.Thumbprint)" -Force

$artifactVersion = $ReleaseTag.TrimStart("v")
$zip = Join-Path $repo "artifacts\LSENext-$artifactVersion-$Architecture.zip"
New-Item -ItemType Directory -Force -Path (Split-Path $zip) | Out-Null
if (Test-Path $zip) {
    Remove-Item -Force $zip
}
Compress-Archive -Path (Join-Path $dist "*") -DestinationPath $zip
Write-Host "Created $zip"

$msi = Join-Path $repo "artifacts\LSENext-$artifactVersion-$Architecture.msi"
if (Test-Path $msi) {
    Remove-Item -Force $msi
}
dotnet tool run wix extension add WixToolset.UI.wixext/5.0.2 | Out-Null
dotnet tool run wix build `
    (Join-Path $repo "packaging\LSENext.wxs") `
    -ext WixToolset.UI.wixext `
    -arch $Architecture `
    -d "SourceDir=$dist" `
    -out $msi
if ($LASTEXITCODE -ne 0) {
    throw "wix build failed for $Architecture"
}
Write-Host "Created $msi"
