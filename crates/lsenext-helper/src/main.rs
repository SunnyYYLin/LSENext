#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{bail, Context, Result};
use lsenext_core::{create_link, load_state, save_sources, LinkKind};
use std::env;
use std::path::{Path, PathBuf};

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
        "drop-symlink" => drop_links(LinkKind::Symbolic, "drop-symlink", args.next())?,
        "drop-junction" => drop_links(LinkKind::Junction, "drop-junction", args.next())?,
        "clear" => {
            lsenext_core::clear_state()?;
        }
        "about" => {
            show_about();
        }
        _ => {
            bail!(
                "usage: lsenext-helper <pick-source|drop-symlink|drop-junction|clear|about> [paths]"
            );
        }
    }
    Ok(())
}

fn drop_links(kind: LinkKind, command: &str, target: Option<String>) -> Result<()> {
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
                run_elevated(command, &target)?;
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

fn run_elevated(command: &str, target: &Path) -> Result<()> {
    let exe = env::current_exe().context("failed to locate LSENext helper")?;
    let params = format!("{} \"{}\"", command, target.display());
    let verb = wide_null("runas");
    let file = wide_null(&exe.to_string_lossy());
    let args = wide_null(&params);

    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            args.as_ptr(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
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
    show_info("LSENext 0.0.1\nQuick symbolic link and directory junction creation.");
}

fn show_info(message: &str) {
    show_message(message, windows_sys::Win32::UI::WindowsAndMessaging::MB_OK);
}

fn show_error(message: &str) {
    show_message(
        message,
        windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
            | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
    );
}

fn show_message(message: &str, style: u32) {
    let title = wide_null("LSENext");
    let message = wide_null(message);
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
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
