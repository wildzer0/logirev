//! Making sure only one copy of the service runs per user session.
//!
//! Two copies would both divert the buttons and perform every action twice.
//! The first copy takes a system-wide named lock; a second copy finds it
//! taken and exits with a clear message.
//!
//! * **Windows** — a named mutex `Local\logirev` (one per log-on session).
//! * **Linux** — an exclusive `flock` on `$XDG_RUNTIME_DIR/logirev.lock`.
//!
//! The lock is released automatically when the process ends, even on crash.

use std::io;

/// Holds the lock for as long as it is alive.
pub struct InstanceGuard {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(target_os = "linux")]
    _file: std::fs::File,
}

/// Takes the lock, or fails with `AlreadyExists` if another copy holds it.
pub fn acquire() -> io::Result<InstanceGuard> {
    imp::acquire()
}

fn already_running() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, "logirev is already running")
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    pub fn acquire() -> io::Result<InstanceGuard> {
        let name: Vec<u16> = "Local\\logirev\0".encode_utf16().collect();
        // SAFETY: `name` is a valid NUL-terminated UTF-16 string.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: plain FFI call reading the calling thread's last error.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: `handle` was returned by CreateMutexW above.
            unsafe { CloseHandle(handle) };
            return Err(already_running());
        }
        Ok(InstanceGuard { handle })
    }

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            // SAFETY: the handle is owned by this guard.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::path::PathBuf;

    pub fn acquire() -> io::Result<InstanceGuard> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                // SAFETY: getuid never fails.
                std::env::temp_dir().join(format!("logirev-{}", unsafe { libc::getuid() }))
            });
        std::fs::create_dir_all(&dir)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("logirev.lock"))?;
        // SAFETY: valid fd owned by `file`.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let err = io::Error::last_os_error();
            return Err(if err.kind() == io::ErrorKind::WouldBlock {
                already_running()
            } else {
                err
            });
        }
        Ok(InstanceGuard { _file: file })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::*;

    pub fn acquire() -> io::Result<InstanceGuard> {
        Ok(InstanceGuard {})
    }
}
