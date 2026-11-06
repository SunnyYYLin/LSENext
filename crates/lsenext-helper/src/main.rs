#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnostics;
mod powershell;
mod registration;

use anyhow::{bail, Context, Result};
use lsenext_core::{
    ask_yes_no, clear_state, elevate_create, execute_batch, is_developer_mode_enabled, load_state,
    open_developer_settings, prompt_developer_mode_if_needed, save_sources, show_error, show_info,
    BatchLinkError, LinkKind, PickedSource,
};
use registration::{
    prepare_machine_registration, register_package_identity,
    trust_package_certificate_machine, unregister_package_identity,
};
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
        "drop-relative-symlink" => drop_links(LinkKind::RelativeSymbolic, args.next())?,
        "drop-junction" => drop_links(LinkKind::Junction, args.next())?,
        "drop-hardlink" => drop_links(LinkKind::HardLink, args.next())?,
        "create-links" => {
            let kind_str = args.next().context("link kind argument is required")?;
            let target = args
                .next()
                .map(PathBuf::from)
                .context("target directory argument is required")?;
            let sources: Vec<PathBuf> = args.map(PathBuf::from).collect();
            if sources.is_empty() {
                bail!("at least one source path is required");
            }
            let kind = LinkKind::from_cli_name(&kind_str)
                .ok_or_else(|| anyhow::anyhow!("unknown link kind: {}", kind_str))?;
            create_links(kind, &target, &sources)?;
        }
        "clear" => {
            clear_state()?;
        }
        "about" => {
            show_about();
        }
        "check-dev-mode" => {
            prompt_developer_mode_if_needed(true);
        }
        "open-dev-settings" => {
            let _ = open_developer_settings();
        }
        #[cfg(feature = "diagnostics")]
        "diagnostics" => {
            diagnostics::show_diagnostics()?;
        }
        #[cfg(not(feature = "diagnostics"))]
        "diagnostics" => bail!("LSENext diagnostics are only available in debug builds"),
        #[cfg(feature = "diagnostics")]
        "repair-native-menu" => {
            diagnostics::launch_repair_native_menu()?;
        }
        #[cfg(not(feature = "diagnostics"))]
        "repair-native-menu" => bail!("LSENext repair is only available in debug builds"),
        #[cfg(feature = "diagnostics")]
        "repair-native-menu-run" => diagnostics::repair_native_menu()?,
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
            bail!("usage: lsenext-helper <pick-source|drop-symlink|drop-relative-symlink|drop-junction|drop-hardlink|create-links|clear|about|check-dev-mode|open-dev-settings|register-package|prepare-machine-registration|trust-package-certificate-machine|unregister-package> [paths]");
        }
    }
    Ok(())
}

fn drop_links(kind: LinkKind, target: Option<String>) -> Result<()> {
    let target = target
        .map(PathBuf::from)
        .context("target directory argument is required")?;
    let state = load_state()?.context("no picked LSENext source is stored")?;

    if let Err(batch_err) = execute_batch(kind, &target, &state.sources) {
        if batch_err.is_elevation_required() {
            let exe = env::current_exe().context("failed to locate LSENext helper")?;
            if let BatchLinkError::Failed {
                remaining_sources, ..
            } = &batch_err
            {
                elevate_create(&exe, kind, &target, remaining_sources)?;
                return Ok(());
            }
        }
        return Err(batch_err.into());
    }
    Ok(())
}

fn create_links(kind: LinkKind, target: &Path, sources: &[PathBuf]) -> Result<()> {
    let picked: Vec<PickedSource> = sources
        .iter()
        .map(|p| PickedSource {
            path: p.clone(),
            is_dir: p.is_dir(),
        })
        .collect();

    if let Err(batch_err) = execute_batch(kind, target, &picked) {
        if batch_err.is_elevation_required() {
            let exe = env::current_exe().context("failed to locate LSENext helper")?;
            if let BatchLinkError::Failed {
                remaining_sources, ..
            } = &batch_err
            {
                elevate_create(&exe, kind, target, remaining_sources)?;
                return Ok(());
            }
        }
        return Err(batch_err.into());
    }
    Ok(())
}

fn show_about() {
    let dev_mode = is_developer_mode_enabled();
    let dev_status = if dev_mode {
        "Developer Mode: Enabled (Symbolic links require no UAC)"
    } else {
        "Developer Mode: Disabled (Enable in Settings for no-UAC symbolic links)"
    };
    let base_info = format!(
        "LSENext {}\nQuick symbolic, hard link, and directory junction creation.\n\n{}",
        env!("CARGO_PKG_VERSION"),
        dev_status,
    );
    if !dev_mode {
        let question = format!(
            "{}\n\nWould you like to open Windows Settings to enable Developer Mode now?",
            base_info,
        );
        if ask_yes_no("LSENext", &question) {
            let _ = open_developer_settings();
        }
    } else {
        show_info(&base_info);
    }
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
