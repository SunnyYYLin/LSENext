#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PAYLOAD_MAGIC: &[u8; 16] = b"LSENEXTPAYLOAD1!";
const FOOTER_LEN: usize = 24;

fn main() {
    if let Err(err) = run() {
        let _ = write_bootstrapper_diagnostics(&err.to_string());
        show_error(&err.to_string());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let exe = env::current_exe().context("failed to locate LSENext setup executable")?;
    let architecture = parse_architecture_from_filename(&exe)?;
    let install_root = default_install_root(&architecture)?;
    let extracted = extract_payload(&exe)?;

    if install_root.exists() {
        fs::remove_dir_all(&install_root).with_context(|| {
            format!(
                "failed to remove previous LSENext install at {}",
                install_root.display()
            )
        })?;
    }
    fs::create_dir_all(&install_root).with_context(|| {
        format!(
            "failed to create LSENext install directory {}",
            install_root.display()
        )
    })?;
    copy_tree(&extracted, &install_root)?;

    let helper = install_root.join("lsenext-helper.exe");
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

    let _ = fs::remove_dir_all(&extracted);
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

fn default_install_root(architecture: &str) -> Result<PathBuf> {
    let local_app_data = env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local_app_data)
        .join("LSENext")
        .join(architecture))
}

fn extract_payload(setup_exe: &Path) -> Result<PathBuf> {
    let mut file = fs::File::open(setup_exe)
        .with_context(|| format!("failed to open {}", setup_exe.display()))?;
    let metadata = file.metadata()?;
    if metadata.len() < FOOTER_LEN as u64 {
        bail!("setup executable is missing its payload footer");
    }

    file.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut footer = [0u8; FOOTER_LEN];
    file.read_exact(&mut footer)?;
    if &footer[..16] != PAYLOAD_MAGIC {
        bail!("setup executable payload marker is missing");
    }
    let payload_len = u64::from_le_bytes(
        footer[16..24]
            .try_into()
            .context("invalid payload footer length")?,
    );
    if payload_len == 0 || payload_len > metadata.len() - FOOTER_LEN as u64 {
        bail!("setup executable payload length is invalid");
    }

    let temp_root = env::temp_dir().join(format!("LSENextSetup-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root).ok();
    }
    fs::create_dir_all(&temp_root)?;

    let payload_zip = temp_root.join("payload.zip");
    file.seek(SeekFrom::End(-((FOOTER_LEN as u64 + payload_len) as i64)))?;
    let mut payload = vec![0u8; payload_len as usize];
    file.read_exact(&mut payload)?;
    fs::write(&payload_zip, payload)?;

    let expanded = temp_root.join("expanded");
    fs::create_dir_all(&expanded)?;
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(format!(
            "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
            ps_quote(&payload_zip.to_string_lossy()),
            ps_quote(&expanded.to_string_lossy())
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to extract embedded payload archive")?;
    if !status.success() {
        bail!("embedded payload extraction exited with {}", status);
    }
    Ok(expanded)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to enumerate {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
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

fn write_bootstrapper_diagnostics(message: &str) -> Result<()> {
    let local_app_data = env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    let path = PathBuf::from(local_app_data)
        .join("LSENext")
        .join("diagnostics.txt");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = String::new();
    text.push_str("LSENext bootstrapper diagnostics\r\n");
    text.push_str("================================\r\n");
    text.push_str(&format!("version: {}\r\n", env!("CARGO_PKG_VERSION")));
    text.push_str(&format!("process_arch: {}\r\n", env::consts::ARCH));
    text.push_str(&format!("current_exe: {:?}\r\n", env::current_exe()));
    text.push_str("\r\n[last_error]\r\n");
    text.push_str(message);
    text.push_str("\r\n");
    fs::write(path, text)?;
    Ok(())
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
