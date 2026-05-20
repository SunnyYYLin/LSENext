#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{bail, Context, Result};
use lsenext_core::{clear_state, create_link, load_state, save_sources, LinkKind};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
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
        "clear" => {
            clear_state()?;
        }
        "about" => {
            show_about();
        }
        "register-package" => {
            register_package_identity()?;
        }
        "unregister-package" => {
            unregister_package_identity()?;
        }
        _ => {
            bail!("usage: lsenext-helper <pick-source|drop-symlink|drop-junction|clear|about|register-package|unregister-package> [paths]");
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
    show_info("LSENext 0.0.2\nQuick symbolic link and directory junction creation.");
}

fn register_package_identity() -> Result<()> {
    let install_root = current_install_root()?;
    let manifest = install_root.join("AppxManifest.xml");
    if !manifest.is_file() {
        bail!("missing package manifest: {}", manifest.display());
    }

    run_powershell_script(&format!(
        "Add-AppxPackage -Register -Path {} -ExternalLocation {} -ForceApplicationShutdown -ForceUpdateFromAnyVersion",
        ps_quote(&manifest.to_string_lossy()),
        ps_quote(&install_root.to_string_lossy())
    ))
    .context("failed to register LSENext package identity")
}

fn unregister_package_identity() -> Result<()> {
    run_powershell_script(
        "$package = Get-AppxPackage -Name Sunnylin.LSENext; if ($package) { Remove-AppxPackage -Package $package.PackageFullName }",
    )
    .context("failed to unregister LSENext package identity")
}

fn current_install_root() -> Result<PathBuf> {
    let exe = env::current_exe().context("failed to locate LSENext helper")?;
    exe.parent()
        .map(Path::to_path_buf)
        .context("failed to locate LSENext install directory")
}

fn run_powershell_script(script: &str) -> Result<()> {
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script)
        .status()
        .context("failed to start PowerShell")?;
    if !status.success() {
        bail!("PowerShell exited with {}", status);
    }
    Ok(())
}

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
