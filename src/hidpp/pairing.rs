//! Pairing new devices with a receiver, and removing pairings.
//!
//! This replaces the "Logi Bolt" / "Unifying" pairing apps. Two receiver
//! families work differently:
//!
//! # Unifying (and Nano receivers with Unifying-style registers)
//!
//! The receiver has a *pairing lock*. Opening it (register `0xB2`, action
//! `0x01`) lets the first device that turns on or presses its connect
//! button join a free slot; no code is needed. The receiver reports the
//! lock state with notification `0x4A`, and the new device announces itself
//! with the usual connection notification `0x41` on its new slot.
//!
//! ```text
//! → 10 FF 80 B2 01 00 1E        open the lock for 30 s
//! ← 10 FF 4A 01 00 ...          lock open
//! ← 10 03 41 ...                a device connected on slot 3 (new)
//! ← 10 FF 4A 00 00 ...          lock closed (error 00 = none)
//! ```
//!
//! # Bolt
//!
//! Bolt pairing is authenticated, so it happens in steps:
//!
//! 1. **Discovery** — register `0xC0` (`timeout`, action `0x01`). The receiver
//!    reports "discovery on/off" with notification `0x53` and every device
//!    that is in pairing mode with notifications `0x4F`: one carries its
//!    radio address, type and authentication method, a second its name.
//! 2. **Pairing request** — long register `0xC1`: action `0x01`, slot `0`
//!    (= first free), the 6-byte address, the authentication method and the
//!    number of passkey bits (20 for keyboards, 10 for mice).
//! 3. **Passkey** — notification `0x4D` carries a 6-digit code. On a
//!    keyboard (authentication bit 0 set) the user types it and presses
//!    Enter. On a mouse the code is a 10-bit number entered as a sequence
//!    of left (0) / right (1) clicks, finished by pressing both buttons.
//!    Notification `0x4E` reports each key/click.
//! 4. **Result** — notification `0x54`: `address 0x02` with no error means
//!    paired, and byte 7 of the data is the slot.
//!
//! # Removing a pairing
//!
//! Unifying: register `0xB2`, action `0x03`, slot. Bolt: long register
//! `0xC1`, action `0x03`, slot. The device can be paired again at any time.
//!
//! Pairing changes only the receiver's list of devices; it never touches
//! device firmware.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::features::DeviceKind;
use super::frame::{Frame, HidppError, RECEIVER_INDEX};
use super::receiver::kind_from_code;
use super::transport::{receiver_name, Transport};

/// Receiver register: Unifying pairing lock.
const REG_UNIFYING_PAIRING: u8 = 0xB2;
/// Receiver register: Bolt discovery.
const REG_BOLT_DISCOVERY: u8 = 0xC0;
/// Receiver long register: Bolt pairing.
const REG_BOLT_PAIRING: u8 = 0xC1;

/// Notification: Unifying pairing lock state.
const NOTIF_PAIRING_LOCK: u8 = 0x4A;
/// Notification: Bolt passkey to enter.
const NOTIF_PASSKEY_REQUEST: u8 = 0x4D;
/// Notification: Bolt passkey key/click entered.
const NOTIF_PASSKEY_PRESSED: u8 = 0x4E;
/// Notification: Bolt device found during discovery.
const NOTIF_DEVICE_DISCOVERY: u8 = 0x4F;
/// Notification: Bolt discovery on/off.
const NOTIF_DISCOVERY_STATUS: u8 = 0x53;
/// Notification: Bolt pairing state / result.
const NOTIF_PAIRING_STATUS: u8 = 0x54;
/// Notification: a device connected or disconnected.
const NOTIF_DEVICE_CONNECTION: u8 = 0x41;

/// How long the receiver listens for a new device.
pub const PAIRING_TIMEOUT_SECS: u8 = 30;
/// Give up if the receiver says nothing for this long.
const STALL: Duration = Duration::from_secs(PAIRING_TIMEOUT_SECS as u64 + 15);

/// The two pairing protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    /// Unifying (and Nano) receivers: pairing lock, no code.
    Unifying,
    /// Bolt receivers: discovery and authenticated pairing.
    Bolt,
}

impl Family {
    /// The pairing protocol of a receiver product id, if it can pair.
    pub fn of(pid: u16) -> Option<Self> {
        match receiver_name(pid)? {
            "Bolt" => Some(Self::Bolt),
            "Unifying" | "Nano" => Some(Self::Unifying),
            _ => None,
        }
    }
}

/// A device found by Bolt discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    /// Identifies the announcement (the receiver numbers them).
    pub counter: u16,
    /// Radio address, needed to pair.
    pub address: [u8; 6],
    /// Device type announced by the device.
    pub kind: DeviceKind,
    /// Authentication method; bit 0 = typed passkey (keyboards).
    pub authentication: u8,
    /// Name, when the second announcement arrived.
    pub name: Option<String>,
}

impl Found {
    /// Typed code (keyboards) rather than clicks (mice).
    pub fn uses_keyboard_passkey(&self) -> bool {
        self.authentication & 0x01 != 0
    }
}

/// Where a pairing stands, as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Stage {
    /// The receiver is listening; the user must put the device in pairing
    /// mode. For Bolt, found devices are listed in [`Pairing::found`].
    Waiting,
    /// A pairing request was sent; waiting for the receiver.
    Pairing,
    /// The user must enter `passkey`: typed on a keyboard, or clicked.
    Passkey {
        /// The 6-digit code.
        passkey: String,
        /// Typed on a keyboard (`true`) or clicked on a mouse.
        keyboard: bool,
        /// Keys/clicks entered so far.
        entered: u8,
    },
    /// Done: the new device is in `slot`.
    Paired {
        /// Receiver slot of the new device.
        slot: u8,
    },
    /// Nothing paired (timeout or error).
    Failed {
        /// Why, in plain English.
        reason: String,
    },
    /// Stopped by the user.
    Cancelled,
}

impl Stage {
    /// The pairing is over (successfully or not).
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            Stage::Paired { .. } | Stage::Failed { .. } | Stage::Cancelled
        )
    }
}

/// One pairing session on one receiver.
#[derive(Debug, Clone)]
pub struct Pairing {
    /// Pairing protocol of the receiver.
    pub family: Family,
    /// Where the session stands.
    pub stage: Stage,
    /// Bolt: devices found so far.
    pub found: Vec<Found>,
    /// Slots that were already paired when the session started (Unifying:
    /// a connection on another slot is the new device).
    known_slots: Vec<u8>,
    /// The device chosen for pairing (Bolt).
    chosen: Option<Found>,
    /// Last time the receiver told us something.
    last_news: Instant,
}

impl Pairing {
    /// Puts the receiver in pairing mode. `known_slots` are the slots in use.
    pub fn start(t: &mut Transport, known_slots: Vec<u8>) -> Result<Self, HidppError> {
        let family =
            Family::of(t.pid).ok_or(HidppError::Io("this receiver cannot pair devices".into()))?;
        match family {
            Family::Unifying => {
                t.write_register(REG_UNIFYING_PAIRING, &[0x01, 0x00, PAIRING_TIMEOUT_SECS])?;
            }
            Family::Bolt => {
                t.write_register(REG_BOLT_DISCOVERY, &[PAIRING_TIMEOUT_SECS, 0x01])?;
            }
        }
        Ok(Self {
            family,
            stage: Stage::Waiting,
            found: Vec::new(),
            known_slots,
            chosen: None,
            last_news: Instant::now(),
        })
    }

    /// Bolt: pairs the `index`-th found device (starts the passkey step).
    pub fn choose(&mut self, t: &mut Transport, index: usize) -> Result<(), HidppError> {
        let device = self
            .found
            .get(index)
            .cloned()
            .ok_or(HidppError::MalformedResponse)?;
        let entropy = if device.kind == DeviceKind::Keyboard {
            20
        } else {
            10
        };
        let mut params = vec![0x01, 0x00];
        params.extend_from_slice(&device.address);
        params.extend_from_slice(&[device.authentication, entropy]);
        t.write_long_register(REG_BOLT_PAIRING, &params)?;
        self.chosen = Some(device);
        self.stage = Stage::Pairing;
        self.last_news = Instant::now();
        Ok(())
    }

    /// Stops listening (best effort) and marks the session cancelled.
    pub fn cancel(&mut self, t: &mut Transport) {
        if !self.stage.is_finished() {
            let _ = match self.family {
                Family::Unifying => t.write_register(REG_UNIFYING_PAIRING, &[0x02, 0x00, 0x00]),
                Family::Bolt => match self.stage {
                    Stage::Waiting => t.write_register(REG_BOLT_DISCOVERY, &[0x00, 0x02]),
                    _ => t.write_long_register(REG_BOLT_PAIRING, &[0x02]),
                },
            };
            self.stage = Stage::Cancelled;
        }
    }

    /// The device chosen for pairing (Bolt).
    pub fn chosen(&self) -> Option<&Found> {
        self.chosen.as_ref()
    }

    /// Fails the session if the receiver has been silent for too long.
    pub fn check_timeout(&mut self) -> bool {
        if !self.stage.is_finished() && self.last_news.elapsed() > STALL {
            self.stage = Stage::Failed {
                reason: "no device answered in time".into(),
            };
            return true;
        }
        false
    }

    /// Feeds a frame received from the receiver; returns true if the
    /// session changed.
    pub fn on_frame(&mut self, f: &Frame) -> bool {
        if self.stage.is_finished() {
            return false;
        }
        let (index, sub, address, data) = (f.device_index(), f.b2(), f.b3(), f.params());
        // A device connecting on a slot that was free: the new device.
        if sub == NOTIF_DEVICE_CONNECTION && index != RECEIVER_INDEX {
            let link_up = data.first().is_some_and(|b| b & 0x40 == 0);
            if link_up && !self.known_slots.contains(&index) {
                self.stage = Stage::Paired { slot: index };
                return true;
            }
            return false;
        }
        if index != RECEIVER_INDEX {
            return false;
        }
        let error = data.first().copied().unwrap_or(0);
        let changed = match sub {
            NOTIF_PAIRING_LOCK if self.family == Family::Unifying => {
                let open = address & 0x01 != 0;
                if error != 0 {
                    self.stage = Stage::Failed {
                        reason: unifying_error(error).into(),
                    };
                } else if !open && self.stage == Stage::Waiting {
                    self.stage = Stage::Failed {
                        reason: "no device answered in time".into(),
                    };
                }
                true
            }
            NOTIF_DISCOVERY_STATUS => {
                // Discovery closing while still waiting with nothing chosen.
                if error != 0 {
                    self.stage = Stage::Failed {
                        reason: bolt_error(error).into(),
                    };
                } else if address != 0x00 && self.stage == Stage::Waiting && self.found.is_empty() {
                    self.stage = Stage::Failed {
                        reason: "no device answered in time".into(),
                    };
                }
                true
            }
            NOTIF_DEVICE_DISCOVERY => self.on_discovery(address, data),
            NOTIF_PASSKEY_REQUEST => {
                let passkey: String = data.iter().take(6).map(|b| *b as char).collect();
                let keyboard = self
                    .chosen
                    .as_ref()
                    .is_some_and(Found::uses_keyboard_passkey);
                self.stage = Stage::Passkey {
                    passkey,
                    keyboard,
                    entered: 0,
                };
                true
            }
            NOTIF_PASSKEY_PRESSED => {
                if let Stage::Passkey { entered, .. } = &mut self.stage {
                    *entered = entered.saturating_add(1);
                }
                true
            }
            NOTIF_PAIRING_STATUS => {
                if error != 0 {
                    self.stage = Stage::Failed {
                        reason: bolt_error(error).into(),
                    };
                } else if address == 0x02 {
                    self.stage = Stage::Paired {
                        slot: data.get(7).copied().unwrap_or(0),
                    };
                }
                true
            }
            _ => false,
        };
        if changed {
            self.last_news = Instant::now();
        }
        changed
    }

    /// Bolt discovery announcements (two per device, same counter).
    fn on_discovery(&mut self, address: u8, data: &[u8]) -> bool {
        if data.len() < 3 {
            return false;
        }
        let counter = u16::from(address) | (u16::from(data[0]) << 8);
        match data[1] {
            0 if data.len() >= 15 => {
                let mut addr = [0u8; 6];
                addr.copy_from_slice(&data[6..12]);
                let entry = Found {
                    counter,
                    address: addr,
                    kind: kind_from_code(data[3] & 0x0F),
                    authentication: data[14],
                    name: None,
                };
                match self.found.iter_mut().find(|f| f.counter == counter) {
                    Some(f) => {
                        f.address = entry.address;
                        f.kind = entry.kind;
                        f.authentication = entry.authentication;
                    }
                    None => self.found.push(entry),
                }
                true
            }
            1 => {
                let len = usize::from(data[2]);
                let name = String::from_utf8_lossy(data.get(3..3 + len).unwrap_or_default())
                    .trim()
                    .to_owned();
                match self.found.iter_mut().find(|f| f.counter == counter) {
                    Some(f) => f.name = Some(name),
                    None => self.found.push(Found {
                        counter,
                        address: [0; 6],
                        kind: DeviceKind::Other,
                        authentication: 0,
                        name: Some(name),
                    }),
                }
                true
            }
            _ => false,
        }
    }
}

/// Removes the device in `slot` from the receiver.
pub fn unpair(t: &mut Transport, slot: u8) -> Result<(), HidppError> {
    match Family::of(t.pid).ok_or(HidppError::Io(
        "this receiver cannot remove pairings".into(),
    ))? {
        Family::Unifying => t
            .write_register(REG_UNIFYING_PAIRING, &[0x03, slot])
            .map(drop),
        Family::Bolt => t
            .write_long_register(REG_BOLT_PAIRING, &[0x03, slot])
            .map(drop),
    }
}

/// The clicks that enter a mouse passkey: the code as a 10-bit binary
/// number, most significant bit first; `false` = left click, `true` =
/// right click. Afterwards both buttons are pressed together.
pub fn mouse_clicks(passkey: &str) -> Vec<bool> {
    let value: u32 = passkey.trim().parse().unwrap_or(0);
    (0..10).rev().map(|bit| value & (1 << bit) != 0).collect()
}

fn unifying_error(code: u8) -> &'static str {
    match code {
        0x01 => "no device answered in time",
        0x02 => "this device cannot be paired with this receiver",
        0x03 => "the receiver already has the maximum number of devices",
        0x06 => "the pairing sequence timed out",
        _ => "pairing failed",
    }
}

fn bolt_error(code: u8) -> &'static str {
    match code {
        0x01 => "no device answered in time",
        _ => "pairing failed (wrong code or device out of range)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(index: u8, sub: u8, address: u8, data: &[u8]) -> Frame {
        let mut payload = vec![sub, address];
        payload.extend_from_slice(data);
        Frame::new(0x11, index, &payload)
    }

    fn session(family: Family, known: Vec<u8>) -> Pairing {
        Pairing {
            family,
            stage: Stage::Waiting,
            found: Vec::new(),
            known_slots: known,
            chosen: None,
            last_news: Instant::now(),
        }
    }

    #[test]
    fn unifying_new_slot_is_the_new_device() {
        let mut p = session(Family::Unifying, vec![1, 2]);
        assert!(p.on_frame(&frame(0xFF, NOTIF_PAIRING_LOCK, 0x01, &[0])));
        assert_eq!(p.stage, Stage::Waiting);
        // A known device reconnecting is not the new one.
        assert!(!p.on_frame(&frame(2, NOTIF_DEVICE_CONNECTION, 0x10, &[0x00])));
        assert!(p.on_frame(&frame(3, NOTIF_DEVICE_CONNECTION, 0x10, &[0x00])));
        assert_eq!(p.stage, Stage::Paired { slot: 3 });
    }

    #[test]
    fn unifying_timeout_and_errors() {
        let mut p = session(Family::Unifying, vec![]);
        p.on_frame(&frame(0xFF, NOTIF_PAIRING_LOCK, 0x00, &[0]));
        assert!(matches!(p.stage, Stage::Failed { .. }));
        let mut p = session(Family::Unifying, vec![]);
        p.on_frame(&frame(0xFF, NOTIF_PAIRING_LOCK, 0x00, &[0x03]));
        assert_eq!(
            p.stage,
            Stage::Failed {
                reason: unifying_error(3).into()
            }
        );
    }

    #[test]
    fn bolt_discovery_passkey_and_result() {
        let mut p = session(Family::Bolt, vec![1, 2]);
        p.on_frame(&frame(0xFF, NOTIF_DISCOVERY_STATUS, 0x00, &[0]));
        // First announcement: kind 2 (mouse), address, authentication 0.
        let mut d = vec![0x00, 0x00, 0x00, 0x02, 0, 0, 1, 2, 3, 4, 5, 6, 0, 0, 0x00];
        p.on_frame(&frame(0xFF, NOTIF_DEVICE_DISCOVERY, 0x07, &d));
        // Second announcement: name.
        d = vec![0x00, 0x01, 0x09];
        d.extend_from_slice(b"MX Ergo S");
        p.on_frame(&frame(0xFF, NOTIF_DEVICE_DISCOVERY, 0x07, &d));
        assert_eq!(p.found.len(), 1);
        let f = &p.found[0];
        assert_eq!(
            (f.counter, f.address, f.kind),
            (7, [1, 2, 3, 4, 5, 6], DeviceKind::Mouse)
        );
        assert_eq!(f.name.as_deref(), Some("MX Ergo S"));
        assert!(!f.uses_keyboard_passkey());

        p.chosen = Some(p.found[0].clone());
        p.stage = Stage::Pairing;
        p.on_frame(&frame(0xFF, NOTIF_PASSKEY_REQUEST, 0x00, b"000613"));
        assert_eq!(
            p.stage,
            Stage::Passkey {
                passkey: "000613".into(),
                keyboard: false,
                entered: 0
            }
        );
        p.on_frame(&frame(0xFF, NOTIF_PASSKEY_PRESSED, 0x00, &[0]));
        assert!(matches!(p.stage, Stage::Passkey { entered: 1, .. }));
        p.on_frame(&frame(
            0xFF,
            NOTIF_PAIRING_STATUS,
            0x02,
            &[0, 0, 0, 0, 0, 0, 0, 4],
        ));
        assert_eq!(p.stage, Stage::Paired { slot: 4 });
        assert!(
            !p.on_frame(&frame(0xFF, NOTIF_PAIRING_STATUS, 0x02, &[1])),
            "finished sessions ignore frames"
        );
    }

    #[test]
    fn bolt_failures() {
        let mut p = session(Family::Bolt, vec![]);
        p.on_frame(&frame(0xFF, NOTIF_DISCOVERY_STATUS, 0x01, &[0]));
        assert!(
            matches!(p.stage, Stage::Failed { .. }),
            "discovery closed with nothing found"
        );
        let mut p = session(Family::Bolt, vec![]);
        p.stage = Stage::Pairing;
        p.on_frame(&frame(0xFF, NOTIF_PAIRING_STATUS, 0x01, &[0x02]));
        assert_eq!(
            p.stage,
            Stage::Failed {
                reason: bolt_error(2).into()
            }
        );
    }

    #[test]
    fn mouse_passkey_is_ten_clicks() {
        // 613 = 0b1001100101
        let clicks = mouse_clicks("000613");
        let pattern: String = clicks.iter().map(|r| if *r { 'R' } else { 'L' }).collect();
        assert_eq!(pattern, "RLLRRLLRLR");
        assert_eq!(mouse_clicks("0").len(), 10);
    }

    #[test]
    fn families() {
        assert_eq!(Family::of(0xC548), Some(Family::Bolt));
        assert_eq!(Family::of(0xC52B), Some(Family::Unifying));
        assert_eq!(
            Family::of(0xC539),
            None,
            "Lightspeed receivers are paired at the factory"
        );
    }
}
