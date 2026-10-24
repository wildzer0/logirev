//! Receiver-level information: which devices are paired to which slot.
//!
//! A receiver remembers its paired devices even while they are switched off.
//! This information lives in the receiver's *long register* `0xB5`, one
//! sub-address per slot. The layout differs between receiver families:
//!
//! | Family   | Pairing info      | Device name                     |
//! |----------|-------------------|---------------------------------|
//! | Unifying | `0x20 + slot - 1` | `0x40 + slot - 1`               |
//! | Bolt     | `0x50 + slot`     | `0x60 + slot` (extra param `1`) |
//!
//! Only reading is performed; parsing is lenient so that an unexpected
//! layout produces "unknown" rather than an error.

use log::debug;

use super::features::DeviceKind;
use super::transport::{receiver_name, Transport};

/// Long register holding pairing data.
const REG_RECEIVER_INFO: u8 = 0xB5;

/// Maximum number of devices a Bolt/Unifying receiver can pair.
pub const MAX_SLOTS: u8 = 6;

/// A device remembered by the receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    /// Receiver slot, 1–6.
    pub slot: u8,
    /// Wireless product id (identifies the model).
    pub wpid: u16,
    /// Device type as recorded by the receiver.
    pub kind: DeviceKind,
    /// Name stored in the receiver (may be shortened by the firmware).
    pub name: Option<String>,
}

/// Reads the list of paired devices. Returns an empty list for receivers
/// whose layout is unknown (Nano, Lightspeed) or on any read error.
pub fn paired_devices(t: &mut Transport) -> Vec<PairedDevice> {
    let family = receiver_name(t.pid).unwrap_or("");
    (1..=MAX_SLOTS)
        .filter_map(|slot| match family {
            "Bolt" => read_bolt(t, slot),
            "Unifying" => read_unifying(t, slot),
            _ => None,
        })
        .collect()
}

fn read_bolt(t: &mut Transport, slot: u8) -> Option<PairedDevice> {
    let info = t
        .read_long_register(REG_RECEIVER_INFO, &[0x50 + slot])
        .ok()?;
    debug!("bolt pairing info slot {slot}: {info:02X?}");
    let mut dev = parse_bolt_info(slot, &info)?;
    if let Ok(raw) = t.read_long_register(REG_RECEIVER_INFO, &[0x60 + slot, 0x01]) {
        debug!("bolt device name slot {slot}: {raw:02X?}");
        dev.name = parse_name(&raw, 2);
    }
    Some(dev)
}

fn read_unifying(t: &mut Transport, slot: u8) -> Option<PairedDevice> {
    let info = t
        .read_long_register(REG_RECEIVER_INFO, &[0x20 + slot - 1])
        .ok()?;
    debug!("unifying pairing info slot {slot}: {info:02X?}");
    let mut dev = parse_unifying_info(slot, &info)?;
    if let Ok(raw) = t.read_long_register(REG_RECEIVER_INFO, &[0x40 + slot - 1]) {
        dev.name = parse_name(&raw, 1);
    }
    Some(dev)
}

/// Bolt: `[sub, kind, wpid_lo, wpid_hi, serial x4, ...]`.
fn parse_bolt_info(slot: u8, p: &[u8]) -> Option<PairedDevice> {
    if p.len() < 4 {
        return None;
    }
    let wpid = u16::from_le_bytes([p[2], p[3]]);
    (wpid != 0).then(|| PairedDevice {
        slot,
        wpid,
        kind: kind_from_code(p[1] & 0x0F),
        name: None,
    })
}

/// Unifying: `[sub, destination, interval, wpid_hi, wpid_lo, x, x, kind, ...]`.
fn parse_unifying_info(slot: u8, p: &[u8]) -> Option<PairedDevice> {
    if p.len() < 8 {
        return None;
    }
    let wpid = u16::from_be_bytes([p[3], p[4]]);
    (wpid != 0).then(|| PairedDevice {
        slot,
        wpid,
        kind: kind_from_code(p[7] & 0x0F),
        name: None,
    })
}

/// Name registers: a length byte at `len_at` followed by the characters.
fn parse_name(p: &[u8], len_at: usize) -> Option<String> {
    let len = usize::from(*p.get(len_at)?);
    let bytes = p.get(len_at + 1..)?;
    let bytes = &bytes[..len.min(bytes.len())];
    let name: String = String::from_utf8_lossy(bytes)
        .trim_matches(char::from(0))
        .trim()
        .to_owned();
    (!name.is_empty() && name.chars().all(|c| !c.is_control())).then_some(name)
}

/// Device kind codes used by receivers (not the same as feature `0x0005`).
pub fn kind_from_code(code: u8) -> DeviceKind {
    match code {
        1 => DeviceKind::Keyboard,
        2 => DeviceKind::Mouse,
        3 => DeviceKind::Numpad,
        4 => DeviceKind::Presenter,
        7 => DeviceKind::Remote,
        8 => DeviceKind::Trackball,
        9 => DeviceKind::Trackpad,
        _ => DeviceKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bolt_layout() {
        let info = [0x52, 0x08, 0x3F, 0xB0, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0];
        let dev = parse_bolt_info(2, &info).unwrap();
        assert_eq!((dev.wpid, dev.kind), (0xB03F, DeviceKind::Trackball));
        let name = [
            0x62, 0x01, 0x09, b'M', b'X', b' ', b'E', b'r', b'g', b'o', b' ', b'S', 0, 0, 0,
        ];
        assert_eq!(parse_name(&name, 2).as_deref(), Some("MX Ergo S"));
    }

    #[test]
    fn parses_unifying_layout() {
        let info = [
            0x21, 0x00, 0x08, 0x40, 0x6F, 0, 0, 0x08, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let dev = parse_unifying_info(2, &info).unwrap();
        assert_eq!((dev.wpid, dev.kind), (0x406F, DeviceKind::Trackball));
        assert_eq!(
            parse_name(&[0x41, 3, b'M', b'X', b'5'], 1).as_deref(),
            Some("MX5")
        );
    }

    #[test]
    fn empty_slots_are_ignored() {
        assert!(parse_bolt_info(1, &[0x51, 0, 0, 0, 0]).is_none());
        assert!(parse_name(&[0x61, 1, 0], 2).is_none());
    }
}
