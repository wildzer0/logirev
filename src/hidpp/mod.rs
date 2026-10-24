//! HID++: Logitech's vendor protocol on top of USB/Bluetooth HID.
//!
//! * [`frame`] — the bytes on the wire.
//! * [`transport`] — finding and opening the receiver, request/response.
//! * [`controls`] — names of buttons and special keys.
//! * [`diagnostics`] — technical details for the developer mode.
//! * [`features`] — the device functions logirev uses (buttons, speed...).
//! * [`traffic`] — recent messages, for the developer mode.
//! * [`receiver`] — receiver registers: which devices are paired to it.
//! * [`pairing`] — pairing new devices and removing pairings.

pub mod controls;
pub mod diagnostics;
pub mod features;
pub mod frame;
pub mod pairing;
pub mod receiver;
pub mod traffic;
pub mod transport;
