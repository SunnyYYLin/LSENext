#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{bail, Context, Result};
use lsenext_core::{create_link, load_state, save_sources, LinkKind};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

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
            let state = save_sources(&paths)?;
            update_user_menu(state.sources.iter().all(|source| source.is_dir))?;
        }
        "drop-symlink" => drop_links(LinkKind::Symbolic, "drop-symlink", args.next())?,
        "drop-junction" => drop_links(LinkKind::Junction, "drop-junction", args.next())?,
        "clear" => {
            lsenext_core::clear_state()?;
            update_user_menu(false)?;
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

fn update_user_menu(include_junction: bool) -> Result<()> {
    let helper = env::current_exe().context("failed to locate LSENext helper")?;
    let helper = helper.to_string_lossy();
    let root = r"HKCU\Software\Classes";

    write_pick_menu(root, &helper)?;
    write_drop_menu(
        root,
        r"Directory\shell\LSENext",
        "%1",
        &helper,
        include_junction,
    )?;
    write_drop_menu(
        root,
        r"Directory\Background\shell\LSENext",
        "%V",
        &helper,
        include_junction,
    )?;
    Ok(())
}

fn write_pick_menu(root: &str, helper: &str) -> Result<()> {
    let menu = format!(r"{root}\*\shell\LSENext");
    reg_add_value(&menu, "MUIVerb", "LSENext")?;
    reg_add_value(&menu, "Icon", "shell32.dll,-16769")?;
    reg_add_value(&menu, "SubCommands", "")?;
    let pick = format!(r"{menu}\shell\PickSource");
    reg_add_value(&pick, "MUIVerb", "Pick Link Source")?;
    reg_add_default(
        &format!(r"{pick}\command"),
        &format!(r#""{helper}" pick-source "%1""#),
    )
}

fn write_drop_menu(
    root: &str,
    menu_path: &str,
    target_token: &str,
    helper: &str,
    include_junction: bool,
) -> Result<()> {
    let menu = format!(r"{root}\{menu_path}");
    reg_add_value(&menu, "MUIVerb", "LSENext")?;
    reg_add_value(&menu, "Icon", "shell32.dll,-16769")?;
    reg_add_value(&menu, "SubCommands", "")?;

    let shell = format!(r"{menu}\shell");
    let clear = format!(r"{shell}\ClearSource");
    reg_add_value(&clear, "MUIVerb", "Clear Link Source")?;
    reg_add_default(
        &format!(r"{clear}\command"),
        &format!(r#""{helper}" clear"#),
    )?;

    if include_junction {
        let junction = format!(r"{shell}\DropJunction");
        let _ = reg_delete_tree(&junction);
        reg_add_value(&junction, "MUIVerb", "Drop Directory Junction")?;
        reg_add_default(
            &format!(r"{junction}\command"),
            &format!(r#""{helper}" drop-junction "{target_token}""#),
        )?;
    } else {
        let junction = format!(r"{shell}\DropJunction");
        reg_add_value(&junction, "MUIVerb", "Drop Directory Junction")?;
        reg_add_value(&junction, "LegacyDisable", "")?;
    }

    let symbolic = format!(r"{shell}\DropSymbolic");
    reg_add_value(&symbolic, "MUIVerb", "Drop Symbolic Link")?;
    reg_add_default(
        &format!(r"{symbolic}\command"),
        &format!(r#""{helper}" drop-symlink "{target_token}""#),
    )
}

fn reg_add_value(key: &str, name: &str, value: &str) -> Result<()> {
    run_reg(["add", key, "/v", name, "/t", "REG_SZ", "/d", value, "/f"])
}

fn reg_add_default(key: &str, value: &str) -> Result<()> {
    run_reg(["add", key, "/ve", "/t", "REG_SZ", "/d", value, "/f"])
}

fn reg_delete_tree(key: &str) -> Result<()> {
    run_reg(["delete", key, "/f"])
}

fn run_reg<const N: usize>(args: [&str; N]) -> Result<()> {
    let status = Command::new("reg")
        .args(args)
        .status()
        .context("failed to start reg.exe")?;
    if status.success() {
        Ok(())
    } else {
        bail!("reg.exe failed with status {status}");
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
