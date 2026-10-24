//! Small Win32 helpers shared by the interface modules.

use std::path::Path;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, SW_SHOWNORMAL,
};

/// UTF-16, NUL-terminated copy of `s` (the string format of Win32).
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies `s` into a fixed-size Win32 buffer, truncating and terminating it.
pub fn copy_into(dst: &mut [u16], s: &str) {
    let max = dst.len().saturating_sub(1);
    let mut n = 0;
    for (slot, unit) in dst.iter_mut().zip(s.encode_utf16().take(max)) {
        *slot = unit;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

/// Shows a blocking message box (used before the tray icon exists).
pub fn message_box(title: &str, text: &str, error: bool) {
    let (t, c) = (wide(text), wide(title));
    let style = MB_OK
        | if error {
            MB_ICONERROR
        } else {
            MB_ICONINFORMATION
        };
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), style) };
}

/// Opens a file with its associated program; text files without an
/// association (such as `.toml`) fall back to Notepad.
pub fn open_file(hwnd: HWND, path: &Path) -> bool {
    let file = wide(&path.to_string_lossy());
    if shell_execute(hwnd, &file, None) {
        return true;
    }
    let quoted = wide(&format!("\"{}\"", path.display()));
    shell_execute(hwnd, &wide("notepad.exe"), Some(&quoted))
}

/// Opens a folder in Explorer.
pub fn open_folder(hwnd: HWND, dir: &Path) -> bool {
    shell_execute(hwnd, &wide(&dir.to_string_lossy()), None)
}

fn shell_execute(hwnd: HWND, file: &[u16], params: Option<&[u16]>) -> bool {
    let verb = wide("open");
    let params = params.map_or(std::ptr::null(), <[u16]>::as_ptr);
    // SAFETY: all strings are NUL-terminated and live until the call returns.
    let result = unsafe {
        ShellExecuteW(
            hwnd,
            verb.as_ptr(),
            file.as_ptr(),
            params,
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value greater than 32 on success.
    result as isize > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_truncates() {
        let mut buf = [0xFFFFu16; 5];
        copy_into(&mut buf, "abcdefgh");
        assert_eq!(buf, [97, 98, 99, 100, 0]);
        copy_into(&mut buf, "x");
        assert_eq!(&buf[..2], &[120, 0]);
        assert_eq!(wide("ab"), vec![97, 98, 0]);
    }
}
