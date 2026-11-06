use crate::powershell::{ps_quote, run_powershell_script};
use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_FLUSH, SHCNF_IDLIST};

pub fn current_install_root() -> Result<PathBuf> {
    let exe = env::current_exe().context("failed to locate LSENext helper")?;
    exe.parent()
        .map(Path::to_path_buf)
        .context("failed to locate LSENext install directory")
}

pub fn register_package_identity() -> Result<()> {
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
    .context("failed to register LSENext package identity")?;

    notify_explorer_registration_changed();
    lsenext_core::prompt_developer_mode_if_needed(false);
    Ok(())
}

pub fn prepare_machine_registration() -> Result<()> {
    cleanup_classic_context_menu().context("failed to clean classic Explorer menu registration")?;
    trust_package_certificate_machine()
}

pub fn trust_package_certificate_machine() -> Result<()> {
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

pub fn unregister_package_identity() -> Result<()> {
    run_powershell_script(unregister_package_identity_script())
        .context("failed to unregister LSENext package identity")
}

pub fn cleanup_classic_context_menu() -> Result<()> {
    run_powershell_script(cleanup_classic_context_menu_script())
}

pub fn notify_explorer_registration_changed() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST | SHCNF_FLUSH,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

pub fn unregister_package_identity_script() -> &'static str {
    "$package = Get-AppxPackage -Name Sunnylin.LSENext; if ($package) { Remove-AppxPackage -Package $package.PackageFullName }"
}

pub fn cleanup_classic_context_menu_script() -> &'static str {
    r#"
$subkeys = @(
  "Software\Classes\*\shell\LSENext",
  "Software\Classes\Directory\shell\LSENext",
  "Software\Classes\Directory\Background\shell\LSENext",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.PickSource",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropSymbolic",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropJunction",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.DropHardLink",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropSymbolic",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropJunction",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.BackgroundDropHardLink",
  "Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell\LSENext.ClearSource"
)
foreach ($hive in @("HKCU:\", "HKLM:\")) {
  foreach ($sub in $subkeys) {
    $path = Join-Path $hive $sub
    if (Test-Path -LiteralPath $path) {
      Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
}
"#
}
