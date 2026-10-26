//! Actions on the user session that are not synthetic input.
//!
//! Locking the screen cannot be done with a simulated `Win+L`: Windows
//! reserves that shortcut and ignores it when it comes from `SendInput`, so
//! there is a system call for it. On Linux the session manager or the usual
//! screen lockers are asked instead.

#[cfg(target_os = "linux")]
use std::process::Command;

/// Locks the current session (the lock screen).
pub fn lock_screen() -> Result<(), String> {
    #[cfg(windows)]
    {
        // SAFETY: plain Win32 call without arguments.
        if unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() } != 0 {
            Ok(())
        } else {
            Err(format!(
                "LockWorkStation failed: {}",
                std::io::Error::last_os_error()
            ))
        }
    }
    #[cfg(target_os = "linux")]
    {
        // The session manager first, then the common screen lockers.
        let attempts: &[(&str, &[&str])] = &[
            ("loginctl", &["lock-session"]),
            ("xdg-screensaver", &["lock"]),
            ("gnome-screensaver-command", &["-l"]),
            (
                "qdbus",
                &["org.freedesktop.ScreenSaver", "/ScreenSaver", "Lock"],
            ),
        ];
        for (program, args) in attempts {
            if Command::new(program)
                .args(*args)
                .status()
                .is_ok_and(|status| status.success())
            {
                return Ok(());
            }
        }
        Err(
            "no screen locker answered (tried loginctl, xdg-screensaver, gnome-screensaver, qdbus)"
                .into(),
        )
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Err("locking the screen is not supported on this system".into())
    }
}
