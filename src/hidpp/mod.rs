//! HID++: Logitech's vendor protocol on top of USB/Bluetooth HID.
//!
//! * [`frame`] — the bytes on the wire.
//! * [`transport`] — finding and opening the receiver, request/response.
//! * [`features`] — the device functions logirev uses (buttons, speed...).
//! * [`receiver`] — receiver registers: which devices are paired to it.

pub mod features;
pub mod frame;
pub mod receiver;
pub mod transport;
