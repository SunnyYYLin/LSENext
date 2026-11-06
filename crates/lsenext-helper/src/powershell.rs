use anyhow::{bail, Context, Result};
use std::process::Command;
#[cfg(feature = "diagnostics")]
use std::process::Stdio;

pub fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn powershell_command(script: &str) -> Command {
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script);
    hide_console_window(&mut command);
    command
}

#[cfg(windows)]
pub fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console_window(_command: &mut Command) {}

pub fn run_powershell_script(script: &str) -> Result<()> {
    #[cfg(feature = "diagnostics")]
    {
        let output = powershell_command(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("failed to start PowerShell")?;
        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            let details = text.trim();
            if details.is_empty() {
                bail!("PowerShell exited with {}", output.status);
            }
            bail!("PowerShell exited with {}\n{}", output.status, details);
        }
        Ok(())
    }
    #[cfg(not(feature = "diagnostics"))]
    {
        let status = powershell_command(script)
            .status()
            .context("failed to start PowerShell")?;
        if !status.success() {
            bail!("PowerShell exited with {}", status);
        }
        Ok(())
    }
}
