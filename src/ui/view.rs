//! What the tray menu and the device window contain, independent of the
//! operating system. The Win32 code only draws these lists, so the content
//! can be unit-tested anywhere.

use crate::hidpp::frame::RECEIVER_INDEX;
use crate::status::{DeviceStatus, Snapshot};

/// A command the user can pick from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Command {
    /// Open the settings window (or the native device list).
    ShowDevices = 1,
    /// Open the configuration file.
    EditSettings,
    /// Open the log file.
    OpenLog,
    /// Open the program folder.
    OpenFolder,
    /// Re-read the configuration now.
    Reload,
    /// Toggle start with Windows.
    Autostart,
    /// Stop logirev.
    Quit,
}

impl Command {
    /// Every command, in menu order.
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

    /// The command with menu id `id`.
    pub fn from_id(id: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// First menu id used for "send to computer" items (below are commands).
const HOST_ID_BASE: usize = 1000;

/// What a picked menu id means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    /// A fixed command.
    Command(Command),
    /// Send the device in `slot` of link `link` to computer `host` (from 0).
    SendTo {
        /// Receiver, as `LinkStatus::key`.
        link: String,
        /// Receiver slot of the device.
        slot: u8,
        /// Target computer, counted from 0.
        host: u8,
    },
}

/// One item of a submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Menu id (decode with [`decode`]).
    pub id: usize,
    /// Text shown.
    pub label: String,
    /// Show a tick.
    pub checked: bool,
}

fn host_id(link: usize, slot: u8, host: u8) -> usize {
    HOST_ID_BASE + (link * 256 + usize::from(slot)) * 4 + usize::from(host)
}

/// Decodes a menu id against the snapshot the menu was built from.
pub fn decode(snapshot: &Snapshot, id: usize) -> Option<Picked> {
    if id < HOST_ID_BASE {
        return Command::from_id(id).map(Picked::Command);
    }
    let n = id - HOST_ID_BASE;
    let (host, rest) = ((n % 4) as u8, n / 4);
    let (slot, link) = ((rest % 256) as u8, rest / 256);
    let link = snapshot.links.get(link)?;
    link.devices.iter().find(|d| d.slot == slot && d.online)?;
    Some(Picked::SendTo {
        link: link.key.clone(),
        slot,
        host,
    })
}

/// One line of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// Non-clickable title (receiver name, "no receiver").
    Header(String),
    /// A device; checked when connected. Clicking opens the device window.
    /// A tab separates the name from the right-aligned status.
    Device {
        /// Text shown (a tab separates name and status).
        label: String,
        /// Connected (shown with a tick).
        online: bool,
    },
    /// A horizontal line.
    Separator,
    /// A command.
    Command {
        /// What it does.
        command: Command,
        /// Text shown.
        label: &'static str,
        /// Show a tick.
        checked: bool,
    },
    /// A line opening a submenu.
    Submenu {
        /// Text of the line opening the submenu.
        label: String,
        /// The submenu items.
        items: Vec<MenuItem>,
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

    // Easy-Switch: one submenu per connected device with several computers.
    let mut send = Vec::new();
    for (li, link) in snapshot.links.iter().enumerate() {
        for d in link.devices.iter().filter(|d| d.online) {
            let Some(h) = d.hosts.as_ref().filter(|h| h.count > 1) else {
                continue;
            };
            let items = (0..h.count.min(4))
                .map(|host| MenuItem {
                    id: host_id(li, d.slot, host),
                    label: h.label(host),
                    checked: host == h.current,
                })
                .collect();
            send.push(MenuEntry::Submenu {
                label: format!("Send {} to", d.name),
                items,
            });
        }
    }
    if !send.is_empty() {
        out.push(MenuEntry::Separator);
        out.extend(send);
    }

    out.push(MenuEntry::Separator);
    let cmd = |command, label| MenuEntry::Command {
        command,
        label,
        checked: false,
    };
    let show = if cfg!(feature = "gui") {
        "Open logirev..."
    } else {
        "Show devices..."
    };
    out.push(cmd(Command::ShowDevices, show));
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
pub const COLUMNS: [&str; 10] = [
    "Receiver",
    "Slot",
    "Device",
    "Type",
    "Status",
    "Battery",
    "DPI",
    "Profile",
    "Remapped buttons",
    "Computer",
];

/// Relative column widths (in characters, roughly).
pub const COLUMN_WIDTHS: [i32; 10] = [24, 6, 20, 10, 14, 10, 7, 18, 10, 22];

/// Rows of the device window, one per device.
pub fn rows(snapshot: &Snapshot) -> Vec<[String; 10]> {
    let mut out = Vec::new();
    for link in &snapshot.links {
        for d in &link.devices {
            let computer = match &d.hosts {
                Some(h) if h.count > 1 => format!("{} of {}", h.label(h.current), h.count),
                _ => String::new(),
            };
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
                computer,
            ]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{HostsStatus, LinkStatus};

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
            hosts: online.then(|| HostsStatus {
                count: 2,
                current: 0,
                names: vec![Some("DESKTOP".into()), Some("LAPTOP".into())],
            }),
            details: None,
        };
        Snapshot {
            links: vec![LinkStatus {
                key: "bolt".into(),
                name: "Bolt receiver (046d:c548)".into(),
                is_receiver: true,
                can_pair: true,
                pairing: None,
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
    fn menu_offers_easy_switch_and_decodes_it() {
        let snap = snapshot();
        let m = menu(&snap, false, true);
        let Some(MenuEntry::Submenu { label, items }) =
            m.iter().find(|e| matches!(e, MenuEntry::Submenu { .. }))
        else {
            panic!("send-to submenu expected");
        };
        assert_eq!(label, "Send MX Ergo S to");
        assert_eq!(items.len(), 2);
        assert!(items[0].checked && !items[1].checked);
        assert_eq!(items[1].label, "Computer 2 (LAPTOP)");
        assert_eq!(
            decode(&snap, items[1].id),
            Some(Picked::SendTo {
                link: "bolt".into(),
                slot: 2,
                host: 1
            })
        );
        assert_eq!(
            decode(&snap, Command::Quit.id()),
            Some(Picked::Command(Command::Quit))
        );
        assert_eq!(decode(&Snapshot::default(), items[1].id), None);
    }

    #[test]
    fn rows_describe_devices() {
        let r = rows(&snapshot());
        assert_eq!(r.len(), 2);
        assert_eq!(r[1][2], "MX Ergo S");
        assert_eq!(r[1][4], "connected");
        assert_eq!(r[1][9], "Computer 1 (DESKTOP) of 2");
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
