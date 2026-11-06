#[cfg(windows)]
use crate::ui::{ask_yes_no, wide_null};

#[cfg(windows)]
fn read_registry_dword(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    value_name: &str,
) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, KEY_READ, REG_DWORD,
    };
    let subkey_w = wide_null(subkey);
    let value_w = wide_null(value_name);
    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(root, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey) };
    if status != 0 {
        return None;
    }
    let mut data: u32 = 0;
    let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
    let mut val_type: u32 = 0;
    let query_status = unsafe {
        RegQueryValueExW(
            hkey,
            value_w.as_ptr(),
            std::ptr::null(),
            &mut val_type,
            &mut data as *mut u32 as *mut u8,
            &mut data_size,
        )
    };
    unsafe {
        RegCloseKey(hkey);
    }
    if query_status == 0 && val_type == REG_DWORD {
        Some(data)
    } else {
        None
    }
}

#[cfg(windows)]
fn write_registry_dword(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    value_name: &str,
    value: u32,
) -> Result<(), std::io::Error> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegSetValueExW, REG_DWORD,
    };
    let subkey_w = wide_null(subkey);
    let value_w = wide_null(value_name);
    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    let status = unsafe { RegCreateKeyW(root, subkey_w.as_ptr(), &mut hkey) };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    let set_status = unsafe {
        RegSetValueExW(
            hkey,
            value_w.as_ptr(),
            0,
            REG_DWORD,
            &value as *const u32 as *const u8,
            std::mem::size_of::<u32>() as u32,
        )
    };
    unsafe {
        RegCloseKey(hkey);
    }
    if set_status != 0 {
        return Err(std::io::Error::from_raw_os_error(set_status as i32));
    }
    Ok(())
}

pub fn is_developer_mode_enabled() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;
        read_registry_dword(
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\AppModelUnlock",
            "AllowDevelopmentWithoutDevLicense",
        )
        .map(|val| val == 1)
        .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn has_prompted_developer_mode() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
        read_registry_dword(
            HKEY_CURRENT_USER,
            "Software\\LSENext",
            "PromptedDeveloperMode",
        )
        .map(|val| val == 1)
        .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn mark_prompted_developer_mode() -> Result<(), std::io::Error> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
        write_registry_dword(
            HKEY_CURRENT_USER,
            "Software\\LSENext",
            "PromptedDeveloperMode",
            1,
        )
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

pub fn open_developer_settings() -> Result<(), std::io::Error> {
    #[cfg(windows)]
    {
        use std::ptr;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let verb = wide_null("open");
        let uri = wide_null("ms-settings:developers");

        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb.as_ptr(),
                uri.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL as i32,
            )
        };
        if (result as isize) <= 32 {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "ShellExecuteW failed to open ms-settings:developers with code {}",
                    result as isize
                ),
            ))
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Developer settings is only supported on Windows",
        ))
    }
}

#[cfg(windows)]
fn is_chinese_ui() -> bool {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    let lang_id = unsafe { GetUserDefaultUILanguage() };
    (lang_id & 0x00FF) == 0x04
}

pub fn prompt_developer_mode_if_needed(force: bool) -> bool {
    if is_developer_mode_enabled() {
        return false;
    }
    if !force && has_prompted_developer_mode() {
        return false;
    }

    let _ = mark_prompted_developer_mode();

    #[cfg(windows)]
    {
        let (title, message) = if is_chinese_ui() {
            (
                "LSENext",
                "检测到当前系统尚未开启「开发人员模式」。\n\n开启开发人员模式后，创建符号链接（软链接）无需每次经过管理员提权 (UAC)，体验更流畅。\n\n是否立即打开系统设置前往开启？",
            )
        } else {
            (
                "LSENext",
                "Windows Developer Mode is currently disabled on this system.\n\nEnabling Developer Mode allows creating symbolic links without UAC administrator prompts every time.\n\nWould you like to open Windows Settings to enable it now?",
            )
        };

        if ask_yes_no(title, message) {
            let _ = open_developer_settings();
            true
        } else {
            false
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_devmode_defaults_on_non_windows() {
        #[cfg(not(windows))]
        {
            assert!(!is_developer_mode_enabled());
            assert!(!has_prompted_developer_mode());
            assert!(mark_prompted_developer_mode().is_ok());
            assert!(!prompt_developer_mode_if_needed(false));
            assert!(open_developer_settings().is_err());
        }
    }
}
