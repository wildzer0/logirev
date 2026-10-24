//! What the service tells the user interface.
//!
//! The daemon runs on its own thread and never touches windows. Whenever its
//! state changes (a device connects, a receiver is unplugged, the settings
//! are reloaded...) it builds a [`Snapshot`] — a plain copy of everything
//! worth showing — and hands it to the interface through an [`Observer`]
//! callback. Messages for the user ("settings not applied: ...") travel the
//! same way as [`UiUpdate::Notice`].

/// Everything the interface shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub links: Vec<LinkStatus>,
}

/// One receiver (or one directly connected device).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkStatus {
    /// e.g. "Bolt receiver (046d:c548)".
    pub name: String,
    pub is_receiver: bool,
    /// Paired or connected devices, by slot.
    pub devices: Vec<DeviceStatus>,
}

/// One device, connected or only paired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStatus {
    /// Receiver slot 1–6, or `0xFF` for a direct connection.
    pub slot: u8,
    pub name: String,
    /// e.g. "trackball".
    pub kind: String,
    /// Connected and configured right now.
    pub online: bool,
    pub battery: Option<String>,
    pub dpi: Option<u16>,
    /// Name of the profile applied, if any.
    pub profile: Option<String>,
    /// Number of buttons remapped.
    pub remapped: usize,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiUpdate {
    /// The state changed.
    Snapshot(Snapshot),
    /// Something the user should read (shown as a notification).
    Notice { title: String, text: String },
    /// The service stopped; `Some` carries the error that stopped it.
    Stopped(Option<String>),
}

/// Callback receiving updates; called on the daemon thread.
pub type Observer = Box<dyn Fn(UiUpdate) + Send>;

/// How a graphical interface is attached to the daemon.
pub struct UiHooks {
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
            dpi: None,
            profile: None,
            remapped: 0,
        }
    }

    #[test]
    fn tooltip_lists_online_devices() {
        assert_eq!(Snapshot::default().tooltip(), "logirev — no receiver");
        let snap = Snapshot {
            links: vec![LinkStatus {
                name: "Bolt".into(),
                is_receiver: true,
                devices: vec![
                    device("Wave Keys", false, None),
                    device("MX Ergo S", true, Some("30%")),
                ],
            }],
        };
        assert_eq!(snap.tooltip(), "logirev — MX Ergo S 30%");
        let long = Snapshot {
            links: vec![LinkStatus {
                name: "x".into(),
                is_receiver: true,
                devices: (0..20)
                    .map(|i| device(&format!("Device {i}"), true, None))
                    .collect(),
            }],
        };
        assert!(long.tooltip().chars().count() <= 127);
    }
}
