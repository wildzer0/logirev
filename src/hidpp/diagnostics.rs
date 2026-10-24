//! Technical information for the developer mode: identity of a device,
//! every firmware it runs, and its full feature list.
//!
//! Everything here only *reads* from the device (DeviceFwInfo `0x0003` and
//! FeatureSet `0x0001`); nothing is changed. The one exception is
//! [`list_hidden_features`], which briefly switches on Logitech's
//! "EnableHiddenFeatures" (`0x1E00`) to see what it unlocks, then puts it
//! back as it was.

use serde::{Deserialize, Serialize};

use super::features::{id, Device, FeatureEntry, FeatureTable};
use super::frame::HidppError;
use super::transport::Transport;

/// What a device says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechInfo {
    /// Unique id of this unit (4 bytes, hexadecimal).
    pub unit_id: String,
    /// Serial number, when the device has one.
    pub serial: Option<String>,
    /// Model ids, one per way of connecting, e.g. `eQuad B03E`.
    pub model_ids: Vec<String>,
    /// How the device can connect: `Bluetooth`, `BLE`, `eQuad` (Unifying /
    /// Bolt receiver), `USB`.
    pub transports: Vec<String>,
    /// Every firmware component (main program, bootloader, radio...).
    pub firmware: Vec<FirmwareEntity>,
    /// Every feature of the device.
    pub features: Vec<FeatureEntry>,
}

/// One firmware component.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareEntity {
    /// What it is: `application`, `bootloader`, `hardware`...
    pub kind: String,
    /// Version, e.g. `RBM 26.01.B0013`.
    pub version: String,
    /// Product id this firmware answers to, when given.
    pub pid: Option<u16>,
    /// The component currently running.
    pub active: bool,
}

/// Name of a DeviceFwInfo entity type.
fn entity_kind(code: u8) -> String {
    match code {
        0 => "application".into(),
        1 => "bootloader".into(),
        2 => "hardware".into(),
        3 => "touchpad".into(),
        4 => "optical sensor".into(),
        5 => "radio stack".into(),
        6 => "radio co-processor".into(),
        n => format!("type {n}"),
    }
}

/// Reads identity, firmware list and features. Missing parts stay empty.
pub fn read(t: &mut Transport, slot: u8, features: &FeatureTable) -> TechInfo {
    let mut info = TechInfo {
        features: features.entries(),
        ..TechInfo::default()
    };
    if !features.has(id::DEVICE_FW_INFO) {
        return info;
    }
    let version = features
        .entries()
        .iter()
        .find(|e| e.id == id::DEVICE_FW_INFO)
        .map_or(0, |e| e.version);
    let mut dev = Device::new(t, slot, features);
    // getDeviceInfo -> [entities, unit id x4, transports x2, model ids x6,
    //                   extended model, capabilities]
    let Ok(p) = dev.call(id::DEVICE_FW_INFO, 0, &[]) else {
        return info;
    };
    let entities = p.first().copied().unwrap_or(0);
    if p.len() >= 15 {
        info.unit_id = hex(&p[1..5]);
        // Byte 6 says which ways of connecting exist; the model ids follow
        // in the same order (Bluetooth, BLE, eQuad receiver, USB).
        let mut ids = p[7..13].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]]));
        for (bit, name) in [(0, "Bluetooth"), (1, "BLE"), (2, "eQuad"), (3, "USB")] {
            if p[6] & (1 << bit) != 0 {
                info.transports.push(name.into());
                if let Some(m) = ids.next().filter(|&m| m != 0) {
                    info.model_ids.push(format!("{name} {m:04X}"));
                }
            }
        }
        // The serial number is available from version 4 when flagged.
        if version >= 4 && p.len() >= 15 && p[14] & 0x01 != 0 {
            if let Ok(s) = dev.call(id::DEVICE_FW_INFO, 2, &[]) {
                let end = s.iter().position(|&b| b == 0).unwrap_or(s.len()).min(12);
                let text = String::from_utf8_lossy(&s[..end]).trim().to_owned();
                if !text.is_empty() {
                    info.serial = Some(text);
                }
            }
        }
    }
    for e in 0..entities {
        let Ok(p) = dev.call(id::DEVICE_FW_INFO, 1, &[e]) else {
            continue;
        };
        // [type, prefix x3, number, revision, build x2, active, pid x2, ...]
        if p.len() < 8 {
            continue;
        }
        let prefix = String::from_utf8_lossy(&p[1..4]).trim().to_owned();
        let version = if prefix.is_empty() && p[4..8].iter().all(|&b| b == 0) {
            String::new()
        } else {
            format!(
                "{prefix} {:02X}.{:02X}.B{:02X}{:02X}",
                p[4], p[5], p[6], p[7]
            )
            .trim()
            .to_owned()
        };
        let pid = (p.len() >= 11)
            .then(|| u16::from_be_bytes([p[9], p[10]]))
            .filter(|&v| v != 0);
        info.firmware.push(FirmwareEntity {
            kind: entity_kind(p[0]),
            version,
            pid,
            active: p.get(8).is_some_and(|b| b & 1 != 0),
        });
    }
    info
}

/// The features that appear only once hidden features are enabled.
///
/// Switches EnableHiddenFeatures (`0x1E00`) on, reads the feature list
/// again, then restores the previous state. This changes nothing stored on
/// the device (the switch is forgotten at power off), but the features it
/// reveals are meant for Logitech's own tools: list them, do not call them.
pub fn list_hidden_features(
    t: &mut Transport,
    slot: u8,
    features: &FeatureTable,
) -> Result<Vec<FeatureEntry>, HidppError> {
    let mut dev = Device::new(t, slot, features);
    let was_on = dev
        .call(id::ENABLE_HIDDEN_FEATURES, 0, &[])?
        .first()
        .is_some_and(|b| *b != 0);
    if !was_on {
        dev.call(id::ENABLE_HIDDEN_FEATURES, 1, &[1])?;
    }
    let unlocked = FeatureTable::discover(t, slot);
    if !was_on {
        let mut dev = Device::new(t, slot, features);
        dev.call(id::ENABLE_HIDDEN_FEATURES, 1, &[0])?;
    }
    let unlocked = unlocked?;
    Ok(unlocked
        .entries()
        .into_iter()
        .filter(|e| !features.has(e.id))
        .collect())
}

/// Bytes as uppercase hexadecimal without separators.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_entities_and_flags() {
        assert_eq!(entity_kind(0), "application");
        assert_eq!(entity_kind(9), "type 9");
        let e = FeatureEntry {
            id: 0x1E00,
            index: 9,
            version: 0,
            flags: 0x60,
        };
        assert!(e.is_hidden() && e.is_engineering() && !e.is_obsolete());
        assert_eq!(e.flag_names(), ["hidden", "engineering"]);
        assert_eq!(hex(&[0x0A, 0xFF]), "0AFF");
    }
}
