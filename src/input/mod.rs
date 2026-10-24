//! Synthetic keyboard and mouse input.
//!
//! When a diverted button is pressed, the device stops sending its normal
//! input and the daemon *injects* the configured keys or clicks instead, as if
//! they came from a real keyboard or mouse:
//!
//! * **Windows** — the `SendInput` API (no driver, no admin rights).
//! * **Linux** — a virtual input device created through `/dev/uinput`
//!   (works on X11 and Wayland; needs write access to `/dev/uinput`).

pub mod keys;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use std::io;

pub use keys::{Key, MouseButton};

/// Something that can press and release keys and mouse buttons.
pub trait Injector {
    fn key(&mut self, key: &Key, down: bool) -> io::Result<()>;
    fn button(&mut self, button: MouseButton, down: bool) -> io::Result<()>;
}

/// Creates the injector for the current operating system.
pub fn create() -> io::Result<Box<dyn Injector>> {
    #[cfg(target_os = "linux")]
    return Ok(Box::new(linux::Uinput::new()?));
    #[cfg(windows)]
    return Ok(Box::new(windows::SendInput));
    #[cfg(not(any(target_os = "linux", windows)))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "input injection is not implemented on this OS",
    ));
}
