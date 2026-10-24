//! Graphical interface.
//!
//! On Windows `logirev.exe` has no console window: it shows an icon in the
//! notification area (next to the clock) with a menu listing the receivers
//! and their paired devices, a small device window, and a "Start with
//! Windows" option. On Linux the service stays headless.
//!
//! * [`view`] — what the menu and the window contain (portable, tested);
//! * `windows` — the Win32 implementation.

pub mod view;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{message_box, run};
