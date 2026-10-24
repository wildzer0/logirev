//! What the service tells the user interface.
//!
//! The daemon runs on its own thread and never touches windows. Whenever its
//! state changes (a device connects, a receiver is unplugged, the settings
//! are reloaded...) it builds a [`Snapshot`] — a plain copy of everything
//! worth showing — and hands it to the interface through an [`Observer`]
//! callback. Messages for the user ("settings not applied: ...") travel the
//! same way as [`UiUpdate::Notice`].

use serde::{Deserialize, Serialize};

/// Everything the interface shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// One entry per open receiver or direct device.
    pub links: Vec<LinkStatus>,
}

/// One receiver (or one directly connected device).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkStatus {
    /// Identifies the receiver in commands sent back to the daemon.
    pub key: String,
    /// e.g. "Bolt receiver (046d:c548)".
    pub name: String,
    /// A receiver (as opposed to a directly connected device).
    pub is_receiver: bool,
    /// Paired or connected devices, by slot.
    pub devices: Vec<DeviceStatus>,
    /// The receiver can pair new devices (Bolt, Unifying).
    #[serde(default)]
    pub can_pair: bool,
    /// A pairing session in progress or just finished.
    #[serde(default)]
    pub pairing: Option<PairingStatus>,
    /// Recent HID++ messages (developer mode only, otherwise empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traffic: Vec<crate::hidpp::traffic::TrafficEntry>,
}

/// State of a pairing session, for the settings window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingStatus {
    /// Pairing protocol of the receiver.
    pub family: crate::hidpp::pairing::Family,
    /// Where the session stands.
    pub stage: crate::hidpp::pairing::Stage,
    /// Bolt: devices found so far (the user picks one).
    pub found: Vec<crate::hidpp::pairing::Found>,
}

/// One device, connected or only paired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStatus {
    /// Receiver slot 1–6, or `0xFF` for a direct connection.
    pub slot: u8,
    /// Marketing name, e.g. "MX Ergo S".
    pub name: String,
    /// e.g. "trackball".
    pub kind: String,
    /// Connected and configured right now.
    pub online: bool,
    /// Battery level, e.g. "80% (charging)".
    pub battery: Option<String>,
    /// Battery level in percent, and whether it is charging.
    #[serde(default)]
    pub battery_level: Option<(u8, bool)>,
    /// Current DPI, if adjustable.
    pub dpi: Option<u16>,
    /// Name of the profile applied, if any.
    pub profile: Option<String>,
    /// Number of buttons remapped.
    pub remapped: usize,
    /// Easy-Switch computers, when the device has several.
    pub hosts: Option<HostsStatus>,
    /// What the device can do (connected devices only), for the settings
    /// window.
    #[serde(default)]
    pub details: Option<DeviceDetails>,
}

/// Capabilities of a connected device: which settings make sense for it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDetails {
    /// Main firmware version, e.g. "RBM 26.01.B0013".
    pub firmware: String,
    /// Buttons the firmware lets software look at.
    pub controls: Vec<ControlInfo>,
    /// Accepted DPI values (empty = no adjustable DPI).
    pub dpi_values: Vec<u16>,
    /// Has an adjustable pointer speed.
    pub pointer_speed: bool,
    /// Has a configurable high-resolution wheel.
    pub hires_wheel: bool,
    /// Has SmartShift.
    pub smartshift: bool,
    /// Has a thumb wheel.
    pub thumb_wheel: bool,
    /// Accepted report rates in Hz (empty = not adjustable).
    pub report_rates: Vec<u16>,
    /// Can swap F-keys and media keys.
    pub fn_keys: bool,
    /// Keys that can be disabled (bit mask, see `DISABLE_KEY_NAMES`).
    pub disable_keys: u8,
    /// Number of manual backlight levels, if the keyboard is backlit.
    pub backlight_levels: Option<u8>,
    /// The backlight can follow the room light.
    pub backlight_auto: bool,
    /// Identity, firmware components and features (developer mode only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tech: Option<crate::hidpp::diagnostics::TechInfo>,
}

/// One button of a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlInfo {
    /// Control id (identifies the button in HID++ messages).
    pub cid: u16,
    /// How the button is written in the configuration (`back`, `"0x00C3"`).
    pub key: String,
    /// Human-readable name.
    pub label: String,
    /// Can be remapped.
    pub remappable: bool,
    /// Can follow mouse movement (gestures).
    pub gestures: bool,
}

/// Easy-Switch channels of a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostsStatus {
    /// Number of computers the device can switch between.
    pub count: u8,
    /// Channel in use, counted from 0.
    pub current: u8,
    /// Computer names per channel, when the device reports them.
    pub names: Vec<Option<String>>,
}

impl HostsStatus {
    /// "Computer 2 (DESKTOP-ABC)".
    pub fn label(&self, host: u8) -> String {
        match self.names.get(usize::from(host)).cloned().flatten() {
            Some(name) => format!("Computer {} ({name})", host + 1),
            None => format!("Computer {}", host + 1),
        }
    }
}

impl Snapshot {
    /// Short text for the tray icon tooltip (Windows limits it to 127
    /// characters), e.g. "logirev — MX Ergo S 30%".
    pub fn tooltip(&self) -> String {
        let online: Vec<String> = self
            .links
            .iter()
            .flat_map(|l| l.devices.iter())
            .filter(|d| d.online)
            .map(|d| match &d.battery {
                Some(b) => format!("{} {b}", d.name),
                None => d.name.clone(),
            })
            .collect();
        let text = if self.links.is_empty() {
            "logirev — no receiver".to_owned()
        } else if online.is_empty() {
            "logirev — no device connected".to_owned()
        } else {
            format!("logirev — {}", online.join(", "))
        };
        text.chars().take(127).collect()
    }
}

/// A message from the service to the interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiUpdate {
    /// The state changed.
    Snapshot(Snapshot),
    /// Something the user should read (shown as a notification).
    Notice {
        /// Short title.
        title: String,
        /// The message.
        text: String,
    },
    /// The service stopped; `Some` carries the error that stopped it.
    Stopped(Option<String>),
    /// A short message to show on screen for a moment (e.g. "800 DPI").
    Osd(String),
    /// Settings the tray process applies itself (at start and on reload).
    Preferences(Preferences),
}

/// Settings applied by the tray process rather than by the service.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    /// Show the lowest battery level on the tray icon.
    pub tray_battery: bool,
    /// Wheel speed in percent (100 = unchanged).
    pub scroll_percent: u16,
    /// Smooth scrolling.
    pub smooth_scrolling: bool,
}

impl Preferences {
    /// From the configuration.
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            tray_battery: cfg.general.tray_battery,
            scroll_percent: (cfg.general.scroll_speed * 100.0).round() as u16,
            smooth_scrolling: cfg.general.smooth_scrolling,
        }
    }
}

/// Callback receiving updates; called on the daemon thread.
pub type Observer = Box<dyn Fn(UiUpdate) + Send>;

/// How a graphical interface is attached to the daemon.
pub struct UiHooks {
    /// Receives every update (called on the daemon thread).
    pub observer: Observer,
    /// The interface reports device arrivals (`Event::Rescan`), so the
    /// daemon does not need to look for new receivers periodically.
    pub hotplug_events: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, online: bool, battery: Option<&str>) -> DeviceStatus {
        DeviceStatus {
            slot: 1,
            name: name.into(),
            kind: "mouse".into(),
            online,
            battery: battery.map(Into::into),
            battery_level: None,
            dpi: None,
            profile: None,
            remapped: 0,
            hosts: None,
            details: None,
        }
    }

    #[test]
    fn tooltip_lists_online_devices() {
        assert_eq!(Snapshot::default().tooltip(), "logirev — no receiver");
        let snap = Snapshot {
            links: vec![LinkStatus {
                traffic: Vec::new(),
                key: "k".into(),
                name: "Bolt".into(),
                is_receiver: true,
                can_pair: true,
                pairing: None,
                devices: vec![
                    device("Wave Keys", false, None),
                    device("MX Ergo S", true, Some("30%")),
                ],
            }],
        };
        assert_eq!(snap.tooltip(), "logirev — MX Ergo S 30%");
        let long = Snapshot {
            links: vec![LinkStatus {
                traffic: Vec::new(),
                key: "k".into(),
                name: "x".into(),
                is_receiver: true,
                can_pair: true,
                pairing: None,
                devices: (0..20)
                    .map(|i| device(&format!("Device {i}"), true, None))
                    .collect(),
            }],
        };
        assert!(long.tooltip().chars().count() <= 127);
    }

    #[test]
    fn labels_hosts() {
        let h = HostsStatus {
            count: 3,
            current: 0,
            names: vec![Some("PC".into()), None],
        };
        assert_eq!(h.label(0), "Computer 1 (PC)");
        assert_eq!(h.label(1), "Computer 2");
        assert_eq!(h.label(2), "Computer 3");
    }
}
