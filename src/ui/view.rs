//! What the tray menu and the device window contain, independent of the
//! operating system. The Win32 code only draws these lists, so the content
//! can be unit-tested anywhere.

use crate::hidpp::frame::RECEIVER_INDEX;
use crate::status::{DeviceStatus, Snapshot};

/// A command the user can pick from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Command {
    ShowDevices = 1,
    EditSettings,
    OpenLog,
    OpenFolder,
    Reload,
    Autostart,
    Quit,
}

impl Command {
    pub const ALL: [Command; 7] = [
        Command::ShowDevices,
        Command::EditSettings,
        Command::OpenLog,
        Command::OpenFolder,
        Command::Reload,
        Command::Autostart,
        Command::Quit,
    ];

    /// Menu item id.
    pub fn id(self) -> usize {
        self as usize
    }

    pub fn from_id(id: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// One line of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// Non-clickable title (receiver name, "no receiver").
    Header(String),
    /// A device; checked when connected. Clicking opens the device window.
    /// A tab separates the name from the right-aligned status.
    Device {
        label: String,
        online: bool,
    },
    Separator,
    Command {
        command: Command,
        label: &'static str,
        checked: bool,
    },
}

/// Builds the tray menu.
pub fn menu(snapshot: &Snapshot, autostart: bool, has_log: bool) -> Vec<MenuEntry> {
    let mut out = vec![MenuEntry::Header(format!(
        "logirev {}",
        env!("CARGO_PKG_VERSION")
    ))];
    out.push(MenuEntry::Separator);
    if snapshot.links.is_empty() {
        out.push(MenuEntry::Header("No Logitech receiver found".into()));
    }
    for link in &snapshot.links {
        out.push(MenuEntry::Header(link.name.clone()));
        if link.devices.is_empty() {
            out.push(MenuEntry::Header("    no paired device".into()));
        }
        for d in &link.devices {
            out.push(MenuEntry::Device {
                label: device_label(d),
                online: d.online,
            });
        }
    }
    out.push(MenuEntry::Separator);
    let cmd = |command, label| MenuEntry::Command {
        command,
        label,
        checked: false,
    };
    out.push(cmd(Command::ShowDevices, "Show devices..."));
    out.push(cmd(Command::EditSettings, "Edit settings"));
    out.push(cmd(Command::Reload, "Reload settings"));
    if has_log {
        out.push(cmd(Command::OpenLog, "Open log"));
    }
    out.push(cmd(Command::OpenFolder, "Open logirev folder"));
    out.push(MenuEntry::Separator);
    out.push(MenuEntry::Command {
        command: Command::Autostart,
        label: "Start with Windows",
        checked: autostart,
    });
    out.push(MenuEntry::Separator);
    out.push(cmd(Command::Quit, "Quit"));
    out
}

fn device_label(d: &DeviceStatus) -> String {
    let place = if d.slot == RECEIVER_INDEX {
        "Direct".to_owned()
    } else {
        format!("Slot {}", d.slot)
    };
    let status = if d.online {
        d.battery.clone().unwrap_or_else(|| "connected".into())
    } else {
        "not connected".into()
    };
    format!("{place}:  {}\t{status}", d.name)
}

/// Column titles of the device window.
pub const COLUMNS: [&str; 9] = [
    "Receiver",
    "Slot",
    "Device",
    "Type",
    "Status",
    "Battery",
    "DPI",
    "Profile",
    "Remapped buttons",
];

/// Relative column widths (in characters, roughly).
pub const COLUMN_WIDTHS: [i32; 9] = [24, 6, 20, 10, 14, 10, 7, 18, 10];

/// Rows of the device window, one per device.
pub fn rows(snapshot: &Snapshot) -> Vec<[String; 9]> {
    let mut out = Vec::new();
    for link in &snapshot.links {
        for d in &link.devices {
            out.push([
                link.name.clone(),
                if d.slot == RECEIVER_INDEX {
                    "-".into()
                } else {
                    d.slot.to_string()
                },
                d.name.clone(),
                d.kind.clone(),
                if d.online {
                    "connected".into()
                } else {
                    "not connected".into()
                },
                d.battery.clone().unwrap_or_default(),
                d.dpi.map(|v| v.to_string()).unwrap_or_default(),
                d.profile.clone().unwrap_or_else(|| "(none)".into()),
                if d.online {
                    d.remapped.to_string()
                } else {
                    String::new()
                },
            ]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::LinkStatus;

    fn snapshot() -> Snapshot {
        let dev = |slot, name: &str, online, battery: Option<&str>| DeviceStatus {
            slot,
            name: name.into(),
            kind: "mouse".into(),
            online,
            battery: battery.map(Into::into),
            dpi: online.then_some(750),
            profile: Some("MX Ergo".into()),
            remapped: 5,
        };
        Snapshot {
            links: vec![LinkStatus {
                name: "Bolt receiver (046d:c548)".into(),
                is_receiver: true,
                devices: vec![
                    dev(1, "Wave Keys", false, None),
                    dev(2, "MX Ergo S", true, Some("30%")),
                ],
            }],
        }
    }

    #[test]
    fn menu_lists_slots_and_commands() {
        let m = menu(&snapshot(), true, true);
        assert!(m.contains(&MenuEntry::Header("Bolt receiver (046d:c548)".into())));
        assert!(m.contains(&MenuEntry::Device {
            label: "Slot 2:  MX Ergo S\t30%".into(),
            online: true
        }));
        assert!(m.contains(&MenuEntry::Device {
            label: "Slot 1:  Wave Keys\tnot connected".into(),
            online: false
        }));
        assert!(m.contains(&MenuEntry::Command {
            command: Command::Autostart,
            label: "Start with Windows",
            checked: true
        }));
        assert_eq!(
            m.last(),
            Some(&MenuEntry::Command {
                command: Command::Quit,
                label: "Quit",
                checked: false
            })
        );
        assert!(menu(&Snapshot::default(), false, false)
            .contains(&MenuEntry::Header("No Logitech receiver found".into())));
    }

    #[test]
    fn rows_describe_devices() {
        let r = rows(&snapshot());
        assert_eq!(r.len(), 2);
        assert_eq!(r[1][2], "MX Ergo S");
        assert_eq!(r[1][4], "connected");
        assert_eq!(r[0][8], "", "offline devices show no remapped count");
    }

    #[test]
    fn command_ids_round_trip() {
        for c in Command::ALL {
            assert_eq!(Command::from_id(c.id()), Some(c));
        }
        assert_eq!(Command::from_id(0), None);
    }
}
