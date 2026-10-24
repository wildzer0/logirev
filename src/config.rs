//! The configuration file (`logirev.toml`).
//!
//! The file is plain text in [TOML](https://toml.io) format and lives next to
//! the executable, so the whole program can be carried around on a USB stick.
//! It contains three kinds of sections:
//!
//! * `[general]` — which receiver to use, battery warning level;
//! * `[logging]` — log file, its size limit and the optional packet capture;
//! * `[[profile]]` — one block per kind of device: what it matches and how
//!   the device must be configured (speed, wheel, buttons...), optionally
//!   with `[[profile.app]]` blocks that change buttons in given programs.
//!
//! When a device connects, the **first** profile whose `match` rules fit is
//! applied. See `config/logirev.toml` for a fully commented example and
//! `docs/CONFIGURATION.md` for the reference.
//!
//! The file is read in two stages: serde parses the text into the `Raw*`
//! structures (mirroring the file), then `validate` turns them into the
//! types the daemon uses, with readable error messages.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use log::LevelFilter;
use serde::Deserialize;

use crate::hidpp::features::{parse_control, DeviceKind, SmartShift, DISABLE_KEY_NAMES};
use crate::input::keys::{parse_chord, Key, MouseButton};

/// The commented default configuration, written by `logirevctl init`.
pub const TEMPLATE: &str = include_str!("../config/logirev.toml");

/// Smallest and largest raw pointer speed accepted by the firmware.
const POINTER_SPEED_RAW_RANGE: (u16, u16) = (0x002E, 0x01FF);
/// Longest single pause in a `sequence`, and longest total pause, in ms.
/// Sequences run on the service thread, so they must stay short.
const MAX_STEP_DELAY_MS: u32 = 2000;
const MAX_SEQUENCE_DELAY_MS: u32 = 5000;
/// Default movement (in device counts) needed to recognise a gesture.
const DEFAULT_GESTURE_THRESHOLD: u16 = 50;

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
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    /// Product id of the receiver/device to use (e.g. `"c548"`); empty = auto.
    pub product_id: Option<String>,
    /// Warn when a battery drops to this percentage (0 = never).
    pub battery_warning: u8,
}

impl Default for General {
    fn default() -> Self {
        Self {
            product_id: None,
            battery_warning: 15,
        }
    }
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
    smartshift: Option<RawSmartShift>,
    thumb_wheel_invert: Option<bool>,
    report_rate: Option<u16>,
    function_keys: Option<FunctionKeys>,
    disable_keys: Option<Vec<String>>,
    #[serde(default)]
    buttons: BTreeMap<String, RawAction>,
    #[serde(default, rename = "app")]
    apps: Vec<RawApp>,
}

/// `smartshift = { mode = "smart", threshold = 12 }`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSmartShift {
    mode: SmartShiftMode,
    threshold: Option<u8>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SmartShiftMode {
    /// Ratchet that frees itself when scrolling fast.
    Smart,
    /// Always clicking.
    Ratchet,
    /// Always free-spinning.
    Free,
}

/// What F1–F12 do without holding Fn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FunctionKeys {
    /// F1, F2... (media functions need Fn).
    Standard,
    /// Media / special functions (F1–F12 need Fn).
    Special,
}

/// `[[profile.app]]`: button changes inside some programs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApp {
    #[serde(rename = "match")]
    programs: Vec<String>,
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
    CycleDpi {
        dpi: Vec<u16>,
    },
    Scroll {
        direction: ScrollDirection,
        #[serde(default = "one")]
        amount: u8,
    },
    ChangeHost {
        host: RawHost,
    },
    Sequence {
        steps: Vec<RawStep>,
    },
    Gesture {
        up: Option<Box<RawAction>>,
        down: Option<Box<RawAction>>,
        left: Option<Box<RawAction>>,
        right: Option<Box<RawAction>>,
        click: Option<Box<RawAction>>,
        threshold: Option<u16>,
    },
    Disable,
    Default,
}

fn one() -> u8 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// `host = 2` or `host = "next"`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawHost {
    Number(u8),
    Name(String),
}

/// One step of a `sequence`: exactly one of the fields.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    keys: Option<String>,
    text: Option<String>,
    click: Option<MouseButton>,
    delay: Option<u32>,
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
    pub smartshift: Option<SmartShift>,
    pub thumb_wheel_invert: Option<bool>,
    /// Report rate in Hz.
    pub report_rate: Option<u16>,
    /// `true` = F1–F12 do their special function without Fn.
    pub fn_keys_special: Option<bool>,
    /// Bit mask of keys to disable (feature 0x4521).
    pub disable_keys: Option<u8>,
    /// Control id → what to do when that button is used.
    pub buttons: BTreeMap<u16, Action>,
    /// Per-program changes, first match wins.
    pub apps: Vec<AppOverride>,
}

/// `[[profile.app]]`, validated.
#[derive(Debug)]
pub struct AppOverride {
    /// Program file names, lower case, without `.exe`.
    pub programs: Vec<String>,
    /// `None` = the button behaves normally in these programs.
    pub buttons: BTreeMap<u16, Option<Action>>,
}

/// What a remapped button does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Hold these keys for as long as the button is held.
    Keys(Vec<&'static Key>),
    /// Press this mouse button for as long as the button is held.
    Click(MouseButton),
    /// Start a program when the button is pressed.
    Command { program: String, args: Vec<String> },
    /// "Slow mode": switch between the normal value and this one.
    Toggle(SlowMode),
    /// Step through these DPI values, one per press.
    CycleDpi(Vec<u16>),
    /// Scroll by whole wheel notches (positive = up / right).
    Scroll { vertical: i32, horizontal: i32 },
    /// Easy-Switch to another computer.
    ChangeHost(HostTarget),
    /// Several steps in a row (simple macro).
    Sequence(Vec<Step>),
    /// Hold the button and move the pointer: one action per direction.
    Gesture(Box<Gesture>),
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

/// Target of a `change-host` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTarget {
    Next,
    /// Host counted from 0.
    Index(u8),
}

/// One step of a sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Keys(Vec<&'static Key>),
    Text(String),
    Click(MouseButton),
    Delay(u32),
}

/// A gesture button: what happens for each movement direction, and for a
/// press without (enough) movement.
#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    pub up: Option<Action>,
    pub down: Option<Action>,
    pub left: Option<Action>,
    pub right: Option<Action>,
    pub click: Option<Action>,
    /// Minimum movement, in device counts.
    pub threshold: u16,
}

impl Gesture {
    /// The action for a total movement of `(dx, dy)` (screen coordinates:
    /// positive `dy` = down).
    pub fn pick(&self, dx: i32, dy: i32) -> Option<&Action> {
        let t = i32::from(self.threshold);
        if dx.abs() < t && dy.abs() < t {
            return self.click.as_ref();
        }
        if dx.abs() >= dy.abs() {
            if dx > 0 {
                self.right.as_ref()
            } else {
                self.left.as_ref()
            }
        } else if dy > 0 {
            self.down.as_ref()
        } else {
            self.up.as_ref()
        }
    }
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
        if raw.general.battery_warning > 100 {
            return Err("battery_warning must be between 0 and 100".into());
        }
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
    /// The buttons to remap while `program` is in the foreground: the
    /// profile's buttons, changed by the first `[[profile.app]]` that lists
    /// the program.
    pub fn buttons_for(&self, program: Option<&str>) -> BTreeMap<u16, Action> {
        let mut map = self.buttons.clone();
        let Some(program) = program.map(program_key) else {
            return map;
        };
        if let Some(app) = self.apps.iter().find(|a| a.programs.contains(&program)) {
            for (cid, action) in &app.buttons {
                match action {
                    Some(a) => map.insert(*cid, a.clone()),
                    None => map.remove(cid),
                };
            }
        }
        map
    }

    /// Whether some `[[profile.app]]` changes this profile for `program`.
    pub fn has_app_override(&self, program: &str) -> bool {
        let key = program_key(program);
        self.apps.iter().any(|a| a.programs.contains(&key))
    }

    fn validate(raw: RawProfile) -> Result<Self, String> {
        let name = raw.name.clone();
        let ctx = |msg: String| format!("profile \"{name}\": {msg}");

        let mut buttons = BTreeMap::new();
        for (key, action) in raw.buttons {
            let cid =
                parse_control(&key).ok_or_else(|| ctx(format!("unknown button \"{key}\"")))?;
            if let Some(a) =
                validate_action(action, false).map_err(|e| ctx(format!("button \"{key}\": {e}")))?
            {
                buttons.insert(cid, a);
            }
        }

        let mut apps = Vec::new();
        for app in raw.apps {
            if app.programs.is_empty() {
                return Err(ctx(
                    "an [[profile.app]] block needs a non-empty match list".into()
                ));
            }
            let mut map = BTreeMap::new();
            for (key, action) in app.buttons {
                let cid =
                    parse_control(&key).ok_or_else(|| ctx(format!("unknown button \"{key}\"")))?;
                let a = validate_action(action, false)
                    .map_err(|e| ctx(format!("app {:?}, button \"{key}\": {e}", app.programs)))?;
                map.insert(cid, a);
            }
            apps.push(AppOverride {
                programs: app.programs.iter().map(|p| program_key(p)).collect(),
                buttons: map,
            });
        }

        let smartshift = raw.smartshift.map(|s| match s.mode {
            SmartShiftMode::Free => SmartShift {
                free_spin: true,
                threshold: s.threshold.unwrap_or(10),
            },
            SmartShiftMode::Ratchet => SmartShift {
                free_spin: false,
                threshold: 255,
            },
            SmartShiftMode::Smart => SmartShift {
                free_spin: false,
                threshold: s.threshold.unwrap_or(10).clamp(1, 254),
            },
        });

        if let Some(hz) = raw.report_rate {
            if !(125..=1000).contains(&hz) {
                return Err(ctx(format!(
                    "report_rate {hz} must be between 125 and 1000 (Hz)"
                )));
            }
        }

        let disable_keys = match raw.disable_keys {
            None => None,
            Some(list) => {
                let mut mask = 0u8;
                for k in &list {
                    let bit = DISABLE_KEY_NAMES
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(k.trim()))
                        .map(|(_, b)| *b)
                        .ok_or_else(|| {
                            let names: Vec<&str> =
                                DISABLE_KEY_NAMES.iter().map(|(n, _)| *n).collect();
                            ctx(format!(
                                "disable_keys: unknown key \"{k}\" (use {})",
                                names.join(", ")
                            ))
                        })?;
                    mask |= bit;
                }
                Some(mask)
            }
        };

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
            smartshift,
            thumb_wheel_invert: raw.thumb_wheel_invert,
            report_rate: raw.report_rate,
            fn_keys_special: raw.function_keys.map(|f| f == FunctionKeys::Special),
            disable_keys,
            buttons,
            apps,
        })
    }
}

/// `"Chrome.EXE"` → `"chrome"`: how program names are compared.
pub fn program_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let file = lower.rsplit(['\\', '/']).next().unwrap_or(&lower);
    file.strip_suffix(".exe").unwrap_or(file).to_owned()
}

/// Turns one action from the file into its validated form. `Ok(None)` means
/// "default behaviour". `nested` is true inside a gesture.
fn validate_action(raw: RawAction, nested: bool) -> Result<Option<Action>, String> {
    Ok(Some(match raw {
        RawAction::Keys { keys } => Action::Keys(parse_chord(&keys)?),
        RawAction::Click { button } => Action::Click(button),
        RawAction::Command { command, args } => Action::Command {
            program: command,
            args,
        },
        RawAction::TogglePointerSpeed { speed } => {
            Action::Toggle(SlowMode::PointerSpeed(speed_to_raw(speed)?))
        }
        RawAction::ToggleDpi { dpi } => Action::Toggle(SlowMode::Dpi(dpi)),
        RawAction::CycleDpi { dpi } => {
            if dpi.is_empty() {
                return Err(
                    "cycle-dpi needs at least one value, e.g. dpi = [400, 800, 1600]".into(),
                );
            }
            Action::CycleDpi(dpi)
        }
        RawAction::Scroll { direction, amount } => {
            let n = i32::from(amount.max(1));
            let (vertical, horizontal) = match direction {
                ScrollDirection::Up => (n, 0),
                ScrollDirection::Down => (-n, 0),
                ScrollDirection::Left => (0, -n),
                ScrollDirection::Right => (0, n),
            };
            Action::Scroll {
                vertical,
                horizontal,
            }
        }
        RawAction::ChangeHost { host } => Action::ChangeHost(match host {
            RawHost::Number(n @ 1..=3) => HostTarget::Index(n - 1),
            RawHost::Name(s) if s.eq_ignore_ascii_case("next") => HostTarget::Next,
            RawHost::Number(n) => return Err(format!("host {n} must be 1, 2, 3 or \"next\"")),
            RawHost::Name(s) => return Err(format!("host \"{s}\" must be 1, 2, 3 or \"next\"")),
        }),
        RawAction::Sequence { steps } => Action::Sequence(validate_steps(steps)?),
        RawAction::Gesture {
            up,
            down,
            left,
            right,
            click,
            threshold,
        } => {
            if nested {
                return Err("a gesture cannot contain another gesture".into());
            }
            let sub = |a: Option<Box<RawAction>>| -> Result<Option<Action>, String> {
                match a {
                    Some(a) => validate_action(*a, true),
                    None => Ok(None),
                }
            };
            Action::Gesture(Box::new(Gesture {
                up: sub(up)?,
                down: sub(down)?,
                left: sub(left)?,
                right: sub(right)?,
                click: sub(click)?,
                threshold: threshold.unwrap_or(DEFAULT_GESTURE_THRESHOLD).max(1),
            }))
        }
        RawAction::Disable => Action::Disable,
        RawAction::Default => return Ok(None),
    }))
}

fn validate_steps(steps: Vec<RawStep>) -> Result<Vec<Step>, String> {
    if steps.is_empty() {
        return Err("a sequence needs at least one step".into());
    }
    let mut total = 0u32;
    let mut out = Vec::with_capacity(steps.len());
    for (i, s) in steps.into_iter().enumerate() {
        let n = i + 1;
        let step = match (s.keys, s.text, s.click, s.delay) {
            (Some(k), None, None, None) => Step::Keys(parse_chord(&k)?),
            (None, Some(t), None, None) => Step::Text(t),
            (None, None, Some(b), None) => Step::Click(b),
            (None, None, None, Some(ms)) => {
                if ms > MAX_STEP_DELAY_MS {
                    return Err(format!(
                        "step {n}: delay {ms} ms is longer than {MAX_STEP_DELAY_MS} ms"
                    ));
                }
                total += ms;
                Step::Delay(ms)
            }
            _ => {
                return Err(format!(
                    "step {n} must have exactly one of keys, text, click or delay"
                ))
            }
        };
        out.push(step);
    }
    if total > MAX_SEQUENCE_DELAY_MS {
        return Err(format!(
            "the delays add up to {total} ms, more than {MAX_SEQUENCE_DELAY_MS} ms"
        ));
    }
    Ok(out)
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

    fn parse(text: &str) -> Result<Config, String> {
        Config::parse(text, Path::new("."))
    }

    #[test]
    fn template_is_valid() {
        let cfg = parse(TEMPLATE).expect("template must parse");
        assert!(!cfg.profiles.is_empty());
    }

    #[test]
    fn template_examples_are_valid_when_enabled() {
        // Un-comment the example lines (`# [[profile.app]]`, `# buttons...`,
        // `# match...`, `# dpi...`, `# function_keys...`, `# disable_keys...`).
        let enabled: String = TEMPLATE
            .lines()
            .map(|l| {
                let body = l.strip_prefix("# ").unwrap_or(l);
                let example = [
                    "[[profile.app]]",
                    "match = [",
                    "buttons.",
                    "dpi = ",
                    "function_keys",
                    "disable_keys",
                ]
                .iter()
                .any(|p| body.starts_with(p));
                if example {
                    body
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let cfg = parse(&enabled).expect("examples must parse");
        let ergo = &cfg.profiles[0];
        assert_eq!(ergo.dpi, Some(750));
        assert!(ergo.has_app_override("explorer.exe"));
        assert_eq!(cfg.profiles[1].fn_keys_special, Some(false));
    }

    #[test]
    fn first_matching_profile_wins() {
        let cfg = parse(
            r#"
            [[profile]]
            name = "ergo"
            match = { name = "ergo", kind = "trackball" }
            buttons.back = { action = "keys", keys = "Alt+Left" }
            buttons.middle = { action = "default" }

            [[profile]]
            name = "any"
            "#,
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
        let err = parse(
            "[[profile]]\nname = \"x\"\nbuttons.back = { action = \"keys\", keys = \"Ctrl+Nope\" }",
        )
        .unwrap_err();
        assert!(
            err.contains("profile \"x\"") && err.contains("Nope"),
            "{err}"
        );
        assert!(parse("[[profile]]\nname = \"x\"\npointer_speed = 9.0").is_err());
        assert!(parse("[general]\nbattery_warning = 150").is_err());
        assert!(parse("[[profile]]\nname = \"x\"\nreport_rate = 5000").is_err());
        let err =
            parse("[[profile]]\nname = \"x\"\ndisable_keys = [\"capslock\", \"f13\"]").unwrap_err();
        assert!(err.contains("f13"), "{err}");
    }

    #[test]
    fn converts_speed() {
        assert_eq!(speed_to_raw(1.0), Ok(256));
        assert_eq!(speed_to_raw(0.5), Ok(128));
    }

    #[test]
    fn app_overrides_change_buttons() {
        let cfg = parse(
            r#"
            [[profile]]
            name = "p"
            buttons.back = { action = "keys", keys = "Alt+Left" }
            buttons.forward = { action = "keys", keys = "Alt+Right" }

            [[profile.app]]
            match = ["Photoshop.exe", "gimp"]
            buttons.back = { action = "keys", keys = "Ctrl+Z" }
            buttons.forward = { action = "default" }
            buttons.middle = { action = "click", button = "right" }
            "#,
        )
        .unwrap();
        let p = &cfg.profiles[0];
        assert_eq!(p.buttons_for(None).len(), 2);
        let ps = p.buttons_for(Some(r"C:\Program Files\Adobe\PHOTOSHOP.EXE"));
        assert_eq!(ps.len(), 2, "back changed, forward default, middle added");
        assert!(matches!(&ps[&0x53], Action::Keys(k) if k[1].name == "Z"));
        assert!(!ps.contains_key(&0x56));
        assert_eq!(ps[&0x52], Action::Click(MouseButton::Right));
        assert_eq!(p.buttons_for(Some("notepad.exe")), p.buttons_for(None));
        assert!(p.has_app_override("gimp.exe"));
    }

    #[test]
    fn parses_new_actions() {
        let cfg = parse(
            r#"
            [[profile]]
            name = "p"
            smartshift = { mode = "smart", threshold = 20 }
            function_keys = "standard"
            disable_keys = ["capslock", "Insert"]
            [profile.buttons]
            tilt_left = { action = "scroll", direction = "left", amount = 3 }
            tilt_right = { action = "change-host", host = "next" }
            middle = { action = "change-host", host = 2 }
            precision = { action = "cycle-dpi", dpi = [400, 800] }
            back = { action = "sequence", steps = [ { keys = "Ctrl+C" }, { delay = 50 }, { text = "hi" } ] }
            gesture = { action = "gesture", up = { action = "keys", keys = "Super+Tab" }, click = { action = "click", button = "middle" } }
            "#,
        )
        .unwrap();
        let p = &cfg.profiles[0];
        assert_eq!(
            p.smartshift,
            Some(SmartShift {
                free_spin: false,
                threshold: 20
            })
        );
        assert_eq!(p.fn_keys_special, Some(false));
        assert_eq!(p.disable_keys, Some(0x09));
        assert_eq!(
            p.buttons[&0x5B],
            Action::Scroll {
                vertical: 0,
                horizontal: -3
            }
        );
        assert_eq!(p.buttons[&0x5D], Action::ChangeHost(HostTarget::Next));
        assert_eq!(p.buttons[&0x52], Action::ChangeHost(HostTarget::Index(1)));
        assert_eq!(p.buttons[&0xFD], Action::CycleDpi(vec![400, 800]));
        assert!(matches!(&p.buttons[&0x53], Action::Sequence(s) if s.len() == 3));
        let Action::Gesture(g) = &p.buttons[&0xC3] else {
            panic!("gesture expected")
        };
        assert_eq!(g.threshold, DEFAULT_GESTURE_THRESHOLD);
        assert!(matches!(g.pick(0, -200), Some(Action::Keys(_))));
        assert_eq!(g.pick(5, 5), Some(&Action::Click(MouseButton::Middle)));
        assert_eq!(g.pick(300, 10), None, "no action for right");
    }

    #[test]
    fn documented_block_syntax_parses() {
        // The forms used in docs/CONFIGURATION.md.
        let cfg = parse(
            r#"
            [[profile]]
            name = "p"

            [profile.buttons.back]
            action = "sequence"
            steps = [
                { keys = "Ctrl+A" },
                { keys = "Ctrl+C" },
                { delay = 100 },
                { keys = "Alt+Tab" },
            ]

            [profile.buttons.gesture]
            action = "gesture"
            up    = { action = "keys", keys = "Super+Tab" }
            down  = { action = "keys", keys = "Super+D" }
            left  = { action = "keys", keys = "Ctrl+Super+Left" }
            right = { action = "keys", keys = "Ctrl+Super+Right" }
            click = { action = "click", button = "middle" }

            [[profile.app]]
            match = ["photoshop.exe"]
            buttons.back = { action = "keys", keys = "Ctrl+Z" }
            buttons.tilt_left = { action = "keys", keys = "LeftBracket" }
            "#,
        )
        .unwrap();
        let p = &cfg.profiles[0];
        assert!(matches!(&p.buttons[&0x53], Action::Sequence(s) if s.len() == 4));
        assert!(matches!(&p.buttons[&0xC3], Action::Gesture(_)));
        assert!(p.has_app_override("Photoshop.exe"));
    }

    #[test]
    fn rejects_bad_new_actions() {
        let bad =
            |b: &str| parse(&format!("[[profile]]\nname = \"p\"\nbuttons.back = {b}")).is_err();
        assert!(bad(r#"{ action = "change-host", host = 7 }"#));
        assert!(bad(r#"{ action = "cycle-dpi", dpi = [] }"#));
        assert!(bad(
            r#"{ action = "sequence", steps = [ { keys = "A", delay = 5 } ] }"#
        ));
        assert!(bad(
            r#"{ action = "sequence", steps = [ { delay = 3000 } ] }"#
        ));
        assert!(bad(
            r#"{ action = "gesture", up = { action = "gesture" } }"#
        ));
    }

    #[test]
    fn normalises_program_names() {
        assert_eq!(program_key("Chrome.EXE"), "chrome");
        assert_eq!(program_key(r"C:\Apps\Code.exe"), "code");
        assert_eq!(program_key("firefox"), "firefox");
    }
}
