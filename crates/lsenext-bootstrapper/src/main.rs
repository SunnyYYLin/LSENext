#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    if let Err(err) = run() {
        show_error(&err.to_string());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let exe = env::current_exe().context("failed to locate LSENext setup executable")?;
    let setup_dir = exe
        .parent()
        .map(Path::to_path_buf)
        .context("failed to locate LSENext setup directory")?;

    let architecture = parse_architecture_from_filename(&exe)?;
    let msi = setup_dir.join(format!("LSENext-{architecture}.msi"));
    if !msi.is_file() {
        bail!("missing packaged MSI next to setup executable: {}", msi.display());
    }

    let install_root = default_install_root(&architecture);
    let status = Command::new("msiexec.exe")
        .arg("/i")
        .arg(&msi)
        .arg(format!("INSTALLFOLDER={install_root}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to start Windows Installer")?;
    if !status.success() {
        bail!("Windows Installer exited with {}", status);
    }

    let helper = PathBuf::from(&install_root).join("lsenext-helper.exe");
    if !helper.is_file() {
        bail!(
            "installation finished but helper was not found at {}",
            helper.display()
        );
    }

    let status = Command::new(&helper)
        .arg("register-package")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to start {}", helper.display()))?;
    if !status.success() {
        bail!("LSENext native menu registration exited with {}", status);
    }

    Ok(())
}

fn parse_architecture_from_filename(path: &Path) -> Result<&'static str> {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .context("failed to read setup executable file name")?;
    if name.contains("arm64") {
        Ok("arm64")
    } else if name.contains("x64") {
        Ok("x64")
    } else {
        bail!("cannot infer target architecture from setup executable name: {name}");
    }
}

fn default_install_root(architecture: &str) -> String {
    let program_files = env::var("ProgramFiles").unwrap_or_else(|_| String::from(r"C:\Program Files"));
    format!(r"{program_files}\LSENext\{architecture}")
}

fn show_error(message: &str) {
    let script = format!(
        r#"[System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; [System.Windows.Forms.MessageBox]::Show({message}, 'LSENext Setup', 'OK', 'Error') | Out-Null"#
    );
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script.replace("{message}", &ps_quote(message)))
        .status();
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
