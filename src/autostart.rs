//! "Start with Windows": the per-user `Run` registry key.
//!
//! Windows starts every program listed under
//! `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` when
//! the user logs in. Writing there needs no administrator rights and only
//! affects the current user. Because logirev is portable and may be moved,
//! the stored path is refreshed at every start while the option is on.

use std::path::Path;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};

/// UTF-16, NUL-terminated copy of `s`.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "logirev";

/// The command Windows should run: this executable, plus `--config` when a
/// non-default configuration file is in use.
pub fn command_line(exe: &Path, config: Option<&Path>) -> String {
    match config {
        Some(c) => format!("\"{}\" --config \"{}\"", exe.display(), c.display()),
        None => format!("\"{}\"", exe.display()),
    }
}

/// The command currently registered, if any.
pub fn current() -> Option<String> {
    let (key, value) = (wide(RUN_KEY), wide(VALUE_NAME));
    let mut size = 0u32;
    // SAFETY: first call only asks for the size of the value.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `buf` holds `size` bytes, as reported by the first call.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

pub fn is_enabled() -> bool {
    current().is_some()
}

/// Registers `command` to run at log-in.
pub fn enable(command: &str) -> bool {
    let (key, value, data) = (wide(RUN_KEY), wide(VALUE_NAME), wide(command));
    let bytes = (data.len() * 2) as u32;
    // SAFETY: all buffers are valid for the given lengths.
    let rc = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            bytes,
        )
    };
    rc == ERROR_SUCCESS
}

/// Removes the log-in entry.
pub fn disable() -> bool {
    let (key, value) = (wide(RUN_KEY), wide(VALUE_NAME));
    // SAFETY: NUL-terminated strings.
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) == ERROR_SUCCESS }
}

/// If the option is on but points to an old location, update it.
pub fn refresh(command: &str) {
    if let Some(stored) = current() {
        if stored != command {
            enable(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_command_lines() {
        let exe = Path::new(r"C:\Tools\logirev\logirev.exe");
        assert_eq!(command_line(exe, None), r#""C:\Tools\logirev\logirev.exe""#);
        assert_eq!(
            command_line(exe, Some(Path::new(r"D:\cfg.toml"))),
            r#""C:\Tools\logirev\logirev.exe" --config "D:\cfg.toml""#
        );
    }
}
