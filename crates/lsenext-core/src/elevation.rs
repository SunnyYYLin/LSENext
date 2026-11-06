use crate::links::LinkKind;
#[cfg(windows)]
use crate::ui::wide_null;
use std::path::{Path, PathBuf};

pub fn elevate_drop(helper: &Path, kind: LinkKind, target: &Path) -> Result<(), std::io::Error> {
    let args = format!("{} \"{}\"", kind.drop_subcommand(), target.display());
    run_elevated(helper, &args)
}

pub fn elevate_create(
    helper: &Path,
    kind: LinkKind,
    target: &Path,
    sources: &[PathBuf],
) -> Result<(), std::io::Error> {
    let mut args = format!("create-links {} \"{}\"", kind.cli_name(), target.display());
    for source in sources {
        args.push_str(&format!(" \"{}\"", source.display()));
    }
    run_elevated(helper, &args)
}

#[cfg(windows)]
pub fn run_elevated(exe: &Path, args: &str) -> Result<(), std::io::Error> {
    use std::ptr;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = wide_null("runas");
    let file = wide_null(&exe.to_string_lossy());
    let params = wide_null(args);

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            ptr::null(),
            SW_SHOWNORMAL as i32,
        )
    };
    if (result as isize) <= 32 {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("ShellExecuteW failed with code {}", result as isize),
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn run_elevated(_exe: &Path, _args: &str) -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "ShellExecuteW elevation is only supported on Windows",
    ))
}
