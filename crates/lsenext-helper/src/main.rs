#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{bail, Context, Result};
use lsenext_core::{clear_state, create_link, load_state, save_sources, LinkKind};
use std::env;
#[cfg(feature = "diagnostics")]
use std::fs;
#[cfg(feature = "diagnostics")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(feature = "diagnostics")]
use std::process::Stdio;
#[cfg(feature = "diagnostics")]
use std::thread;
#[cfg(feature = "diagnostics")]
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_OK, SW_SHOWNORMAL,
};

fn main() {
    if let Err(err) = run() {
        show_error(&err.to_string());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_default();
    if command.is_empty() {
        match invoked_alias_action() {
            Some(AliasAction::Register) => {
                register_package_identity()?;
                return Ok(());
            }
            Some(AliasAction::Unregister) => {
                unregister_package_identity()?;
                return Ok(());
            }
            None => {}
        }
    }
    match command.as_str() {
        "pick-source" => {
            let paths = args.map(PathBuf::from).collect::<Vec<_>>();
            if paths.is_empty() {
                bail!("at least one source path is required");
            }
            save_sources(&paths)?;
        }
        "drop-symlink" => drop_links(LinkKind::Symbolic, args.next())?,
        "drop-junction" => drop_links(LinkKind::Junction, args.next())?,
        "drop-hardlink" => drop_links(LinkKind::HardLink, args.next())?,
        "clear" => {
            clear_state()?;
        }
        "about" => {
            show_about();
        }
        #[cfg(feature = "diagnostics")]
        "diagnostics" => {
            show_diagnostics()?;
        }
        #[cfg(not(feature = "diagnostics"))]
        "diagnostics" => bail!("LSENext diagnostics are only available in debug builds"),
        #[cfg(feature = "diagnostics")]
        "repair-native-menu" => {
            launch_repair_native_menu()?;
        }
        #[cfg(not(feature = "diagnostics"))]
        "repair-native-menu" => bail!("LSENext repair is only available in debug builds"),
        #[cfg(feature = "diagnostics")]
        "repair-native-menu-run" => repair_native_menu()?,
        #[cfg(not(feature = "diagnostics"))]
        "repair-native-menu-run" => bail!("LSENext repair is only available in debug builds"),
        "register-package" => {
            register_package_identity()?;
        }
        "prepare-machine-registration" => {
            prepare_machine_registration()?;
        }
        "trust-package-certificate-machine" => {
            trust_package_certificate_machine()?;
        }
        "unregister-package" => {
            unregister_package_identity()?;
        }
        _ => {
            bail!("usage: lsenext-helper <pick-source|drop-symlink|drop-junction|drop-hardlink|clear|about|register-package|prepare-machine-registration|trust-package-certificate-machine|unregister-package> [paths]");
        }
    }
    Ok(())
}

fn drop_links(kind: LinkKind, target: Option<String>) -> Result<()> {
    let target = target
        .map(PathBuf::from)
        .context("target directory argument is required")?;
    let state = load_state()?.context("no picked LSENext source is stored")?;
    if kind == LinkKind::Junction && state.sources.iter().any(|source| !source.is_dir) {
        bail!("Directory junctions can only be created from picked directory sources.");
    }
    if kind == LinkKind::HardLink && state.sources.iter().any(|source| source.is_dir) {
        bail!("Hard links can only be created from picked file sources.");
    }
    for source in &state.sources {
        if let Err(err) = create_link(kind, source, &target) {
            if should_retry_elevated(&err) {
                run_elevated(kind, &target)?;
                return Ok(());
            }
            return Err(err.into());
        }
    }
    Ok(())
}

fn should_retry_elevated(error: &lsenext_core::links::LinkError) -> bool {
    match error {
        lsenext_core::links::LinkError::CreateFailed { error, .. } => {
            matches!(error.raw_os_error(), Some(5) | Some(1314))
        }
        _ => false,
    }
}

fn run_elevated(kind: LinkKind, target: &Path) -> Result<()> {
    let exe = env::current_exe().context("failed to locate LSENext helper")?;
    let command = match kind {
        LinkKind::Symbolic => "drop-symlink",
        LinkKind::Junction => "drop-junction",
        LinkKind::HardLink => "drop-hardlink",
    };
    let params = format!("{} \"{}\"", command, target.display());
    let verb = wide_null("runas");
    let file = wide_null(&exe.to_string_lossy());
    let args = wide_null(&params);

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            args.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL as i32,
        )
    };
    if result as isize <= 32 {
        bail!(
            "failed to start elevated helper, ShellExecuteW returned {}",
            result as isize
        );
    }
    Ok(())
}

fn show_about() {
    show_info("LSENext 0.1.0\nQuick symbolic, hard link, and directory junction creation.");
}

#[cfg(feature = "diagnostics")]
fn show_diagnostics() -> Result<()> {
    write_and_open_diagnostics(None)
}

#[cfg(feature = "diagnostics")]
fn launch_repair_native_menu() -> Result<()> {
    let install_root = current_install_root()?;
    let script_path = diagnostics_path()?
        .parent()
        .context("failed to locate diagnostics directory")?
        .join("repair-native-menu.ps1");
    if let Some(parent) = script_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let script = r#"
$Host.UI.RawUI.WindowTitle = "LSENext Repair Native Menu"
"LSENext Repair Native Menu"
"This window shows repair progress and writes diagnostics."
""

$installRoot = __INSTALL_ROOT__
$diag = Join-Path $env:LOCALAPPDATA "LSENext\diagnostics.txt"
$package = Join-Path $installRoot "LSENext.identity.msix"
$certificate = Join-Path $installRoot "LSENext.cer"
New-Item -ItemType Directory -Force -Path (Split-Path $diag) | Out-Null
"LSENext repair log" | Set-Content -Path $diag
"==================" | Add-Content -Path $diag
"script: $PSCommandPath" | Add-Content -Path $diag
"installRoot: $installRoot" | Add-Content -Path $diag

function Step($Percent, $Name, [scriptblock]$Action) {
  Write-Progress -Activity "LSENext Repair Native Menu" -Status $Name -PercentComplete $Percent
  Write-Host ("[{0,3}%] {1}" -f $Percent, $Name)
  "" | Add-Content -Path $diag
  "STEP ${Name}: started" | Add-Content -Path $diag
  try {
    $output = & $Action 2>&1 | Out-String
    if ($output.Trim().Length -gt 0) { $output.Trim() | Add-Content -Path $diag }
    "STEP ${Name}: ok" | Add-Content -Path $diag
    Write-Host ("[{0,3}%] {1}: ok" -f $Percent, $Name)
  } catch {
    "STEP ${Name}: ERROR: $($_.Exception.Message)" | Add-Content -Path $diag
    Write-Host ("[{0,3}%] {1}: ERROR" -f $Percent, $Name)
    Write-Host $_.Exception.Message
  }
}

Step 5 "check installed files" {
  foreach ($path in @($package, $certificate, (Join-Path $installRoot "lsenext-shell.dll"), (Join-Path $installRoot "AppxManifest.xml"), (Join-Path $installRoot "Assets\LSENext.ico"))) {
    if (Test-Path $path) {
      $item = Get-Item -LiteralPath $path
      "$path exists size=$($item.Length)"
    } else {
      "$path missing"
    }
  }
}

Step 10 "cleanup classic context menu" {
  $paths = @(
    "HKCU:\Software\Classes\*\shell\LSENext",
    "HKCU:\Software\Classes\Directory\shell\LSENext",
    "HKCU:\Software\Classes\Directory\Background\shell\LSENext",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.PickSource",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropSymbolic",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropJunction",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropHardLink",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropSymbolic",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropJunction",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropHardLink",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.ClearSource",
    "HKLM:\Software\Classes\*\shell\LSENext",
    "HKLM:\Software\Classes\Directory\shell\LSENext",
    "HKLM:\Software\Classes\Directory\Background\shell\LSENext",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.PickSource",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropSymbolic",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropJunction",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropHardLink",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropSymbolic",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropJunction",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropHardLink",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.ClearSource"
  )
  foreach ($path in $paths) {
    "cleanup $path"
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue }
  }
}

Step 35 "unregister existing package identity" {
  $pkg = Get-AppxPackage -Name Sunnylin.LSENext -ErrorAction SilentlyContinue
  if ($pkg) {
    Remove-AppxPackage -Package $pkg.PackageFullName -ErrorAction Stop
  } else {
    "Sunnylin.LSENext was not registered"
  }
}

Step 60 "trust package certificate" {
  if (Test-Path $certificate) {
    $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($certificate)
    "certificate subject=$($cert.Subject) thumbprint=$($cert.Thumbprint)"
    Import-Certificate -FilePath $certificate -CertStoreLocation Cert:\CurrentUser\Root -ErrorAction Stop | Out-Null
    Import-Certificate -FilePath $certificate -CertStoreLocation Cert:\CurrentUser\TrustedPeople -ErrorAction Stop | Out-Null
    Import-Certificate -FilePath $certificate -CertStoreLocation Cert:\LocalMachine\Root -ErrorAction Stop | Out-Null
    Import-Certificate -FilePath $certificate -CertStoreLocation Cert:\LocalMachine\TrustedPeople -ErrorAction Stop | Out-Null
    foreach ($store in @("Cert:\CurrentUser\Root", "Cert:\CurrentUser\TrustedPeople", "Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPeople")) {
      "$store\$($cert.Thumbprint) exists=$(Test-Path -LiteralPath (Join-Path $store $cert.Thumbprint))"
    }
  } else {
    "certificate missing: $certificate"
  }
}

Step 85 "register package identity" {
  if (-not (Test-Path $package)) { throw "package missing: $package" }
  Add-AppxPackage -Path $package -ExternalLocation $installRoot -ForceApplicationShutdown -ForceUpdateFromAnyVersion -ErrorAction Stop
}

Step 95 "collect registration diagnostics" {
  "[appx-package]"
  $pkg = Get-AppxPackage -Name Sunnylin.LSENext -ErrorAction SilentlyContinue
  if ($pkg) {
    $pkg | Format-List Name, PackageFullName, InstallLocation, SignatureKind, Status | Out-String
  } else {
    "Sunnylin.LSENext package not found"
  }
  ""
  "[classic-context-menu-registry]"
  foreach ($path in @(
    "HKCU:\Software\Classes\*\shell\LSENext",
    "HKCU:\Software\Classes\Directory\shell\LSENext",
    "HKCU:\Software\Classes\Directory\Background\shell\LSENext",
    "HKLM:\Software\Classes\*\shell\LSENext",
    "HKLM:\Software\Classes\Directory\shell\LSENext",
    "HKLM:\Software\Classes\Directory\Background\shell\LSENext"
  )) {
    "KEY $path exists=$(Test-Path -LiteralPath $path)"
  }
}

Write-Progress -Activity "LSENext Repair Native Menu" -Status "Opening diagnostics" -PercentComplete 100
Write-Host "[100%] opening diagnostics in Notepad"
"" | Add-Content -Path $diag
Start-Process notepad.exe -ArgumentList $diag
Write-Progress -Activity "LSENext Repair Native Menu" -Completed
Write-Host ""
Write-Host "Done. This window will stay open. If Notepad did not open, copy the lines above and the file path below:"
Write-Host $diag
"#
    .replace(
        "__INSTALL_ROOT__",
        &ps_quote(&install_root.to_string_lossy()),
    );
    fs::write(&script_path, script)?;

    Command::new("powershell.exe")
        .args([
            "-NoExit",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .spawn()
        .context("failed to start visible repair window")?;
    Ok(())
}

#[cfg(feature = "diagnostics")]
fn repair_native_menu() -> Result<()> {
    let mut repair = String::new();
    push_line(&mut repair, "LSENext repair log");
    push_line(&mut repair, "==================");

    write_repair_progress(&repair)?;
    run_repair_step(
        &mut repair,
        10,
        "cleanup classic context menu",
        cleanup_classic_context_menu_script(),
    );
    write_repair_progress(&repair)?;
    run_repair_step(
        &mut repair,
        35,
        "unregister existing package identity",
        unregister_package_identity_script(),
    );
    write_repair_progress(&repair)?;
    if let Ok(install_root) = current_install_root() {
        let certificate = install_root.join("LSENext.cer");
        if certificate.is_file() {
            run_repair_step(
                &mut repair,
                60,
                "trust package certificate",
                &format!(
                    "Import-Certificate -FilePath {} -CertStoreLocation Cert:\\CurrentUser\\Root | Out-Null; Import-Certificate -FilePath {} -CertStoreLocation Cert:\\CurrentUser\\TrustedPeople | Out-Null; Import-Certificate -FilePath {} -CertStoreLocation Cert:\\LocalMachine\\Root | Out-Null; Import-Certificate -FilePath {} -CertStoreLocation Cert:\\LocalMachine\\TrustedPeople | Out-Null",
                    ps_quote(&certificate.to_string_lossy()),
                    ps_quote(&certificate.to_string_lossy()),
                    ps_quote(&certificate.to_string_lossy()),
                    ps_quote(&certificate.to_string_lossy())
                ),
            );
        } else {
            push_line(
                &mut repair,
                "STEP trust package certificate: skipped, certificate missing",
            );
        }
        write_repair_progress(&repair)?;

        let package = install_root.join("LSENext.identity.msix");
        run_repair_step(
            &mut repair,
            85,
            "register package identity",
            &format!(
                "Add-AppxPackage -Path {} -ExternalLocation {} -ForceApplicationShutdown -ForceUpdateFromAnyVersion",
                ps_quote(&package.to_string_lossy()),
                ps_quote(&install_root.to_string_lossy())
            ),
        );
    } else {
        push_line(&mut repair, "STEP locate install root: failed");
    }
    write_repair_progress(&repair)?;

    write_and_open_diagnostics(Some(&repair))
}

#[cfg(feature = "diagnostics")]
fn diagnostics_path() -> Result<PathBuf> {
    let local_app_data = env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local_app_data)
        .join("LSENext")
        .join("diagnostics.txt"))
}

#[cfg(feature = "diagnostics")]
fn build_diagnostics() -> String {
    let install_root = current_install_root().ok();
    let state_path = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("LSENext").join("state.json"));
    let mut output = String::new();

    push_line(&mut output, "LSENext diagnostics");
    push_line(&mut output, "===================");
    push_line(
        &mut output,
        &format!("version: {}", env!("CARGO_PKG_VERSION")),
    );
    push_line(&mut output, &format!("process_arch: {}", env::consts::ARCH));
    push_line(
        &mut output,
        &format!("current_exe: {:?}", env::current_exe()),
    );
    push_line(&mut output, &format!("install_root: {:?}", install_root));
    push_line(&mut output, &format!("state_path: {:?}", state_path));
    if let Some(path) = &state_path {
        push_line(&mut output, &format!("state_exists: {}", path.exists()));
    }

    if let Some(root) = &install_root {
        push_line(&mut output, "");
        push_line(&mut output, "[files]");
        for name in [
            "lsenext-helper.exe",
            "lsenext-shell.dll",
            "AppxManifest.xml",
            "LSENext.identity.msix",
            "LSENext.cer",
            "Assets\\StoreLogo.png",
            "Assets\\Square150x150Logo.png",
            "Assets\\Square44x44Logo.png",
            "Assets\\LSENext.ico",
        ] {
            let path = root.join(name);
            let size = fs::metadata(&path).map(|metadata| metadata.len()).ok();
            push_line(
                &mut output,
                &format!("{} exists={} size={:?}", name, path.exists(), size),
            );
        }
        if let Ok(manifest) = fs::read_to_string(root.join("AppxManifest.xml")) {
            push_line(&mut output, "");
            push_line(&mut output, "[installed AppxManifest.xml]");
            push_line(&mut output, &manifest);
        }
    }

    push_ps(
        &mut output,
        "appx-package",
        r#"
$pkg = Get-AppxPackage -Name Sunnylin.LSENext
if ($pkg) {
  $pkg | Format-List Name, PackageFullName, InstallLocation, SignatureKind, Status, IsFramework, PackageUserInformation
  ""
  "Manifest extension XML:"
  ([xml](Get-AppxPackageManifest -Package $pkg.PackageFullName)).Package.Applications.Application.Extensions.InnerXml
} else {
  "Sunnylin.LSENext package not found"
}
"#,
    );
    push_ps(
        &mut output,
        "classic-context-menu-registry",
        r#"
$keys = @(
  "HKLM:\Software\Classes\*\shell\LSENext",
  "HKLM:\Software\Classes\Directory\shell\LSENext",
  "HKLM:\Software\Classes\Directory\Background\shell\LSENext",
  "HKCU:\Software\Classes\*\shell\LSENext",
  "HKCU:\Software\Classes\Directory\shell\LSENext",
  "HKCU:\Software\Classes\Directory\Background\shell\LSENext",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.PickSource",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.PickSource"
)
foreach ($key in $keys) {
  "KEY $key exists=$(Test-Path -LiteralPath $key)"
  if (Test-Path -LiteralPath $key) { Get-ItemProperty -LiteralPath $key | Format-List * }
}
"#,
    );
    push_ps(
        &mut output,
        "packaged-com-registry",
        r#"
Get-ChildItem "HKCU:\Software\Classes\ActivatableClasses\Package" -ErrorAction SilentlyContinue |
  Where-Object { $_.PSChildName -like "Sunnylin.LSENext*" } |
  ForEach-Object {
    "PACKAGE KEY $($_.Name)"
    Get-ChildItem $_.PSPath -Recurse -ErrorAction SilentlyContinue |
      Where-Object { $_.PSChildName -match "32ad61d5|LSENext|Explorer|Context" } |
      Select-Object -First 80 Name
  }
"#,
    );

    output
}

#[cfg(feature = "diagnostics")]
fn write_and_open_diagnostics(prefix: Option<&str>) -> Result<()> {
    let mut text = String::new();
    if let Some(prefix) = prefix {
        text.push_str(prefix);
        text.push_str("\r\n\r\n");
    }
    text.push_str(&build_diagnostics());

    let path = diagnostics_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(&path)?;
    file.write_all(text.as_bytes())?;

    Command::new("notepad.exe")
        .arg(&path)
        .spawn()
        .context("failed to open diagnostics in Notepad")?;
    Ok(())
}

#[cfg(feature = "diagnostics")]
fn write_repair_progress(repair: &str) -> Result<()> {
    let path = diagnostics_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, repair)?;
    Ok(())
}

#[cfg(feature = "diagnostics")]
fn push_line(output: &mut String, value: &str) {
    output.push_str(value);
    output.push_str("\r\n");
}

#[cfg(feature = "diagnostics")]
fn push_ps(output: &mut String, title: &str, script: &str) {
    push_line(output, "");
    push_line(output, &format!("[powershell:{}]", title));
    match powershell_output(script) {
        Ok(text) => push_line(output, &text),
        Err(err) => push_line(output, &format!("ERROR: {err:#}")),
    }
}

#[cfg(feature = "diagnostics")]
fn powershell_output(script: &str) -> Result<String> {
    let output = powershell_command(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to start PowerShell")?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        bail!("PowerShell exited with {}\n{}", output.status, text);
    }
    Ok(text)
}

fn register_package_identity() -> Result<()> {
    cleanup_classic_context_menu().context("failed to clean classic Explorer menu registration")?;
    unregister_package_identity()
        .context("failed to unregister existing LSENext package identity")?;

    let install_root = current_install_root()?;
    let package = install_root.join("LSENext.identity.msix");
    if !package.is_file() {
        bail!("missing package identity file: {}", package.display());
    }
    let certificate = install_root.join("LSENext.cer");
    if certificate.is_file() {
        run_powershell_script(&format!(
            "Import-Certificate -FilePath {} -CertStoreLocation Cert:\\CurrentUser\\Root | Out-Null; Import-Certificate -FilePath {} -CertStoreLocation Cert:\\CurrentUser\\TrustedPeople | Out-Null",
            ps_quote(&certificate.to_string_lossy()),
            ps_quote(&certificate.to_string_lossy()),
        ))
        .context("failed to trust LSENext package certificate for the current user")?;
    }

    run_powershell_script(&format!(
        "Add-AppxPackage -Path {} -ExternalLocation {} -ForceApplicationShutdown -ForceUpdateFromAnyVersion",
        ps_quote(&package.to_string_lossy()),
        ps_quote(&install_root.to_string_lossy())
    ))
    .context("failed to register LSENext package identity")
}

fn prepare_machine_registration() -> Result<()> {
    cleanup_classic_context_menu().context("failed to clean classic Explorer menu registration")?;
    trust_package_certificate_machine()
}

fn trust_package_certificate_machine() -> Result<()> {
    let install_root = current_install_root()?;
    let certificate = install_root.join("LSENext.cer");
    if !certificate.is_file() {
        bail!(
            "missing package certificate file: {}",
            certificate.display()
        );
    }
    run_powershell_script(&format!(
        "Import-Certificate -FilePath {} -CertStoreLocation Cert:\\LocalMachine\\Root | Out-Null; Import-Certificate -FilePath {} -CertStoreLocation Cert:\\LocalMachine\\TrustedPeople | Out-Null",
        ps_quote(&certificate.to_string_lossy()),
        ps_quote(&certificate.to_string_lossy())
    ))
    .context("failed to trust LSENext package certificate machine-wide")
}

fn unregister_package_identity() -> Result<()> {
    run_powershell_script(unregister_package_identity_script())
        .context("failed to unregister LSENext package identity")
}

fn cleanup_classic_context_menu() -> Result<()> {
    run_powershell_script(cleanup_classic_context_menu_script())
}

fn unregister_package_identity_script() -> &'static str {
    "$package = Get-AppxPackage -Name Sunnylin.LSENext; if ($package) { Remove-AppxPackage -Package $package.PackageFullName }"
}

fn cleanup_classic_context_menu_script() -> &'static str {
    r#"
$paths = @(
  "HKCU:\Software\Classes\*\shell\LSENext",
  "HKCU:\Software\Classes\Directory\shell\LSENext",
  "HKCU:\Software\Classes\Directory\Background\shell\LSENext",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.PickSource",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropSymbolic",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropJunction",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropHardLink",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropSymbolic",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropJunction",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropHardLink",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.ClearSource",
  "HKLM:\Software\Classes\*\shell\LSENext",
  "HKLM:\Software\Classes\Directory\shell\LSENext",
  "HKLM:\Software\Classes\Directory\Background\shell\LSENext",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.PickSource",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropSymbolic",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropJunction",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropHardLink",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropSymbolic",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropJunction",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropHardLink",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.ClearSource"
)
foreach ($path in $paths) {
  if (Test-Path -LiteralPath $path) {
    Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue
  }
}
"#
}

fn current_install_root() -> Result<PathBuf> {
    let exe = env::current_exe().context("failed to locate LSENext helper")?;
    exe.parent()
        .map(Path::to_path_buf)
        .context("failed to locate LSENext install directory")
}

enum AliasAction {
    Register,
    Unregister,
}

fn invoked_alias_action() -> Option<AliasAction> {
    env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|name| name.to_string_lossy().to_ascii_lowercase()))
        .and_then(|name| match name.as_str() {
            "lsenext-register" => Some(AliasAction::Register),
            "lsenext-unregister" => Some(AliasAction::Unregister),
            _ => None,
        })
}

fn run_powershell_script(script: &str) -> Result<()> {
    let status = powershell_command(script)
        .status()
        .context("failed to start PowerShell")?;
    if !status.success() {
        bail!("PowerShell exited with {}", status);
    }
    Ok(())
}

#[cfg(feature = "diagnostics")]
fn run_repair_step(repair: &mut String, percent: u32, name: &str, script: &str) {
    push_line(repair, "");
    println!("[{percent:>3}%] {name}");
    push_line(repair, &format!("STEP {name}: started"));
    match powershell_output_with_timeout(script, Duration::from_secs(15)) {
        Ok(text) => {
            println!("[{percent:>3}%] {name}: ok");
            push_line(repair, &format!("STEP {name}: ok"));
            if !text.trim().is_empty() {
                push_line(repair, text.trim());
            }
        }
        Err(err) => {
            println!("[{percent:>3}%] {name}: ERROR");
            push_line(repair, &format!("STEP {name}: ERROR: {err:#}"));
        }
    }
}

#[cfg(feature = "diagnostics")]
fn powershell_output_with_timeout(script: &str, timeout: Duration) -> Result<String> {
    let mut child = powershell_command(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start PowerShell")?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            return powershell_text_result(output);
        }
        if started.elapsed() >= timeout {
            child.kill().ok();
            let output = child.wait_with_output()?;
            let text = output_to_text(&output);
            bail!(
                "PowerShell timed out after {} seconds\n{}",
                timeout.as_secs(),
                text
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(feature = "diagnostics")]
fn powershell_text_result(output: std::process::Output) -> Result<String> {
    let text = output_to_text(&output);
    if !output.status.success() {
        bail!("PowerShell exited with {}\n{}", output.status, text);
    }
    Ok(text)
}

#[cfg(feature = "diagnostics")]
fn output_to_text(output: &std::process::Output) -> String {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

fn powershell_command(script: &str) -> Command {
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script);
    hide_console_window(&mut command);
    command
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn show_info(message: &str) {
    show_message(message, MB_OK);
}

fn show_error(message: &str) {
    show_message(message, MB_OK | MB_ICONERROR);
}

fn show_message(message: &str, style: u32) {
    let title = wide_null("LSENext");
    let message = wide_null(message);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            style,
        );
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
