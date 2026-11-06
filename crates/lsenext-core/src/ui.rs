pub fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub fn show_error(message: &str) {
    show_message("LSENext", message, true);
}

pub fn show_info(message: &str) {
    show_message("LSENext", message, false);
}

#[cfg(windows)]
pub fn show_message(title: &str, message: &str, is_error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let title_wide = wide_null(title);
    let message_wide = wide_null(message);
    let style = MB_OK | if is_error { MB_ICONERROR } else { 0 };
    unsafe {
        MessageBoxW(0, message_wide.as_ptr(), title_wide.as_ptr(), style);
    }
}

#[cfg(not(windows))]
pub fn show_message(title: &str, message: &str, is_error: bool) {
    if is_error {
        eprintln!("[{}] Error: {}", title, message);
    } else {
        println!("[{}] {}", title, message);
    }
}
