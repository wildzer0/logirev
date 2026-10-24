//! The configuration file (`logirev.toml`).
//!
//! The file is plain text in [TOML](https://toml.io) format and lives next to
//! the executable, so the whole program can be carried around on a USB stick.
//! It contains three kinds of sections:
//!
//! * `[general]` — which receiver to use (normally auto-detected);
//! * `[logging]` — log file, its size limit and the optional packet capture;
//! * `[[profile]]` — one block per kind of device: what it matches and how
//!   the device must be configured (speed, wheel, buttons).
//!
//! When a device connects, the **first** profile whose `match` rules fit is
//! applied. See `config/logirev.toml` for a fully commented example.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use log::LevelFilter;
use serde::Deserialize;

use crate::hidpp::features::{parse_control, DeviceKind};
use crate::input::keys::{parse_chord, Key, MouseButton};

/// The commented default configuration, written by `logirevctl init`.
pub const TEMPLATE: &str = include_str!("../config/logirev.toml");

/// Smallest and largest raw pointer speed accepted by the firmware.
const POINTER_SPEED_RAW_RANGE: (u16, u16) = (0x002E, 0x01FF);

// ---------------------------------------------------------------------------
// File layout (what serde reads)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    general: General,
    #[serde(default)]
    logging: Logging,
    #[serde(default, rename = "profile")]
    profiles: Vec<RawProfile>,
}

/// `[general]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct General {
    /// Product id of the receiver/device to use (e.g. `"c548"`); empty = auto.
    #[serde(default)]
    pub product_id: Option<String>,
}

/// `[logging]` section.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Logging {
    /// `error`, `warn`, `info`, `debug` or `trace` (trace = every frame).
    pub level: String,
    /// Log file; relative paths are relative to the configuration file.
    pub file: Option<PathBuf>,
    /// Maximum size of the log file in bytes (default 1 MiB).
    pub max_size: u64,
    /// Optional Wireshark capture of all HID++ traffic.
    pub pcap: Option<PathBuf>,
    /// Maximum size of the capture file in bytes (default 10 MiB).
    pub pcap_max_size: u64,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            level: "info".into(),
            file: Some(PathBuf::from("logirev.log")),
            max_size: 1024 * 1024,
            pcap: None,
            pcap_max_size: 10 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    name: String,
    #[serde(default, rename = "match")]
    matcher: Match,
    pointer_speed: Option<f32>,
    dpi: Option<u16>,
    hires_wheel: Option<bool>,
    invert_wheel: Option<bool>,
    #[serde(default)]
    buttons: BTreeMap<String, RawAction>,
}

/// `match` rules of a profile. Every rule that is present must fit; a
/// profile without rules matches every device.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    /// Text that must appear in the device name (case-insensitive).
    pub name: Option<String>,
    /// Device category: `mouse`, `trackball`, `keyboard`, ...
    pub kind: Option<DeviceKind>,
}

impl Match {
    pub fn fits(&self, name: &str, kind: DeviceKind) -> bool {
        let name_ok = self
            .name
            .as_ref()
            .map_or(true, |n| name.to_lowercase().contains(&n.to_lowercase()));
        let kind_ok = self.kind.map_or(true, |k| k == kind);
        name_ok && kind_ok
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
enum RawAction {
    Keys {
        keys: String,
    },
    Click {
        button: MouseButton,
    },
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    TogglePointerSpeed {
        speed: f32,
    },
    ToggleDpi {
        dpi: u16,
    },
    Disable,
    Default,
}

// ---------------------------------------------------------------------------
// Validated configuration (what the daemon uses)
// ---------------------------------------------------------------------------

/// A fully validated configuration.
#[derive(Debug)]
pub struct Config {
    /// Directory containing the configuration file.
    pub base_dir: PathBuf,
    pub general: General,
    pub logging: Logging,
    pub profiles: Vec<Profile>,
}

/// One validated `[[profile]]`.
#[derive(Debug)]
pub struct Profile {
    pub name: String,
    pub matcher: Match,
    /// Raw firmware units (256 = 1.0×).
    pub pointer_speed: Option<u16>,
    pub dpi: Option<u16>,
    pub hires_wheel: Option<bool>,
    pub invert_wheel: Option<bool>,
    /// Control id → what to do when that button is used.
    pub buttons: BTreeMap<u16, Action>,
}

/// What a remapped button does.
#[derive(Debug, Clone)]
pub enum Action {
    /// Hold these keys for as long as the button is held.
    Keys(Vec<&'static Key>),
    /// Press this mouse button for as long as the button is held.
    Click(MouseButton),
    /// Start a program when the button is pressed.
    Command { program: String, args: Vec<String> },
    /// "Slow mode": switch between the normal value and this one.
    Toggle(SlowMode),
    /// Swallow the button: it does nothing at all.
    Disable,
}

/// The setting a "slow mode" button lowers while it is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlowMode {
    /// Pointer speed in raw firmware units (feature 0x2205).
    PointerSpeed(u16),
    /// Sensor resolution (feature 0x2201), e.g. on the MX Ergo S.
    Dpi(u16),
}

impl Config {
    /// Reads and validates the configuration file. Error messages are meant
    /// for end users and point at the offending profile or button.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Self::parse(&text, path.parent().unwrap_or(Path::new(".")))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Validates configuration text; `base_dir` resolves relative paths.
    pub fn parse(text: &str, base_dir: &Path) -> Result<Self, String> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| e.to_string())?;
        level_filter(&raw.logging.level)?;
        let profiles = raw
            .profiles
            .into_iter()
            .map(Profile::validate)
            .collect::<Result<_, _>>()?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
            general: raw.general,
            logging: raw.logging,
            profiles,
        })
    }

    /// First profile that fits a device, if any.
    pub fn profile_for(&self, name: &str, kind: DeviceKind) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.matcher.fits(name, kind))
    }

    /// Resolves a path from the file relative to the configuration directory.
    pub fn resolve(&self, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base_dir.join(p)
        }
    }

    /// The requested product id, parsed from hex.
    pub fn product_id(&self) -> Result<Option<u16>, String> {
        match self.general.product_id.as_deref().map(str::trim) {
            None | Some("") | Some("auto") => Ok(None),
            Some(s) => {
                let hex = s.rsplit(':').next().unwrap_or(s).trim_start_matches("0x");
                u16::from_str_radix(hex, 16)
                    .map(Some)
                    .map_err(|_| format!("invalid product_id \"{s}\""))
            }
        }
    }
}

impl Profile {
    fn validate(raw: RawProfile) -> Result<Self, String> {
        let ctx = |msg: String| format!("profile \"{}\": {msg}", raw.name);
        let mut buttons = BTreeMap::new();
        for (key, action) in raw.buttons {
            let cid =
                parse_control(&key).ok_or_else(|| ctx(format!("unknown button \"{key}\"")))?;
            let action = match action {
                RawAction::Keys { keys } => Action::Keys(
                    parse_chord(&keys).map_err(|e| ctx(format!("button \"{key}\": {e}")))?,
                ),
                RawAction::Click { button } => Action::Click(button),
                RawAction::Command { command, args } => Action::Command {
                    program: command,
                    args,
                },
                RawAction::TogglePointerSpeed { speed } => Action::Toggle(SlowMode::PointerSpeed(
                    speed_to_raw(speed).map_err(|e| ctx(format!("button \"{key}\": {e}")))?,
                )),
                RawAction::ToggleDpi { dpi } => Action::Toggle(SlowMode::Dpi(dpi)),
                RawAction::Disable => Action::Disable,
                RawAction::Default => continue, // leave the button to the firmware
            };
            buttons.insert(cid, action);
        }
        Ok(Self {
            pointer_speed: raw
                .pointer_speed
                .map(speed_to_raw)
                .transpose()
                .map_err(ctx)?,
            name: raw.name,
            matcher: raw.matcher,
            dpi: raw.dpi,
            hires_wheel: raw.hires_wheel,
            invert_wheel: raw.invert_wheel,
            buttons,
        })
    }
}

/// Converts a user-facing speed multiplier (1.0 = normal) to firmware units.
fn speed_to_raw(multiplier: f32) -> Result<u16, String> {
    let (lo, hi) = POINTER_SPEED_RAW_RANGE;
    let raw = (multiplier * 256.0).round();
    if !(f32::from(lo)..=f32::from(hi)).contains(&raw) {
        return Err(format!(
            "pointer speed {multiplier} out of range ({:.2} .. {:.2})",
            f32::from(lo) / 256.0,
            f32::from(hi) / 256.0
        ));
    }
    Ok(raw as u16)
}

/// Parses the `level` setting.
pub fn level_filter(level: &str) -> Result<LevelFilter, String> {
    level.parse().map_err(|_| {
        format!("invalid log level \"{level}\" (use error, warn, info, debug or trace)")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_valid() {
        let cfg = Config::parse(TEMPLATE, Path::new(".")).expect("template must parse");
        assert!(!cfg.profiles.is_empty());
    }

    #[test]
    fn first_matching_profile_wins() {
        let cfg = Config::parse(
            r#"
            [[profile]]
            name = "ergo"
            match = { name = "ergo", kind = "trackball" }
            buttons.back = { action = "keys", keys = "Alt+Left" }
            buttons.middle = { action = "default" }

            [[profile]]
            name = "any"
            "#,
            Path::new("."),
        )
        .unwrap();
        let p = cfg.profile_for("MX Ergo S", DeviceKind::Trackball).unwrap();
        assert_eq!(p.name, "ergo");
        assert_eq!(p.buttons.len(), 1, "\"default\" buttons are not diverted");
        assert_eq!(
            cfg.profile_for("MX Master 3S", DeviceKind::Mouse)
                .unwrap()
                .name,
            "any"
        );
    }

    #[test]
    fn reports_bad_input_clearly() {
        let err = Config::parse(
            "[[profile]]\nname = \"x\"\nbuttons.back = { action = \"keys\", keys = \"Ctrl+Nope\" }",
            Path::new("."),
        )
        .unwrap_err();
        assert!(
            err.contains("profile \"x\"") && err.contains("Nope"),
            "{err}"
        );
        assert!(Config::parse(
            "[[profile]]\nname = \"x\"\npointer_speed = 9.0",
            Path::new(".")
        )
        .is_err());
    }

    #[test]
    fn converts_speed() {
        assert_eq!(speed_to_raw(1.0), Ok(256));
        assert_eq!(speed_to_raw(0.5), Ok(128));
    }
}
