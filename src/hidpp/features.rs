//! HID++ 2.0 features used by logirev.
//!
//! A HID++ 2.0 device advertises a list of *features*, each identified by a
//! stable 16-bit id (e.g. `0x1B04` = reprogrammable buttons). At runtime a
//! feature is addressed by its *index* in the device's own table, which
//! differs between models and firmware versions. [`FeatureTable::discover`]
//! reads that table once per connection; every other call goes through it.
//!
//! Function numbers and payload layouts come from the public HID++ 2.0
//! documentation, cross-checked against the traffic of Logi Options+.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use super::frame::HidppError;
use super::transport::Transport;

/// Feature ids (only the ones logirev uses or reports).
pub mod id {
    pub const ROOT: u16 = 0x0000;
    pub const FEATURE_SET: u16 = 0x0001;
    pub const DEVICE_FW_INFO: u16 = 0x0003;
    pub const DEVICE_NAME_TYPE: u16 = 0x0005;
    pub const BATTERY_STATUS: u16 = 0x1000;
    pub const UNIFIED_BATTERY: u16 = 0x1004;
    pub const REPROG_CONTROLS_V4: u16 = 0x1B04;
    pub const WIRELESS_DEVICE_STATUS: u16 = 0x1D4B;
    pub const ADJUSTABLE_DPI: u16 = 0x2201;
    pub const POINTER_SPEED: u16 = 0x2205;
    pub const HIRES_WHEEL: u16 = 0x2121;
}

/// Human-readable name of a feature id, for `scan` output and logs.
pub fn feature_name(feature: u16) -> &'static str {
    match feature {
        0x0000 => "Root",
        0x0001 => "FeatureSet",
        0x0002 => "FeatureInfo",
        0x0003 => "DeviceFwInfo",
        0x0005 => "DeviceNameType",
        0x0007 => "DeviceFriendlyName",
        0x0011 => "PropertyAccess",
        0x0020 => "ConfigChange",
        0x0021 => "UniqueIdentifier",
        0x00C2 => "DfuControl",
        0x00C3 => "DfuControlSecure",
        0x00D0 => "Dfu",
        0x1000 => "BatteryStatus",
        0x1004 => "UnifiedBattery",
        0x1602 => "PasswordAuthentication",
        0x1802 => "DeviceReset",
        0x1803 => "GpioAccess",
        0x1805 => "OobState",
        0x1806 => "ConfigDeviceProps",
        0x1807 => "ConfigurableProperties",
        0x1814 => "ChangeHost",
        0x1815 => "HostsInfo",
        0x1816 => "BleProPrepairing",
        0x1830 => "PowerModes",
        0x1B04 => "ReprogControlsV4",
        0x1D4B => "WirelessDeviceStatus",
        0x1DF3 => "EquadDjDebug",
        0x1E00 => "EnableHiddenFeatures",
        0x1E02 => "ManageDeactivatableFeatures",
        0x2110 => "SmartShift",
        0x2111 => "SmartShiftEnhanced",
        0x2121 => "HiResWheel",
        0x2130 => "LowResWheel",
        0x2150 => "ThumbWheel",
        0x2201 => "AdjustableDpi",
        0x2202 => "ExtendedAdjustableDpi",
        0x2205 => "PointerSpeed",
        0x2250 => "AnalysisMode",
        0x8060 => "ReportRate",
        0x8061 => "ExtendedReportRate",
        0x8100 => "OnboardProfiles",
        _ => "?",
    }
}

/// Feature id → feature index for one connected device.
#[derive(Debug, Clone, Default)]
pub struct FeatureTable {
    map: BTreeMap<u16, u8>,
}

impl FeatureTable {
    /// Reads the complete feature list through the FeatureSet feature.
    pub fn discover(t: &mut Transport, index: u8) -> Result<Self, HidppError> {
        let mut map = BTreeMap::from([(id::ROOT, 0u8)]);
        // Root.getFeature(0x0001) -> [index, type, version]
        let fs = t.call(index, 0x00, 0, &id::FEATURE_SET.to_be_bytes())?;
        let fs_index = *fs.first().ok_or(HidppError::MalformedResponse)?;
        if fs_index == 0 {
            return Err(HidppError::FeatureNotSupported(id::FEATURE_SET));
        }
        // FeatureSet.getCount() -> [count] (Root is not counted).
        let count = *t
            .call(index, fs_index, 0, &[])?
            .first()
            .ok_or(HidppError::MalformedResponse)?;
        for i in 1..=count {
            // FeatureSet.getFeatureId(i) -> [id_hi, id_lo, type, version]
            let p = t.call(index, fs_index, 1, &[i])?;
            if p.len() >= 2 {
                map.insert(u16::from_be_bytes([p[0], p[1]]), i);
            }
        }
        Ok(Self { map })
    }

    pub fn index_of(&self, feature: u16) -> Option<u8> {
        self.map.get(&feature).copied()
    }

    pub fn has(&self, feature: u16) -> bool {
        self.map.contains_key(&feature)
    }

    /// `(feature id, feature index)` pairs in id order.
    pub fn iter(&self) -> impl Iterator<Item = (u16, u8)> + '_ {
        self.map.iter().map(|(k, v)| (*k, *v))
    }
}

/// Device category reported by `DeviceNameType.getDeviceType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Keyboard,
    Remote,
    Numpad,
    Mouse,
    Trackpad,
    Trackball,
    Presenter,
    Receiver,
    Headset,
    Webcam,
    Other,
}

impl DeviceKind {
    fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Keyboard,
            1 => Self::Remote,
            2 => Self::Numpad,
            3 => Self::Mouse,
            4 => Self::Trackpad,
            5 => Self::Trackball,
            6 => Self::Presenter,
            7 => Self::Receiver,
            8 => Self::Headset,
            9 => Self::Webcam,
            _ => Self::Other,
        }
    }
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{self:?}").to_lowercase())
    }
}

/// Short-lived handle used to talk to one device: the transport, the device
/// index and its feature table.
pub struct Device<'a> {
    t: &'a mut Transport,
    index: u8,
    features: &'a FeatureTable,
}

impl<'a> Device<'a> {
    pub fn new(t: &'a mut Transport, index: u8, features: &'a FeatureTable) -> Self {
        Self { t, index, features }
    }

    /// Calls `function` of `feature` with `params`.
    pub fn call(
        &mut self,
        feature: u16,
        function: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, HidppError> {
        let fi = self
            .features
            .index_of(feature)
            .ok_or(HidppError::FeatureNotSupported(feature))?;
        self.t.call(self.index, fi, function, params)
    }

    fn call_min(
        &mut self,
        feature: u16,
        function: u8,
        params: &[u8],
        min_len: usize,
    ) -> Result<Vec<u8>, HidppError> {
        let p = self.call(feature, function, params)?;
        if p.len() < min_len {
            return Err(HidppError::MalformedResponse);
        }
        Ok(p)
    }

    // ---------------------------------------------------------------- 0x0005

    /// Marketing name, e.g. "MX Ergo S".
    pub fn name(&mut self) -> Result<String, HidppError> {
        let len = usize::from(self.call_min(id::DEVICE_NAME_TYPE, 0, &[], 1)?[0]);
        let mut name = Vec::with_capacity(len);
        while name.len() < len {
            let chunk = self.call(id::DEVICE_NAME_TYPE, 1, &[name.len() as u8])?;
            if chunk.is_empty() {
                break;
            }
            name.extend_from_slice(&chunk);
        }
        name.truncate(len);
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        Ok(String::from_utf8_lossy(&name[..end]).trim().to_owned())
    }

    pub fn kind(&mut self) -> Result<DeviceKind, HidppError> {
        Ok(DeviceKind::from_code(
            self.call_min(id::DEVICE_NAME_TYPE, 2, &[], 1)?[0],
        ))
    }

    // ---------------------------------------------------------------- 0x0003

    /// Main firmware version, e.g. "RBM 17.01.B0011".
    pub fn firmware(&mut self) -> Result<String, HidppError> {
        let entities = self.call_min(id::DEVICE_FW_INFO, 0, &[], 1)?[0];
        for e in 0..entities {
            let p = self.call(id::DEVICE_FW_INFO, 1, &[e])?;
            // [type, prefix x3, number(BCD), revision(BCD), build_hi, build_lo, ...]
            if p.len() >= 8 && p[0] == 0 {
                let prefix = String::from_utf8_lossy(&p[1..4]).into_owned();
                return Ok(format!(
                    "{prefix} {:02X}.{:02X}.B{:02X}{:02X}",
                    p[4], p[5], p[6], p[7]
                ));
            }
        }
        Ok(String::new())
    }

    // ------------------------------------------------------- 0x1004 / 0x1000

    /// Battery level as text, e.g. "80% (charging)".
    pub fn battery(&mut self) -> Result<String, HidppError> {
        if self.features.has(id::UNIFIED_BATTERY) {
            // getStatus -> [percent, level flags, charging status, external power]
            let p = self.call_min(id::UNIFIED_BATTERY, 1, &[], 3)?;
            let state = match p[2] {
                0 => "",
                1 | 2 => " (charging)",
                3 => " (full)",
                _ => " (charge error)",
            };
            return Ok(format!("{}%{state}", p[0]));
        }
        // getBatteryLevelStatus -> [percent, next level, status]
        let p = self.call_min(id::BATTERY_STATUS, 0, &[], 3)?;
        let state = if (1..=4).contains(&p[2]) {
            " (charging)"
        } else {
            ""
        };
        Ok(format!("{}%{state}", p[0]))
    }

    // ---------------------------------------------------------------- 0x2205

    /// Pointer speed in raw units, where `256` = 1.0×.
    pub fn pointer_speed(&mut self) -> Result<u16, HidppError> {
        let p = self.call_min(id::POINTER_SPEED, 0, &[], 2)?;
        Ok(u16::from_be_bytes([p[0], p[1]]))
    }

    pub fn set_pointer_speed(&mut self, raw: u16) -> Result<(), HidppError> {
        self.call(id::POINTER_SPEED, 1, &raw.to_be_bytes())
            .map(drop)
    }

    // ---------------------------------------------------------------- 0x2201

    /// Current DPI of sensor 0.
    pub fn dpi(&mut self) -> Result<u16, HidppError> {
        let p = self.call_min(id::ADJUSTABLE_DPI, 2, &[0], 3)?;
        Ok(u16::from_be_bytes([p[1], p[2]]))
    }

    pub fn set_dpi(&mut self, dpi: u16) -> Result<(), HidppError> {
        let [hi, lo] = dpi.to_be_bytes();
        self.call(id::ADJUSTABLE_DPI, 3, &[0, hi, lo]).map(drop)
    }

    /// DPI values supported by sensor 0.
    pub fn dpi_choices(&mut self) -> Result<DpiChoices, HidppError> {
        // getSensorDpiList(sensor) -> [sensor, value x2, value x2, ..., 0 0]
        let p = self.call_min(id::ADJUSTABLE_DPI, 1, &[0], 3)?;
        Ok(DpiChoices::parse(&p[1..]))
    }

    // ---------------------------------------------------------------- 0x2121

    /// Wheel mode flags: bit 0 = divert to HID++ (never set by logirev),
    /// bit 1 = high resolution, bit 2 = inverted direction.
    pub fn wheel_mode(&mut self) -> Result<u8, HidppError> {
        Ok(self.call_min(id::HIRES_WHEEL, 1, &[], 1)?[0])
    }

    pub fn set_wheel_mode(&mut self, flags: u8) -> Result<(), HidppError> {
        self.call(id::HIRES_WHEEL, 2, &[flags]).map(drop)
    }

    // ---------------------------------------------------------------- 0x1B04

    /// Every physical/virtual button the firmware lets software inspect.
    pub fn controls(&mut self) -> Result<Vec<Control>, HidppError> {
        let count = self.call_min(id::REPROG_CONTROLS_V4, 0, &[], 1)?[0];
        let mut out = Vec::with_capacity(usize::from(count));
        for i in 0..count {
            // getCidInfo(i) -> [cid x2, task id x2, flags, position, group, group mask, flags2]
            let p = self.call_min(id::REPROG_CONTROLS_V4, 1, &[i], 5)?;
            out.push(Control {
                cid: u16::from_be_bytes([p[0], p[1]]),
                task: u16::from_be_bytes([p[2], p[3]]),
                flags: p[4],
            });
        }
        Ok(out)
    }

    /// Diverts a button to software (`true`) or gives it back to the firmware.
    ///
    /// While diverted, the button no longer produces normal mouse/keyboard
    /// input: the device sends a HID++ "diverted buttons" notification
    /// instead, which the daemon turns into the configured action. The
    /// setting is *volatile*: the device forgets it when it powers down or
    /// reconnects, which is why the daemon re-applies it on every connection.
    pub fn set_diverted(&mut self, cid: u16, diverted: bool) -> Result<(), HidppError> {
        // setCidReporting(cid, flags, remap): each flag is followed by a
        // "valid" bit telling the firmware to apply it. 0x02 = divert valid.
        let flags = if diverted { 0x03 } else { 0x02 };
        let [hi, lo] = cid.to_be_bytes();
        self.call(id::REPROG_CONTROLS_V4, 3, &[hi, lo, flags, 0, 0])
            .map(drop)
    }
}

/// Resolutions a sensor accepts: either a list or a stepped range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiChoices {
    Values(Vec<u16>),
    Range { min: u16, max: u16, step: u16 },
}

impl DpiChoices {
    /// Decodes the big-endian list returned by `getSensorDpiList`. A value
    /// of the form `0xE000 | step` between two values means "every `step`
    /// DPI from the previous value to the next one".
    fn parse(bytes: &[u8]) -> Self {
        let values: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .take_while(|&v| v != 0)
            .collect();
        match values.as_slice() {
            [min, step, max, ..] if step >> 13 == 0b111 => Self::Range {
                min: *min,
                max: *max,
                step: step & 0x1FFF,
            },
            _ => Self::Values(values),
        }
    }

    pub fn contains(&self, dpi: u16) -> bool {
        match self {
            Self::Values(v) => v.contains(&dpi),
            Self::Range { min, max, step } => {
                (*min..=*max).contains(&dpi) && (*step == 0 || (dpi - min) % step == 0)
            }
        }
    }
}

impl fmt::Display for DpiChoices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Range { min, max, step } => write!(f, "{min}..{max} in steps of {step}"),
            Self::Values(v) => {
                let v: Vec<String> = v.iter().map(u16::to_string).collect();
                f.write_str(&v.join(", "))
            }
        }
    }
}

/// One reprogrammable control, as listed by `ReprogControlsV4`.
#[derive(Debug, Clone, Copy)]
pub struct Control {
    /// Control id: identifies the button in every HID++ message.
    pub cid: u16,
    /// Task id: the default function the firmware assigns to it.
    pub task: u16,
    /// Capability flags (see the `is_*` helpers).
    pub flags: u8,
}

impl Control {
    pub fn is_mouse_button(&self) -> bool {
        self.flags & 0x01 != 0
    }
    /// Only divertable controls can be remapped by logirev.
    pub fn is_divertable(&self) -> bool {
        self.flags & 0x20 != 0
    }
    pub fn is_virtual(&self) -> bool {
        self.flags & 0x80 != 0
    }
}

/// Friendly names and configuration aliases of well-known control ids.
///
/// The alias is what users write in the configuration file (`back = ...`);
/// any control can also be referenced by its hexadecimal id (`"0x53" = ...`).
pub const CONTROL_NAMES: &[(u16, &str, &str)] = &[
    (0x0050, "left", "Left click"),
    (0x0051, "right", "Right click"),
    (0x0052, "middle", "Middle button (wheel click)"),
    (0x0053, "back", "Back"),
    (0x0056, "forward", "Forward"),
    (0x005B, "tilt_left", "Wheel tilt left"),
    (0x005D, "tilt_right", "Wheel tilt right"),
    (0x00C3, "gesture", "Gesture button"),
    (0x00C4, "smartshift", "SmartShift / wheel mode"),
    (0x00D7, "virtual_gesture", "Virtual gesture button"),
    (0x00FD, "precision", "Precision mode / DPI switch button"),
    // Second alias for the same button, as Logitech calls it on mice.
    (0x00FD, "dpi_switch", "Precision mode / DPI switch button"),
];

/// `(alias, description)` for a control id, if well-known.
pub fn control_name(cid: u16) -> Option<(&'static str, &'static str)> {
    CONTROL_NAMES
        .iter()
        .find(|(c, _, _)| *c == cid)
        .map(|(_, a, d)| (*a, *d))
}

/// Resolves a configuration key (`"back"`, `"0x53"`, `"83"`) to a control id.
pub fn parse_control(key: &str) -> Option<u16> {
    let key = key.trim();
    if let Some((cid, _, _)) = CONTROL_NAMES
        .iter()
        .find(|(_, a, _)| a.eq_ignore_ascii_case(key))
    {
        return Some(*cid);
    }
    match key.strip_prefix("0x").or_else(|| key.strip_prefix("0X")) {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => key.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dpi_choices() {
        let range = DpiChoices::parse(&[0x01, 0x90, 0xE0, 0x32, 0x0F, 0xA0, 0, 0]);
        assert_eq!(
            range,
            DpiChoices::Range {
                min: 400,
                max: 4000,
                step: 50
            }
        );
        assert!(range.contains(750) && !range.contains(775));
        let list = DpiChoices::parse(&[0x01, 0x90, 0x03, 0x20, 0, 0, 0x05]);
        assert_eq!(list, DpiChoices::Values(vec![400, 800]));
    }

    #[test]
    fn parses_control_keys() {
        assert_eq!(parse_control("back"), Some(0x53));
        assert_eq!(parse_control("Tilt_Left"), Some(0x5B));
        assert_eq!(parse_control("precision"), Some(0xFD));
        assert_eq!(parse_control("0x00FD"), Some(0xFD));
        assert_eq!(parse_control("86"), Some(0x56));
        assert_eq!(parse_control("nope"), None);
    }
}
