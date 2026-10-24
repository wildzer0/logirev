//! The background service: keeps every connected device configured.
//!
//! # Why a daemon is needed
//!
//! Office devices such as the MX Ergo S have no memory for user settings:
//! DPI, wheel mode and button "diversion" are **volatile** and are forgotten
//! whenever the device sleeps, is switched off or reconnects. So logirev stays
//! running and:
//!
//! 1. listens to every receiver for *connection* notifications;
//! 2. on every (re)connection, re-applies the matching profile;
//! 3. receives the HID++ notifications of diverted buttons and performs the
//!    configured action (shortcut, click, program, slow mode...).
//!
//! # Event loop
//!
//! Everything runs on one thread that sleeps until something happens:
//!
//! * a reader thread queued a HID++ frame ([`Event::Hid`]);
//! * the user asked to stop ([`Event::Quit`]) or to look for devices
//!   ([`Event::Rescan`]);
//! * a scheduled device setup is due, or the 2-second housekeeping tick
//!   (configuration file changed? receiver plugged in?) elapses.
//!
//! When idle the process therefore uses no CPU at all apart from that tick.
//!
//! # Terminology
//!
//! A [`Link`] is one open receiver (or directly connected device) together
//! with the [`Session`]s of the devices currently connected through it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use hidapi::HidApi;
use log::{debug, error, info, warn};

use crate::config::{
    self, Action, BacklightChange, BacklightMode, Config, Gesture, HostTarget, Macro, Profile,
    SlowMode, Step,
};
use crate::hidpp::diagnostics;
use crate::hidpp::features::{
    control_name, id, BacklightState, Battery, Control, Device, DeviceKind, FeatureTable,
    DISABLE_KEY_NAMES,
};
use crate::hidpp::frame::{Frame, HidppError, RECEIVER_INDEX, REPORT_SHORT, SOFTWARE_ID};
use crate::hidpp::pairing::{Family, Pairing, Stage};
use crate::hidpp::receiver::{paired_devices, PairedDevice};
use crate::hidpp::transport::{self, Incoming, Transport, Waker};
use crate::input::{self, Injector, Key};
use crate::pcap::PcapWriter;
use crate::status::{
    ControlInfo, DeviceDetails, DeviceStatus, HostsStatus, LinkStatus, PairingStatus, Preferences,
    Snapshot, UiHooks, UiUpdate,
};

/// HID++ 1.0 notification sent by a receiver when a device (dis)connects.
const NOTIF_DEVICE_CONNECTION: u8 = 0x41;
/// Receiver register: which notifications the receiver sends us.
const REG_NOTIFICATIONS: u8 = 0x00;
/// Receiver register: writing `0x02` re-sends a connection notification for
/// every device currently connected (used at start-up).
const REG_CONNECTION_STATE: u8 = 0x02;
/// Notification flags: "wireless" (connection events) | "software present".
const NOTIFICATION_FLAGS: [u8; 3] = [0x00, 0x09, 0x00];

/// Delays between successive attempts to configure a device that does not
/// answer yet (it may still be waking up).
const SETUP_BACKOFF: &[Duration] = &[
    Duration::from_millis(300),
    Duration::from_millis(700),
    Duration::from_millis(1500),
    Duration::from_secs(3),
    Duration::from_secs(6),
];
/// Period of the housekeeping tick (configuration changes, hot-plug).
const HOUSEKEEPING: Duration = Duration::from_secs(2);
/// How often to look for *additional* receivers while some are open (only
/// when no interface reports device arrivals).
const RESCAN_WHILE_CONNECTED: Duration = Duration::from_secs(15);
/// Delay between an OS "device arrived" message and the rescan: Windows
/// announces each HID collection separately and they need a moment to be
/// ready, so several announcements are served by one rescan.
const RESCAN_DEBOUNCE: Duration = Duration::from_millis(500);
/// Developer mode: send recent HID++ messages to the window at most this
/// often.
const TRAFFIC_EVERY: Duration = Duration::from_millis(400);

/// Something the event loop must react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A reader thread queued a HID++ frame or a disconnection.
    Hid,
    /// Look for receivers/devices (e.g. the OS reported a new device).
    Rescan,
    /// Re-read the configuration file now.
    Reload,
    /// A different program came to the foreground (file name, e.g.
    /// `chrome.exe`), for per-application buttons.
    Foreground(String),
    /// Send one device to another computer (from the tray menu):
    /// `link` = [`LinkStatus::key`], `host` counted from 0.
    ChangeHost {
        /// Receiver, as [`LinkStatus::key`].
        link: String,
        /// Receiver slot of the device.
        slot: u8,
        /// Target computer, counted from 0.
        host: u8,
    },
    /// Put a receiver in pairing mode.
    StartPairing {
        /// Receiver, as [`LinkStatus::key`].
        link: String,
    },
    /// Bolt: pair the `index`-th device found.
    ChoosePairing {
        /// Receiver, as [`LinkStatus::key`].
        link: String,
        /// Position in the list of devices found.
        index: usize,
    },
    /// Stop pairing (or forget a finished session).
    StopPairing {
        /// Receiver, as [`LinkStatus::key`].
        link: String,
    },
    /// Remove the device in `slot` from the receiver.
    Unpair {
        /// Receiver, as [`LinkStatus::key`].
        link: String,
        /// Receiver slot of the device to remove.
        slot: u8,
    },
    /// A newer version is published (from the update checker).
    UpdateAvailable(String),
    /// Stop: restore every device and return.
    Quit,
}

/// Runs until [`Event::Quit`] is received (or every sender is dropped).
///
/// * `cfg` / `config_path` — the configuration and where it came from
///   (watched for changes);
/// * `tx` / `rx` — the event channel; `tx` is cloned into the HID readers;
/// * `hooks` — the graphical interface, if any, to keep informed.
pub fn run(
    cfg: Config,
    config_path: &Path,
    pcap: Option<Arc<Mutex<PcapWriter>>>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    hooks: Option<UiHooks>,
) -> Result<(), String> {
    let injector = match input::create() {
        Ok(i) => Some(i),
        Err(e) => {
            error!("keyboard/mouse injection unavailable, \"keys\" and \"click\" actions will not work: {e}");
            None
        }
    };
    let api = HidApi::new().map_err(|e| format!("cannot initialise the HID library: {e}"))?;
    let now = Instant::now();
    let mut daemon = Daemon {
        cfg,
        config_mtime: mtime(config_path),
        config_path: config_path.to_path_buf(),
        injector,
        api,
        pcap,
        tx,
        links: Vec::new(),
        failed: BTreeSet::new(),
        waiting_logged: false,
        last_rescan: now,
        rescan_due: None,
        next_housekeeping: now + HOUSEKEEPING,
        hooks,
        dirty: true,
        app: None,
        traffic_seen: 0,
        traffic_sent: Instant::now(),
        traffic_due: None,
    };
    daemon.rescan();
    daemon.publish();
    daemon.send_preferences();
    daemon.event_loop(&rx);
    daemon.shutdown();
    info!("stopped");
    Ok(())
}

struct Daemon {
    cfg: Config,
    config_path: PathBuf,
    config_mtime: Option<SystemTime>,
    injector: Option<Box<dyn Injector>>,
    api: HidApi,
    pcap: Option<Arc<Mutex<PcapWriter>>>,
    tx: Sender<Event>,
    links: Vec<Link>,
    /// Device keys that failed to open (logged once, retried silently).
    failed: BTreeSet<String>,
    waiting_logged: bool,
    last_rescan: Instant,
    /// A rescan requested by the OS, due at this time.
    rescan_due: Option<Instant>,
    next_housekeeping: Instant,
    hooks: Option<UiHooks>,
    /// The interface must be sent a new snapshot.
    dirty: bool,
    /// Program in the foreground (normalised name), if known.
    app: Option<String>,
    /// Developer mode: number of the last message sent to the interface,
    /// when, and when the next batch is due.
    traffic_seen: u64,
    traffic_sent: Instant,
    traffic_due: Option<Instant>,
}

/// What a [`Link`] needs from the daemon while handling events.
struct Ctx<'a> {
    cfg: &'a Config,
    injector: Option<&'a mut (dyn Injector + 'static)>,
    /// Program in the foreground, for per-application buttons.
    app: Option<&'a str>,
    /// Messages for the user, shown after the pass `(title, text)`.
    notices: Vec<(String, String)>,
    /// A button asked to send *every* device to another computer; done by
    /// the daemon after the pass because it involves all links.
    switch_all: Option<HostTarget>,
}

/// Builds the [`Ctx`] for links from a daemon. A macro rather than a method
/// so that only the needed fields are borrowed, leaving `links` free.
macro_rules! ctx {
    ($daemon:expr) => {
        Ctx {
            cfg: &$daemon.cfg,
            injector: $daemon.injector.as_deref_mut(),
            app: $daemon.app.as_deref(),
            notices: Vec::new(),
            switch_all: None,
        }
    };
}

impl Daemon {
    fn event_loop(&mut self, rx: &Receiver<Event>) {
        loop {
            let wait = self.next_wakeup().saturating_duration_since(Instant::now());
            let first = match rx.recv_timeout(wait) {
                Ok(ev) => Some(ev),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            // Coalesce everything already queued: one pass serves them all.
            for ev in first.into_iter().chain(rx.try_iter()) {
                match ev {
                    Event::Quit => return,
                    Event::Rescan => {
                        self.rescan_due
                            .get_or_insert(Instant::now() + RESCAN_DEBOUNCE);
                    }
                    Event::Reload => self.reload_config(),
                    Event::Foreground(program) => self.set_foreground(&program),
                    Event::UpdateAvailable(version) => {
                        info!("logirev {version} is available");
                        self.notify(
                            "Update available",
                            &format!("logirev {version} is available. Open logirev to install it."),
                        );
                    }
                    Event::ChangeHost { link, slot, host } => {
                        if let Some(l) = self.links.iter_mut().find(|l| l.key == link) {
                            l.change_host(slot, HostTarget::Index(host));
                        }
                    }
                    Event::StartPairing { link } => {
                        if let Some(l) = self.links.iter_mut().find(|l| l.key == link) {
                            l.start_pairing();
                        }
                    }
                    Event::ChoosePairing { link, index } => {
                        if let Some(l) = self.links.iter_mut().find(|l| l.key == link) {
                            l.choose_pairing(index);
                        }
                    }
                    Event::StopPairing { link } => {
                        if let Some(l) = self.links.iter_mut().find(|l| l.key == link) {
                            l.stop_pairing();
                        }
                    }
                    Event::Unpair { link, slot } => {
                        let mut ctx = ctx!(self);
                        if let Some(l) = self.links.iter_mut().find(|l| l.key == link) {
                            l.unpair(&mut ctx, slot);
                        }
                    }
                    Event::Hid => {}
                }
            }
            if self.rescan_due.is_some_and(|due| due <= Instant::now()) {
                self.rescan();
            }
            self.pump();
            if Instant::now() >= self.next_housekeeping {
                self.housekeeping();
            }
            self.publish();
        }
    }

    /// When the loop must wake up even if no event arrives.
    fn next_wakeup(&self) -> Instant {
        if self.links.iter().any(|l| l.t.has_pending()) {
            return Instant::now();
        }
        self.links
            .iter()
            .flat_map(|l| {
                let setups = l.pending.values().map(|p| p.due);
                let repeats = l
                    .sessions
                    .values()
                    .filter_map(|s| s.repeat.as_ref().map(|r| r.next));
                setups.chain(repeats)
            })
            .chain(std::iter::once(self.next_housekeeping))
            .chain(self.rescan_due)
            .chain(self.traffic_due)
            .min()
            .unwrap_or(self.next_housekeeping)
    }

    /// Handles every queued frame and every due setup, on every link.
    fn pump(&mut self) {
        let mut ctx = ctx!(self);
        let mut osd = None;
        for link in &mut self.links {
            link.pump(&mut ctx);
            osd = link.osd.take().or(osd);
        }
        let mut i = 0;
        while i < self.links.len() {
            if let Some(reason) = self.links[i].lost.clone() {
                let mut link = self.links.remove(i);
                link.release_all(&mut ctx);
                warn!("lost connection to {}: {reason}", link.t.name);
                self.dirty = true;
            } else {
                i += 1;
            }
        }
        if let (Some(event), Some(h)) = (osd, &self.hooks) {
            if self.cfg.general.osd {
                let italian = crate::locale::italian(&self.cfg.general.language);
                (h.observer)(UiUpdate::Osd(event.text(italian)));
            }
        }
        let (notices, switch_all) = (ctx.notices, ctx.switch_all);
        for (title, text) in notices {
            self.notify(&title, &text);
        }
        if let Some(target) = switch_all {
            info!("sending every connected device to another computer");
            for link in &mut self.links {
                let slots: Vec<u8> = link.sessions.keys().copied().collect();
                for slot in slots {
                    link.change_host(slot, target);
                }
            }
        }
    }

    /// Applies the buttons configured for the program now in front.
    fn set_foreground(&mut self, program: &str) {
        let key = config::program_key(program);
        if self.app.as_deref() == Some(key.as_str()) {
            return;
        }
        debug!("foreground program: {key}");
        self.app = Some(key);
        let mut ctx = ctx!(self);
        for link in &mut self.links {
            link.refresh_buttons(&mut ctx);
        }
    }

    fn housekeeping(&mut self) {
        self.next_housekeeping = Instant::now() + HOUSEKEEPING;
        let current = mtime(&self.config_path);
        if current.is_some() && current != self.config_mtime {
            self.config_mtime = current;
            self.reload_config();
        }
        // With an interface reporting device arrivals there is nothing to
        // poll; otherwise look for receivers now and then.
        let hotplug = self.hooks.as_ref().is_some_and(|h| h.hotplug_events);
        let interval = if self.links.is_empty() {
            HOUSEKEEPING
        } else {
            RESCAN_WHILE_CONNECTED
        };
        if !hotplug && self.last_rescan.elapsed() >= interval {
            self.rescan();
        }
    }

    /// Re-reads the configuration; on error the previous one stays active.
    fn reload_config(&mut self) {
        let new = match Config::load(&self.config_path) {
            Ok(c) => c,
            Err(e) => {
                error!("configuration NOT reloaded, keeping the previous one: {e}");
                self.notify("Settings not applied", &e);
                return;
            }
        };
        if let Ok(level) = config::level_filter(&new.logging.level) {
            log::set_max_level(level);
        }
        self.cfg = new;
        let mut ctx = ctx!(self);
        for link in &mut self.links {
            link.t.traffic().set_enabled(ctx.cfg.general.developer);
            link.apply_new_config(&mut ctx);
        }
        self.dirty = true;
        self.send_preferences();
        info!("configuration reloaded; re-applying profiles (log file and capture settings need a restart)");
    }

    /// Tells the tray process about the settings it applies itself.
    fn send_preferences(&self) {
        if let Some(h) = &self.hooks {
            (h.observer)(UiUpdate::Preferences(Preferences::from_config(&self.cfg)));
        }
    }

    /// Sends a snapshot to the interface if anything changed.
    fn publish(&mut self) {
        if self.cfg.general.developer {
            let seq: u64 = self.links.iter().map(|l| l.t.traffic().last_seq()).sum();
            if seq != self.traffic_seen {
                if self.traffic_sent.elapsed() >= TRAFFIC_EVERY {
                    self.traffic_seen = seq;
                    self.traffic_sent = Instant::now();
                    self.traffic_due = None;
                    self.dirty = true;
                } else {
                    self.traffic_due = Some(self.traffic_sent + TRAFFIC_EVERY);
                }
            }
        }
        let changed = self.dirty || self.links.iter().any(|l| l.changed);
        if !changed {
            return;
        }
        self.dirty = false;
        let mut links = Vec::with_capacity(self.links.len());
        for link in &mut self.links {
            link.changed = false;
            links.push(link.status(&self.cfg));
        }
        if let Some(h) = &self.hooks {
            (h.observer)(UiUpdate::Snapshot(Snapshot { links }));
        }
    }

    /// Shows a message to the user (only when an interface is attached; the
    /// message is always in the log too).
    fn notify(&self, title: &str, text: &str) {
        if let Some(h) = &self.hooks {
            (h.observer)(UiUpdate::Notice {
                title: title.into(),
                text: text.into(),
            });
        }
    }

    /// Opens every receiver/device that is not open yet.
    fn rescan(&mut self) {
        self.last_rescan = Instant::now();
        self.rescan_due = None;
        let _ = self.api.refresh_devices();
        let wanted = self.cfg.product_id().ok().flatten();
        let groups = transport::group_devices(&transport::enumerate(&self.api));
        self.failed.retain(|k| groups.iter().any(|(g, _)| g == k));

        for (key, collections) in groups {
            if self.links.iter().any(|l| l.key == key) {
                continue;
            }
            if wanted.is_some_and(|pid| collections[0].pid != pid) {
                continue;
            }
            let tx = self.tx.clone();
            let waker: Waker = Arc::new(move || {
                let _ = tx.send(Event::Hid);
            });
            match Transport::open(
                &self.api,
                &collections,
                SOFTWARE_ID,
                self.pcap.clone(),
                Some(waker),
            ) {
                Ok(t) => {
                    self.failed.remove(&key);
                    info!("using {}", t.name);
                    t.traffic().set_enabled(self.cfg.general.developer);
                    let mut link = Link::new(key, t);
                    link.start();
                    self.links.push(link);
                    self.dirty = true;
                }
                Err(e) => {
                    let msg = format!("{e} (close Logi Options+ / Solaar, or check permissions)");
                    if self.failed.insert(key) {
                        error!("{msg}");
                        self.notify("Cannot open a Logitech receiver", &msg);
                    } else {
                        debug!("{msg}");
                    }
                }
            }
        }

        if self.links.is_empty() {
            if !self.waiting_logged {
                info!("waiting for a Logitech receiver or device to be plugged in...");
                self.waiting_logged = true;
            }
        } else {
            self.waiting_logged = false;
        }
    }

    /// Clean exit: give every button back to its device.
    fn shutdown(&mut self) {
        let mut ctx = ctx!(self);
        for link in &mut self.links {
            link.restore_all(&mut ctx);
        }
    }
}

/// Last modification time of a file, if readable.
fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// State kept for one connected device.
struct Session {
    name: String,
    kind: DeviceKind,
    battery: Option<Battery>,
    /// Index into `Config::profiles` of the applied profile, if one matched.
    profile: Option<usize>,
    features: FeatureTable,
    /// The device's reprogrammable controls (empty if it has none).
    controls: Vec<Control>,
    /// Index of the ReprogControls feature (diverted button events use it).
    reprog_index: Option<u8>,
    /// Index of WirelessDeviceStatus (direct connections announce reconnects).
    status_index: Option<u8>,
    /// Index of the battery feature, and whether it is UnifiedBattery.
    battery_index: Option<(u8, bool)>,
    /// Buttons currently diverted to us and their actions.
    diverted: BTreeMap<u16, Action>,
    /// Buttons currently diverted *with* raw movement reporting (gestures).
    raw_xy: BTreeSet<u16>,
    /// Buttons reported as held down in the last event.
    pressed: Vec<u16>,
    /// The gesture button being held, with the movement so far.
    gesture: Option<GestureState>,
    /// Pointer speed / DPI to return to when slow mode is switched off.
    base_speed: Option<u16>,
    base_dpi: Option<u16>,
    /// The slow setting currently active, if slow mode is on.
    slow: Option<SlowMode>,
    /// Keyboard backlight state and number of manual levels.
    backlight: Option<(BacklightState, u8)>,
    /// Easy-Switch channels.
    hosts: Option<HostsStatus>,
    /// A macro repeating while its button is held.
    repeat: Option<Repeat>,
    /// Capabilities shown in the settings window.
    details: DeviceDetails,
}

/// A macro that runs again every `interval` while `cid` is held.
struct Repeat {
    cid: u16,
    mac: Macro,
    interval: Duration,
    next: Instant,
}

/// Movement accumulated while a gesture button is held.
#[derive(Debug, Clone, Copy)]
struct GestureState {
    cid: u16,
    dx: i32,
    dy: i32,
}

/// A device that must be (re)configured at `due`.
struct PendingSetup {
    due: Instant,
    attempt: usize,
}

/// One open receiver or direct device and the devices connected through it.
struct Link {
    key: String,
    t: Transport,
    sessions: BTreeMap<u8, Session>,
    pending: BTreeMap<u8, PendingSetup>,
    /// Devices the receiver remembers, connected or not.
    paired: Vec<PairedDevice>,
    /// Slots already warned about a low battery (until it is charged).
    battery_warned: BTreeSet<u8>,
    /// Set when the reader threads report the device is gone.
    lost: Option<String>,
    /// Something the interface shows changed.
    changed: bool,
    /// A pairing session in progress (or just finished).
    pairing: Option<Pairing>,
    /// A change made by a button, to show on screen.
    osd: Option<OsdEvent>,
    /// Slots seen charging, to announce when they are full.
    charging: BTreeSet<u8>,
}

/// A change made by a button, shown briefly on screen.
#[derive(Debug, Clone, PartialEq)]
enum OsdEvent {
    /// Precision mode on (`Some(dpi)` when done with DPI) or off.
    Precision(bool, Option<u16>),
    /// New DPI.
    Dpi(u16),
    /// Device sent to computer N (1-based).
    Host(String, u8),
    /// Keyboard backlight: off, or on at `level` of `levels`.
    Backlight(bool, u8, u8),
}

impl OsdEvent {
    fn text(&self, italian: bool) -> String {
        let tr = |en, it| crate::locale::pick(italian, en, it).to_owned();
        match self {
            OsdEvent::Precision(true, Some(dpi)) => {
                format!(
                    "{} · {dpi} DPI",
                    tr("Precision mode on", "Modalità precisione attiva")
                )
            }
            OsdEvent::Precision(true, None) => {
                tr("Precision mode on", "Modalità precisione attiva")
            }
            OsdEvent::Precision(false, _) => {
                tr("Precision mode off", "Modalità precisione disattivata")
            }
            OsdEvent::Dpi(dpi) => format!("{dpi} DPI"),
            OsdEvent::Host(name, n) => format!("{name} → {} {n}", tr("computer", "computer")),
            OsdEvent::Backlight(false, ..) => tr("Backlight off", "Retroilluminazione spenta"),
            OsdEvent::Backlight(true, level, levels) => format!(
                "{} {}/{}",
                tr("Backlight", "Retroilluminazione"),
                level + 1,
                levels
            ),
        }
    }
}

impl Link {
    fn new(key: String, t: Transport) -> Self {
        Self {
            key,
            t,
            sessions: BTreeMap::new(),
            pending: BTreeMap::new(),
            paired: Vec::new(),
            battery_warned: BTreeSet::new(),
            lost: None,
            changed: true,
            pairing: None,
            osd: None,
            charging: BTreeSet::new(),
        }
    }

    /// What the interface shows for this link.
    fn status(&self, cfg: &Config) -> LinkStatus {
        let mut devices: Vec<DeviceStatus> = self
            .sessions
            .iter()
            .map(|(slot, s)| DeviceStatus {
                slot: *slot,
                name: s.name.clone(),
                kind: s.kind.to_string(),
                online: true,
                battery: s.battery.map(|b| b.to_string()),
                battery_level: s.battery.map(|b| (b.percent, b.charging)),
                dpi: s.base_dpi,
                profile: s
                    .profile
                    .and_then(|i| cfg.profiles.get(i))
                    .map(|p| p.name.clone()),
                remapped: s.diverted.len(),
                hosts: s.hosts.clone(),
                details: Some(s.details.clone()),
            })
            .collect();
        for p in &self.paired {
            if !self.sessions.contains_key(&p.slot) {
                let name = p.name.clone().unwrap_or_else(|| "unnamed device".into());
                devices.push(DeviceStatus {
                    slot: p.slot,
                    profile: cfg.profile_for(&name, p.kind).map(|pr| pr.name.clone()),
                    name,
                    kind: p.kind.to_string(),
                    online: false,
                    battery: None,
                    battery_level: None,
                    dpi: None,
                    remapped: 0,
                    hosts: None,
                    details: None,
                });
            }
        }
        devices.sort_by_key(|d| d.slot);
        LinkStatus {
            key: self.key.clone(),
            name: self.t.name.clone(),
            is_receiver: self.t.is_receiver,
            devices,
            can_pair: Family::of(self.t.pid).is_some(),
            pairing: self.pairing.as_ref().map(|p| PairingStatus {
                family: p.family,
                stage: p.stage.clone(),
                found: p.found.clone(),
            }),
            traffic: if cfg.general.developer {
                self.t.traffic().recent()
            } else {
                Vec::new()
            },
        }
    }

    /// Initial discovery of the devices that are already connected.
    fn start(&mut self) {
        let t = &mut self.t;
        if !t.is_receiver {
            self.schedule(RECEIVER_INDEX, Duration::ZERO);
            return;
        }
        self.paired = paired_devices(t);
        for p in &self.paired {
            let name = p.name.as_deref().unwrap_or("unnamed device");
            info!(
                "  slot {}: paired \"{name}\" ({}, id {:04x})",
                p.slot, p.kind, p.wpid
            );
        }
        // Ask for connection notifications, keeping whatever was enabled.
        match t.read_register(REG_NOTIFICATIONS, &[]) {
            Ok(cur) if cur.len() >= 3 => {
                let flags: Vec<u8> = cur[..3]
                    .iter()
                    .zip(NOTIFICATION_FLAGS)
                    .map(|(a, b)| a | b)
                    .collect();
                if let Err(e) = t.write_register(REG_NOTIFICATIONS, &flags) {
                    warn!("cannot enable receiver notifications: {e}");
                }
            }
            Ok(_) => warn!("unexpected answer reading the receiver notification register"),
            Err(e) => warn!("cannot read receiver notification register: {e}"),
        }
        // Have the receiver announce every connected device; fall back to
        // probing each slot if this receiver does not support it.
        if let Err(e) = t.write_register(REG_CONNECTION_STATE, &[0x02, 0x00, 0x00]) {
            debug!("connection re-announce not supported ({e}), probing slots");
            let alive: Vec<u8> = (1..=6).filter(|&s| t.ping(s).is_some()).collect();
            for slot in alive {
                self.schedule(slot, Duration::ZERO);
            }
        }
    }

    /// Handles queued frames and due setups until nothing is left.
    fn pump(&mut self, ctx: &mut Ctx) {
        loop {
            while let Some(incoming) = self.t.try_next() {
                match incoming {
                    Incoming::Frame(f) => self.on_frame(ctx, &f),
                    Incoming::Lost(reason) => {
                        self.lost = Some(reason);
                        return;
                    }
                }
            }
            self.run_due_setups(ctx);
            self.run_due_repeats(ctx);
            if self.pairing.as_mut().is_some_and(Pairing::check_timeout) {
                self.changed = true;
            }
            // Setups and actions may have set notifications aside.
            if !self.t.has_pending() {
                return;
            }
        }
    }

    fn on_frame(&mut self, ctx: &mut Ctx, f: &Frame) {
        let slot = f.device_index();

        // Pairing notifications (the frame is still handled normally below:
        // a newly paired device must also be configured).
        if let Some(p) = self.pairing.as_mut() {
            if p.on_frame(f) {
                self.changed = true;
                if let Stage::Paired { slot: new } = p.stage {
                    info!("paired a new device in slot {new}");
                    self.paired = paired_devices(&mut self.t);
                } else if let Stage::Failed { reason } = &p.stage {
                    warn!("pairing failed: {reason}");
                }
            }
        }

        // Receiver: a device connected or disconnected.
        if self.t.is_receiver && f.report_id() == REPORT_SHORT && f.b2() == NOTIF_DEVICE_CONNECTION
        {
            let link_up = f.params()[0] & 0x40 == 0;
            if link_up {
                debug!("slot {slot}: link established");
                self.schedule(slot, SETUP_BACKOFF[0]);
            } else if let Some(s) = self.sessions.remove(&slot) {
                info!(
                    "slot {slot}: \"{}\" disconnected (asleep or switched off)",
                    s.name
                );
                release_session(ctx, &s);
                self.pending.remove(&slot);
                self.changed = true;
            }
            return;
        }

        // Only HID++ 2.0 notifications (software id 0) from known devices
        // are interesting from here on.
        let Some(session) = self.sessions.get(&slot) else {
            return;
        };
        if f.software_id() != 0 {
            return;
        }
        let (index, function, params) = (Some(f.b2()), f.function(), f.params());
        if index == session.reprog_index && function == 0 {
            self.on_buttons(ctx, slot, params);
        } else if index == session.reprog_index && function == 1 {
            self.on_raw_xy(slot, params);
        } else if index == session.battery_index.map(|(i, _)| i) && function == 0 {
            self.on_battery(ctx, slot, params);
        } else if index == session.status_index && function == 0 && params[0] == 1 {
            // WirelessDeviceStatus "reconnection": the device lost its settings.
            info!("slot {slot}: device reports a reconnection, re-applying profile");
            self.schedule(slot, SETUP_BACKOFF[0]);
        }
    }

    /// Handles a "diverted buttons" event: the list of buttons held now.
    fn on_buttons(&mut self, ctx: &mut Ctx, slot: u8, params: &[u8]) {
        let now: Vec<u16> = params
            .as_chunks::<2>()
            .0
            .iter()
            .take(4)
            .map(|c| u16::from_be_bytes(*c))
            .filter(|&cid| cid != 0)
            .collect();
        let Some(session) = self.sessions.get_mut(&slot) else {
            return;
        };
        let before = std::mem::replace(&mut session.pressed, now.clone());
        // Resolve actions first: performing them needs `&mut self`.
        let released: Vec<(u16, Action)> = before
            .iter()
            .filter(|c| !now.contains(c))
            .filter_map(|c| session.diverted.get(c).map(|a| (*c, a.clone())))
            .collect();
        let pressed: Vec<(u16, Action)> = now
            .iter()
            .filter(|c| !before.contains(c))
            .filter_map(|c| session.diverted.get(c).map(|a| (*c, a.clone())))
            .collect();

        for (cid, action) in released {
            debug!("slot {slot}: button {} released", describe(cid));
            if let Some(s) = self.sessions.get_mut(&slot) {
                if s.repeat.as_ref().is_some_and(|r| r.cid == cid) {
                    s.repeat = None;
                }
            }
            if let Action::Gesture(g) = &action {
                self.finish_gesture(ctx, slot, cid, g);
            } else {
                self.perform(ctx, slot, &action, false);
            }
        }
        for (cid, action) in pressed {
            info!(
                "slot {slot}: button {} pressed -> {}",
                describe(cid),
                summarize(&action)
            );
            if let Action::Gesture(_) = action {
                if let Some(s) = self.sessions.get_mut(&slot) {
                    s.gesture = Some(GestureState { cid, dx: 0, dy: 0 });
                }
            } else {
                self.perform(ctx, slot, &action, true);
                if let Action::Sequence(m) = &action {
                    if let (Some(ms), Some(s)) = (m.repeat, self.sessions.get_mut(&slot)) {
                        let interval = Duration::from_millis(u64::from(ms));
                        s.repeat = Some(Repeat {
                            cid,
                            mac: m.clone(),
                            interval,
                            next: Instant::now() + interval,
                        });
                    }
                }
            }
        }
    }

    /// Runs again the macros whose button is still held.
    fn run_due_repeats(&mut self, ctx: &mut Ctx) {
        let now = Instant::now();
        for s in self.sessions.values_mut() {
            let Some(r) = s.repeat.as_mut().filter(|r| r.next <= now) else {
                continue;
            };
            r.next = now + r.interval;
            if let Some(inj) = ctx.injector.as_deref_mut() {
                if let Err(e) = run_sequence(inj, &r.mac.steps) {
                    warn!("repeating macro failed: {e}");
                    s.repeat = None;
                }
            }
        }
    }

    /// Movement reported while a gesture button is held.
    fn on_raw_xy(&mut self, slot: u8, params: &[u8]) {
        let Some(g) = self
            .sessions
            .get_mut(&slot)
            .and_then(|s| s.gesture.as_mut())
        else {
            return;
        };
        if params.len() >= 4 {
            g.dx += i32::from(i16::from_be_bytes([params[0], params[1]]));
            g.dy += i32::from(i16::from_be_bytes([params[2], params[3]]));
        }
    }

    /// The gesture button was released: run the action for the movement.
    fn finish_gesture(&mut self, ctx: &mut Ctx, slot: u8, cid: u16, gesture: &Gesture) {
        let state = self.sessions.get_mut(&slot).and_then(|s| s.gesture.take());
        let (dx, dy) = match state {
            Some(g) if g.cid == cid => (g.dx, g.dy),
            _ => (0, 0),
        };
        match gesture.pick(dx, dy) {
            Some(action) => {
                info!("slot {slot}: gesture ({dx}, {dy}) -> {}", summarize(action));
                let action = action.clone();
                self.perform(ctx, slot, &action, true);
                self.perform(ctx, slot, &action, false);
            }
            None => debug!("slot {slot}: gesture ({dx}, {dy}) has no action"),
        }
    }

    /// Spontaneous battery report.
    fn on_battery(&mut self, ctx: &mut Ctx, slot: u8, params: &[u8]) {
        let Some(s) = self.sessions.get_mut(&slot) else {
            return;
        };
        let unified = s.battery_index.is_some_and(|(_, u)| u);
        let parsed = if unified {
            Battery::from_unified(params)
        } else {
            Battery::from_legacy(params)
        };
        if let Some(b) = parsed {
            if s.battery != Some(b) {
                debug!("slot {slot}: battery {b}");
                s.battery = Some(b);
                self.changed = true;
            }
            self.check_battery(ctx, slot);
        }
    }

    /// Warns once when a battery falls to the configured level; the warning
    /// re-arms after charging (or 5 points above the level).
    fn check_battery(&mut self, ctx: &mut Ctx, slot: u8) {
        let level = ctx.cfg.general.battery_warning;
        let Some(s) = self.sessions.get(&slot) else {
            return;
        };
        let Some(b) = s.battery else {
            return;
        };
        let italian = crate::locale::italian(&ctx.cfg.general.language);
        let tr = |en, it| crate::locale::pick(italian, en, it);
        if b.charging && !b.full {
            self.charging.insert(slot);
        } else if b.full && self.charging.remove(&slot) {
            info!("slot {slot}: \"{}\" fully charged", s.name);
            ctx.notices.push((
                tr("Fully charged", "Carica completa").into(),
                format!(
                    "{}: {}",
                    s.name,
                    tr("the battery is full.", "la batteria è carica.")
                ),
            ));
        } else if !b.charging {
            self.charging.remove(&slot);
        }
        if level == 0 || b.charging || b.full || b.percent > level.saturating_add(5) {
            self.battery_warned.remove(&slot);
        } else if b.percent <= level && self.battery_warned.insert(slot) {
            warn!("slot {slot}: \"{}\" battery low ({}%)", s.name, b.percent);
            ctx.notices.push((
                tr("Battery low", "Batteria scarica").into(),
                format!(
                    "{}: {}% {}",
                    s.name,
                    b.percent,
                    tr("left. Charge it soon.", "rimasto. Ricaricala presto.")
                ),
            ));
        }
    }

    /// Executes the press (`down`) or release half of an action.
    fn perform(&mut self, ctx: &mut Ctx, slot: u8, action: &Action, down: bool) {
        let result = match action {
            Action::Keys(keys) => match ctx.injector.as_deref_mut() {
                Some(inj) if down => keys.iter().try_for_each(|k| inj.key(k, true)),
                Some(inj) => keys.iter().rev().try_for_each(|k| inj.key(k, false)),
                None => Ok(()),
            },
            Action::Click(button) => match ctx.injector.as_deref_mut() {
                Some(inj) => inj.button(*button, down),
                None => Ok(()),
            },
            Action::Scroll {
                vertical,
                horizontal,
            } if down => match ctx.injector.as_deref_mut() {
                Some(inj) => inj.scroll(*vertical, *horizontal),
                None => Ok(()),
            },
            Action::Sequence(m) if down => match ctx.injector.as_deref_mut() {
                Some(inj) => run_sequence(inj, &m.steps),
                None => Ok(()),
            },
            Action::Brightness(change) if down => {
                crate::brightness::adjust(*change);
                Ok(())
            }
            Action::Backlight(change) if down => {
                self.change_backlight(slot, *change);
                Ok(())
            }
            Action::Command { program, args } if down => spawn(program, args),
            Action::Toggle(slow) if down => {
                self.toggle_slow_mode(slot, *slow);
                Ok(())
            }
            Action::CycleDpi(values) if down => {
                self.cycle_dpi(slot, values);
                Ok(())
            }
            Action::ChangeHost { target, all } if down => {
                if *all {
                    ctx.switch_all = Some(*target);
                } else {
                    self.change_host(slot, *target);
                }
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            warn!("action {} failed: {e}", summarize(action));
        }
    }

    fn toggle_slow_mode(&mut self, slot: u8, slow: SlowMode) {
        let Some(s) = self.sessions.get_mut(&slot) else {
            return;
        };
        let turning_on = s.slow.is_none();
        let mut dev = Device::new(&mut self.t, slot, &s.features);
        let result = if turning_on {
            set_slow(&mut dev, slow)
        } else {
            restore_normal(&mut dev, s.slow.unwrap_or(slow), s.base_speed, s.base_dpi)
        };
        match result {
            Ok(()) => {
                s.slow = turning_on.then_some(slow);
                info!("slot {slot}: slow mode {}", on_off(turning_on));
                let dpi = match slow {
                    SlowMode::Dpi(d) => Some(d),
                    SlowMode::PointerSpeed(_) => None,
                };
                self.osd = Some(OsdEvent::Precision(turning_on, dpi));
            }
            Err(e) => warn!("slot {slot}: cannot switch slow mode: {e}"),
        }
    }

    /// Moves to the next DPI of `values` (after the current one).
    fn cycle_dpi(&mut self, slot: u8, values: &[u16]) {
        let Some(s) = self.sessions.get_mut(&slot) else {
            return;
        };
        let next = s
            .base_dpi
            .and_then(|cur| values.iter().position(|&v| v == cur))
            .map_or(values[0], |i| values[(i + 1) % values.len()]);
        match Device::new(&mut self.t, slot, &s.features).set_dpi(next) {
            Ok(()) => {
                s.base_dpi = Some(next);
                s.slow = None;
                self.changed = true;
                info!("slot {slot}: DPI {next}");
                self.osd = Some(OsdEvent::Dpi(next));
            }
            Err(e) => warn!("slot {slot}: cannot set DPI {next}: {e}"),
        }
    }

    /// Keyboard backlight: one level brighter / darker, or on / off.
    fn change_backlight(&mut self, slot: u8, change: BacklightChange) {
        let Some(s) = self.sessions.get_mut(&slot) else {
            return;
        };
        let Some((mut state, levels)) = s.backlight else {
            warn!("slot {slot}: \"{}\" has no adjustable backlight", s.name);
            return;
        };
        let top = levels.saturating_sub(1);
        match change {
            BacklightChange::Toggle => state.enabled = !state.enabled,
            BacklightChange::Up | BacklightChange::Down => {
                if !state.supports_manual() {
                    warn!("slot {slot}: this keyboard has no manual backlight levels");
                    return;
                }
                let current = if state.mode() == BacklightState::MODE_MANUAL && state.enabled {
                    state.level
                } else {
                    top / 2
                };
                state.level = if change == BacklightChange::Up {
                    current.saturating_add(1).min(top)
                } else {
                    current.saturating_sub(1)
                };
                state.enabled = true;
                state.set_mode(BacklightState::MODE_MANUAL);
            }
        }
        match Device::new(&mut self.t, slot, &s.features).set_backlight(&state) {
            Ok(()) => {
                info!("slot {slot}: backlight {state}");
                s.backlight = Some((state, levels));
                self.osd = Some(OsdEvent::Backlight(state.enabled, state.level, levels));
            }
            Err(e) => warn!("slot {slot}: cannot change the backlight: {e}"),
        }
    }

    // ------------------------------------------------------------------
    // Pairing
    // ------------------------------------------------------------------

    fn start_pairing(&mut self) {
        if let Some(mut old) = self.pairing.take() {
            old.cancel(&mut self.t);
        }
        let known: Vec<u8> = self
            .paired
            .iter()
            .map(|p| p.slot)
            .chain(self.sessions.keys().copied())
            .collect();
        match Pairing::start(&mut self.t, known) {
            Ok(p) => {
                info!("{}: pairing mode on ({:?})", self.t.name, p.family);
                self.pairing = Some(p);
            }
            Err(e) => {
                warn!("{}: cannot start pairing: {e}", self.t.name);
                self.pairing = None;
            }
        }
        self.changed = true;
    }

    fn choose_pairing(&mut self, index: usize) {
        let Some(p) = self.pairing.as_mut() else {
            return;
        };
        if let Err(e) = p.choose(&mut self.t, index) {
            warn!("cannot pair the chosen device: {e}");
            p.stage = Stage::Failed {
                reason: e.to_string(),
            };
        }
        self.changed = true;
    }

    fn stop_pairing(&mut self) {
        if let Some(mut p) = self.pairing.take() {
            p.cancel(&mut self.t);
            self.changed = true;
        }
    }

    /// Removes the device in `slot` from the receiver.
    fn unpair(&mut self, ctx: &mut Ctx, slot: u8) {
        match crate::hidpp::pairing::unpair(&mut self.t, slot) {
            Ok(()) => {
                info!("{}: removed the device in slot {slot}", self.t.name);
                if let Some(s) = self.sessions.remove(&slot) {
                    release_session(ctx, &s);
                }
                self.pending.remove(&slot);
                self.paired = paired_devices(&mut self.t);
            }
            Err(e) => warn!("{}: cannot remove slot {slot}: {e}", self.t.name),
        }
        self.changed = true;
    }

    /// Easy-Switch: sends the device to another computer.
    fn change_host(&mut self, slot: u8, target: HostTarget) {
        let Some(s) = self.sessions.get(&slot) else {
            return;
        };
        let mut dev = Device::new(&mut self.t, slot, &s.features);
        let result = dev.hosts().and_then(|(count, current)| {
            let host = match target {
                HostTarget::Next => (current + 1) % count.max(1),
                HostTarget::Index(i) if i < count => i,
                HostTarget::Index(i) => {
                    warn!(
                        "slot {slot}: host {} does not exist (the device has {count})",
                        i + 1
                    );
                    return Ok(());
                }
            };
            if host == current {
                return Ok(());
            }
            info!("slot {slot}: switching to computer {} of {count}", host + 1);
            self.osd = Some(OsdEvent::Host(s.name.clone(), host + 1));
            dev.set_host(host)
        });
        if let Err(e) = result {
            warn!("slot {slot}: cannot switch computer: {e}");
        }
    }

    // ------------------------------------------------------------------
    // Configuring devices
    // ------------------------------------------------------------------

    fn schedule(&mut self, slot: u8, delay: Duration) {
        self.pending.insert(
            slot,
            PendingSetup {
                due: Instant::now() + delay,
                attempt: 0,
            },
        );
    }

    fn run_due_setups(&mut self, ctx: &mut Ctx) {
        let now = Instant::now();
        let due: Vec<u8> = self
            .pending
            .iter()
            .filter(|(_, p)| p.due <= now)
            .map(|(s, _)| *s)
            .collect();
        for slot in due {
            let attempt = self.pending.get(&slot).map_or(0, |p| p.attempt);
            match self.setup(ctx.cfg, ctx.app, slot) {
                Ok(session) => {
                    self.pending.remove(&slot);
                    self.sessions.insert(slot, session);
                    self.changed = true;
                    self.check_battery(ctx, slot);
                }
                Err(e) if attempt + 1 < SETUP_BACKOFF.len() => {
                    debug!("slot {slot}: not ready yet ({e}), retrying");
                    self.pending.insert(
                        slot,
                        PendingSetup {
                            due: now + SETUP_BACKOFF[attempt + 1],
                            attempt: attempt + 1,
                        },
                    );
                }
                Err(e) => {
                    warn!("slot {slot}: giving up configuring the device: {e}");
                    self.pending.remove(&slot);
                }
            }
        }
    }

    /// Identifies the device in `slot` and applies its profile.
    fn setup(&mut self, cfg: &Config, app: Option<&str>, slot: u8) -> Result<Session, HidppError> {
        let t = &mut self.t;
        let (major, minor) = t.ping(slot).ok_or(HidppError::Timeout)?;
        if major < 2 {
            // HID++ 1.0 devices (very old) have none of the features we use.
            info!("slot {slot}: HID++ {major}.{minor} device, nothing to configure");
        }
        let features = FeatureTable::discover(t, slot)?;
        let mut dev = Device::new(t, slot, &features);
        let name = dev.name().unwrap_or_else(|_| "unknown device".into());
        let kind = dev.kind().unwrap_or(DeviceKind::Other);
        let battery = dev.battery().ok();
        let battery_text = battery
            .map(|b| format!(", battery {b}"))
            .unwrap_or_default();
        let battery_index = features
            .index_of(id::UNIFIED_BATTERY)
            .map(|i| (i, true))
            .or_else(|| features.index_of(id::BATTERY_STATUS).map(|i| (i, false)));

        let mut session = Session {
            reprog_index: features.index_of(id::REPROG_CONTROLS_V4),
            status_index: features.index_of(id::WIRELESS_DEVICE_STATUS),
            battery_index,
            name: name.clone(),
            kind,
            battery,
            profile: None,
            features: features.clone(),
            controls: Vec::new(),
            diverted: BTreeMap::new(),
            raw_xy: BTreeSet::new(),
            pressed: Vec::new(),
            gesture: None,
            base_speed: None,
            base_dpi: None,
            slow: None,
            backlight: None,
            hosts: None,
            repeat: None,
            details: DeviceDetails::default(),
        };

        if features.has(id::REPROG_CONTROLS_V4) {
            session.controls = dev.controls()?;
        }
        session.details = collect_details(&mut dev, &features, &session.controls);

        if features.has(id::CHANGE_HOST) {
            if let Ok((count, current)) = dev.hosts() {
                let names = if features.has(id::HOSTS_INFO) {
                    dev.host_names().unwrap_or_default()
                } else {
                    Vec::new()
                };
                session.hosts = Some(HostsStatus {
                    count,
                    current,
                    names,
                });
            }
        }

        info!("slot {slot}: \"{name}\" ({kind}) connected{battery_text}");
        apply_profile(&mut dev, cfg, app, slot, &mut session);
        if cfg.general.developer {
            session.details.tech = Some(diagnostics::read(t, slot, &features));
        }
        Ok(session)
    }

    /// The foreground program changed: switch every device to the buttons
    /// configured for it.
    fn refresh_buttons(&mut self, ctx: &mut Ctx) {
        for (slot, s) in self.sessions.iter_mut() {
            let Some(profile) = s.profile.and_then(|i| ctx.cfg.profiles.get(i)) else {
                continue;
            };
            let wanted = profile.buttons_for(ctx.app);
            if wanted == s.diverted {
                continue;
            }
            // Let go of anything held with the old mapping first.
            release_session(ctx, s);
            s.pressed.clear();
            s.gesture = None;
            let mut dev = Device::new(&mut self.t, *slot, &s.features);
            let before = s.diverted.len();
            apply_buttons(
                &mut dev,
                &s.controls,
                &mut s.diverted,
                &mut s.raw_xy,
                wanted,
                *slot,
            );
            debug!(
                "slot {slot}: buttons for {} ({before} -> {} remapped)",
                ctx.app.unwrap_or("desktop"),
                s.diverted.len()
            );
            self.changed = true;
        }
    }

    /// Gives every diverted button back to the firmware and undoes slow mode.
    fn restore_all(&mut self, ctx: &mut Ctx) {
        self.release_all(ctx);
        for (slot, s) in &self.sessions {
            let mut dev = Device::new(&mut self.t, *slot, &s.features);
            for cid in s.diverted.keys() {
                let raw = s.raw_xy.contains(cid).then_some(false);
                if let Err(e) = dev.set_reporting(*cid, false, raw) {
                    debug!("slot {slot}: cannot restore button {}: {e}", describe(*cid));
                }
            }
            if let Some(slow) = s.slow {
                let _ = restore_normal(&mut dev, slow, s.base_speed, s.base_dpi);
            }
            if !s.diverted.is_empty() {
                info!("slot {slot}: buttons restored to their default behaviour");
            }
        }
    }

    /// Applies a new configuration to the connected devices.
    ///
    /// The devices stay connected: only what the profile changes is sent
    /// (a few short messages), so the settings window never sees them
    /// disappear. Devices still being set up get the new profile when ready.
    fn apply_new_config(&mut self, ctx: &mut Ctx) {
        self.release_all(ctx);
        for (slot, s) in self.sessions.iter_mut() {
            let features = s.features.clone();
            let mut dev = Device::new(&mut self.t, *slot, &features);
            if let Some(slow) = s.slow.take() {
                let _ = restore_normal(&mut dev, slow, s.base_speed, s.base_dpi);
            }
            s.gesture = None;
            s.repeat = None;
            apply_profile(&mut dev, ctx.cfg, ctx.app, *slot, s);
            if !ctx.cfg.general.developer {
                s.details.tech = None;
            } else if s.details.tech.is_none() {
                s.details.tech = Some(diagnostics::read(&mut self.t, *slot, &features));
            }
        }
        self.changed = true;
    }

    /// Releases any key/click still held (avoids "stuck" keys).
    fn release_all(&mut self, ctx: &mut Ctx) {
        for s in self.sessions.values_mut() {
            release_session(ctx, s);
            s.pressed.clear();
        }
    }
}

/// Applies the first profile matching the device in `slot`; with none, the
/// device gets its own button behaviour back.
fn apply_profile(dev: &mut Device, cfg: &Config, app: Option<&str>, slot: u8, s: &mut Session) {
    let features = s.features.clone();
    s.profile = cfg
        .profiles
        .iter()
        .position(|p| p.matcher.fits(&s.name, s.kind));
    let Some(profile) = s.profile.map(|i| &cfg.profiles[i]) else {
        info!(
            "slot {slot}: no profile matches \"{}\", leaving it unchanged",
            s.name
        );
        apply_buttons(
            dev,
            &s.controls,
            &mut s.diverted,
            &mut s.raw_xy,
            BTreeMap::new(),
            slot,
        );
        return;
    };
    info!("slot {slot}: applying profile \"{}\"", profile.name);
    apply_settings(dev, &features, profile, slot);
    if features.has(id::POINTER_SPEED) {
        s.base_speed = profile.pointer_speed.or_else(|| dev.pointer_speed().ok());
    }
    if features.has(id::ADJUSTABLE_DPI) {
        s.base_dpi = profile.dpi.or_else(|| dev.dpi().ok());
    }
    if features.has(id::BACKLIGHT2) {
        s.backlight = apply_backlight(dev, profile, slot);
    }
    let wanted = profile.buttons_for(app);
    if !wanted.is_empty() && !features.has(id::REPROG_CONTROLS_V4) {
        warn!("slot {slot}: device has no reprogrammable buttons, ignoring the buttons section");
    }
    apply_buttons(
        dev,
        &s.controls,
        &mut s.diverted,
        &mut s.raw_xy,
        wanted,
        slot,
    );
    info!("slot {slot}: {} button(s) remapped", s.diverted.len());
}

/// Releases the keys/clicks of the buttons a session reports as held.
fn release_session(ctx: &mut Ctx, s: &Session) {
    let Some(inj) = ctx.injector.as_deref_mut() else {
        return;
    };
    for cid in &s.pressed {
        match s.diverted.get(cid) {
            Some(Action::Keys(keys)) => keys.iter().rev().for_each(|k| drop(inj.key(k, false))),
            Some(Action::Click(b)) => drop(inj.button(*b, false)),
            _ => {}
        }
    }
}

/// Makes the device's diverted buttons match `wanted`: buttons no longer
/// wanted are given back, new ones are diverted (with movement reporting
/// for gestures), and actions are updated.
fn apply_buttons(
    dev: &mut Device,
    controls: &[Control],
    diverted: &mut BTreeMap<u16, Action>,
    raw_xy: &mut BTreeSet<u16>,
    wanted: BTreeMap<u16, Action>,
    slot: u8,
) {
    let gone: Vec<u16> = diverted
        .keys()
        .filter(|c| !wanted.contains_key(c))
        .copied()
        .collect();
    for cid in gone {
        let was_raw = raw_xy.remove(&cid);
        if let Err(e) = dev.set_reporting(cid, false, was_raw.then_some(false)) {
            debug!("slot {slot}: cannot restore button {}: {e}", describe(cid));
        }
        diverted.remove(&cid);
    }
    for (cid, action) in wanted {
        let Some(control) = controls.iter().find(|c| c.cid == cid) else {
            if !controls.is_empty() {
                warn!("slot {slot}: this device has no button {}", describe(cid));
            }
            continue;
        };
        if !control.is_divertable() {
            warn!("slot {slot}: button {} cannot be remapped", describe(cid));
            continue;
        }
        let gesture = matches!(action, Action::Gesture(_));
        let raw = gesture && control.supports_raw_xy();
        if gesture && !raw {
            warn!(
                "slot {slot}: button {} cannot report movement; its gesture will only do the \"click\" action",
                describe(cid)
            );
        }
        let was_raw = raw_xy.contains(&cid);
        let already = diverted.contains_key(&cid) && was_raw == raw;
        if !already {
            // Only touch the raw-XY setting when it has to change.
            let raw_change = (raw || was_raw).then_some(raw);
            if let Err(e) = dev.set_reporting(cid, true, raw_change) {
                warn!("slot {slot}: cannot remap button {}: {e}", describe(cid));
                continue;
            }
        }
        debug!(
            "slot {slot}: button {} -> {}",
            describe(cid),
            summarize(&action)
        );
        if raw {
            raw_xy.insert(cid);
        } else {
            raw_xy.remove(&cid);
        }
        diverted.insert(cid, action);
    }
}

/// Runs the steps of a `sequence` action. Pauses block the service for
/// their duration (limited to a few seconds by the configuration check).
/// Keys held with `press` and never released are released at the end, even
/// if a step fails, so no key stays stuck.
fn run_sequence(inj: &mut dyn Injector, steps: &[Step]) -> std::io::Result<()> {
    let mut held: Vec<&'static Key> = Vec::new();
    let result = run_steps(inj, steps, &mut held);
    for k in held.iter().rev() {
        let _ = inj.key(k, false);
    }
    result
}

fn run_steps(
    inj: &mut dyn Injector,
    steps: &[Step],
    held: &mut Vec<&'static Key>,
) -> std::io::Result<()> {
    for step in steps {
        match step {
            Step::Keys(keys) => {
                keys.iter().try_for_each(|k| inj.key(k, true))?;
                keys.iter().rev().try_for_each(|k| inj.key(k, false))?;
            }
            Step::Press(keys) => {
                for k in keys {
                    inj.key(k, true)?;
                    held.push(k);
                }
            }
            Step::Release(keys) => {
                for k in keys.iter().rev() {
                    inj.key(k, false)?;
                    held.retain(|h| h != k);
                }
            }
            Step::Text(text) => inj.text(text)?,
            Step::Click(b) => {
                inj.button(*b, true)?;
                inj.button(*b, false)?;
            }
            Step::Delay(ms) => thread::sleep(Duration::from_millis(u64::from(*ms))),
        }
    }
    Ok(())
}

/// Lowers speed or DPI for slow mode.
fn set_slow(dev: &mut Device, slow: SlowMode) -> Result<(), HidppError> {
    match slow {
        SlowMode::PointerSpeed(speed) => dev.set_pointer_speed(speed),
        SlowMode::Dpi(dpi) => dev.set_dpi(dpi),
    }
}

/// Brings back the value that was in use before slow mode.
fn restore_normal(
    dev: &mut Device,
    slow: SlowMode,
    base_speed: Option<u16>,
    base_dpi: Option<u16>,
) -> Result<(), HidppError> {
    match slow {
        SlowMode::PointerSpeed(_) => dev.set_pointer_speed(base_speed.unwrap_or(256)),
        SlowMode::Dpi(_) => match base_dpi {
            Some(dpi) => dev.set_dpi(dpi),
            None => Err(HidppError::FeatureNotSupported(id::ADJUSTABLE_DPI)),
        },
    }
}

/// Logs the outcome of applying one setting.
fn report(slot: u8, what: &str, result: Result<String, HidppError>) {
    match result {
        Ok(done) => info!("slot {slot}: {what} {done}"),
        Err(HidppError::FeatureNotSupported(_)) => {
            warn!("slot {slot}: this device has no {what} setting, ignoring it")
        }
        Err(e) => warn!("slot {slot}: cannot set {what}: {e}"),
    }
}

/// Fails with "not supported" unless the device has one of `features`.
fn require(features: &FeatureTable, wanted: &[u16]) -> Result<(), HidppError> {
    if wanted.iter().any(|f| features.has(*f)) {
        Ok(())
    } else {
        Err(HidppError::FeatureNotSupported(wanted[0]))
    }
}

/// Applies the profile's device settings. Failures are logged, not fatal:
/// a profile may list settings that only some devices support.
fn apply_settings(dev: &mut Device, features: &FeatureTable, profile: &Profile, slot: u8) {
    if let Some(speed) = profile.pointer_speed {
        let r = require(features, &[id::POINTER_SPEED]).and_then(|()| dev.set_pointer_speed(speed));
        report(
            slot,
            "pointer speed",
            r.map(|()| format!("set to {:.2}", f32::from(speed) / 256.0)),
        );
    }
    if let Some(dpi) = profile.dpi {
        let r = require(features, &[id::ADJUSTABLE_DPI]).and_then(|()| {
            if let Ok(choices) = dev.dpi_choices() {
                if !choices.contains(dpi) {
                    warn!("slot {slot}: {dpi} DPI is not supported (valid: {choices})");
                }
            }
            dev.set_dpi(dpi)
        });
        report(slot, "DPI", r.map(|()| format!("set to {dpi}")));
    }
    if profile.hires_wheel.is_some() || profile.invert_wheel.is_some() {
        let r = require(features, &[id::HIRES_WHEEL])
            .and_then(|()| dev.wheel_mode())
            .and_then(|mut flags| {
                flags &= !0x01; // never divert the wheel itself
                if let Some(on) = profile.hires_wheel {
                    flags = if on { flags | 0x02 } else { flags & !0x02 };
                }
                if let Some(on) = profile.invert_wheel {
                    flags = if on { flags | 0x04 } else { flags & !0x04 };
                }
                dev.set_wheel_mode(flags).map(|()| flags)
            });
        report(
            slot,
            "wheel",
            r.map(|f| {
                format!(
                    "high-resolution {}, inverted {}",
                    on_off(f & 0x02 != 0),
                    on_off(f & 0x04 != 0)
                )
            }),
        );
    }
    if let Some(mode) = profile.smartshift {
        let r = require(features, &[id::SMART_SHIFT_ENHANCED, id::SMART_SHIFT])
            .and_then(|()| dev.set_smart_shift(mode));
        report(slot, "SmartShift", r.map(|()| format!("set to {mode}")));
    }
    if let Some(inverted) = profile.thumb_wheel_invert {
        let r = require(features, &[id::THUMB_WHEEL])
            .and_then(|()| dev.set_thumb_wheel_inverted(inverted));
        report(
            slot,
            "thumb wheel",
            r.map(|()| format!("inverted {}", on_off(inverted))),
        );
    }
    if let Some(hz) = profile.report_rate {
        let r = require(features, &[id::REPORT_RATE]).and_then(|()| {
            if let Ok(rates) = dev.report_rates() {
                if !rates.contains(&hz) {
                    warn!("slot {slot}: {hz} Hz is not supported (valid: {rates:?})");
                }
            }
            dev.set_report_rate(hz)
        });
        report(slot, "report rate", r.map(|()| format!("set to {hz} Hz")));
    }
    if let Some(special) = profile.fn_keys_special {
        let r = require(
            features,
            &[
                id::FN_INVERSION_K375S,
                id::FN_INVERSION_NEW,
                id::FN_INVERSION,
            ],
        )
        .and_then(|()| dev.set_fn_keys_special(special));
        let what = if special {
            "special functions first"
        } else {
            "F1-F12 first"
        };
        report(slot, "function keys", r.map(|()| what.to_owned()));
    }
    if let Some(mask) = profile.disable_keys {
        let r = require(features, &[id::DISABLE_KEYS]).and_then(|()| {
            let possible = dev.disableable_keys().unwrap_or(0xFF);
            if mask & !possible != 0 {
                warn!("slot {slot}: some keys in disable_keys cannot be disabled on this keyboard");
            }
            dev.set_disabled_keys(mask & possible)
                .map(|()| mask & possible)
        });
        report(slot, "disabled keys", r.map(disabled_key_names));
    }
}

/// What the settings window needs to know about a device.
fn collect_details(
    dev: &mut Device,
    features: &FeatureTable,
    controls: &[Control],
) -> DeviceDetails {
    let backlight = if features.has(id::BACKLIGHT2) {
        dev.backlight().ok()
    } else {
        None
    };
    DeviceDetails {
        firmware: dev.firmware().unwrap_or_default(),
        controls: controls
            .iter()
            .map(|c| {
                let key = match control_name(c.cid) {
                    Some((alias, _)) => alias.to_owned(),
                    None => format!("{:#06x}", c.cid),
                };
                let label = crate::hidpp::controls::name_or_number(c.cid, false);
                ControlInfo {
                    cid: c.cid,
                    key,
                    label,
                    remappable: c.is_divertable(),
                    gestures: c.supports_raw_xy(),
                }
            })
            .collect(),
        dpi_values: if features.has(id::ADJUSTABLE_DPI) {
            dev.dpi_choices().map(|c| c.values()).unwrap_or_default()
        } else {
            Vec::new()
        },
        pointer_speed: features.has(id::POINTER_SPEED),
        hires_wheel: features.has(id::HIRES_WHEEL),
        smartshift: features.has(id::SMART_SHIFT) || features.has(id::SMART_SHIFT_ENHANCED),
        thumb_wheel: features.has(id::THUMB_WHEEL),
        report_rates: if features.has(id::REPORT_RATE) {
            dev.report_rates().unwrap_or_default()
        } else {
            Vec::new()
        },
        fn_keys: [
            id::FN_INVERSION,
            id::FN_INVERSION_NEW,
            id::FN_INVERSION_K375S,
        ]
        .iter()
        .any(|f| features.has(*f)),
        disable_keys: if features.has(id::DISABLE_KEYS) {
            dev.disableable_keys().unwrap_or(0)
        } else {
            0
        },
        backlight_levels: backlight.map(|_| dev.backlight_levels().unwrap_or(1).max(1)),
        backlight_auto: backlight.is_some_and(|b| b.supports_auto()),
        tech: None,
    }
}

/// Reads the keyboard backlight and applies the profile's `backlight`
/// setting; returns the resulting state and the number of levels.
fn apply_backlight(dev: &mut Device, profile: &Profile, slot: u8) -> Option<(BacklightState, u8)> {
    let mut state = match dev.backlight() {
        Ok(s) => s,
        Err(e) => {
            debug!("slot {slot}: cannot read the backlight: {e}");
            return None;
        }
    };
    let levels = dev.backlight_levels().unwrap_or(1).max(1);
    let Some((mode, level)) = profile.backlight else {
        return Some((state, levels));
    };
    match mode {
        BacklightMode::Off => state.enabled = false,
        BacklightMode::On => {
            state.enabled = true;
            state.set_mode(BacklightState::MODE_DEFAULT);
        }
        BacklightMode::Auto => {
            if !state.supports_auto() {
                warn!("slot {slot}: this keyboard has no light sensor, ignoring backlight = auto");
                return Some((state, levels));
            }
            state.enabled = true;
            state.set_mode(BacklightState::MODE_AUTO);
        }
        BacklightMode::Manual => {
            state.enabled = true;
            state.set_mode(BacklightState::MODE_MANUAL);
            state.level = level.unwrap_or(0).min(levels - 1);
        }
    }
    report(
        slot,
        "backlight",
        dev.set_backlight(&state)
            .map(|()| format!("set to {state}")),
    );
    Some((state, levels))
}

/// `0x09` → `"capslock, insert"`.
fn disabled_key_names(mask: u8) -> String {
    let names: Vec<&str> = DISABLE_KEY_NAMES
        .iter()
        .filter(|(_, b)| mask & b != 0)
        .map(|(n, _)| *n)
        .collect();
    if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    }
}

/// Starts a program without waiting for it (a helper thread reaps it).
fn spawn(program: &str, args: &[String]) -> std::io::Result<()> {
    let mut child = Command::new(program).args(args).spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// `"back (0x0053)"` or `"0x00A1"`.
pub fn describe(cid: u16) -> String {
    match control_name(cid) {
        Some((alias, _)) => format!("{alias} ({cid:#06x})"),
        None => format!("{cid:#06x}"),
    }
}

/// Short description of an action for the log.
fn summarize(action: &Action) -> String {
    match action {
        Action::Keys(keys) => keys.iter().map(|k| k.name).collect::<Vec<_>>().join("+"),
        Action::Click(b) => format!("{b:?} click").to_lowercase(),
        Action::Command { program, .. } => format!("run \"{program}\""),
        Action::Toggle(SlowMode::PointerSpeed(s)) => {
            format!("toggle speed {:.2}", f32::from(*s) / 256.0)
        }
        Action::Toggle(SlowMode::Dpi(d)) => format!("toggle {d} DPI"),
        Action::CycleDpi(v) => format!("cycle DPI {v:?}"),
        Action::Scroll {
            vertical,
            horizontal,
        } => format!("scroll ({vertical}, {horizontal})"),
        Action::ChangeHost { target, all } => {
            let who = if *all { "all devices" } else { "device" };
            match target {
                HostTarget::Next => format!("{who} to next computer"),
                HostTarget::Index(i) => format!("{who} to computer {}", i + 1),
            }
        }
        Action::Sequence(m) => match &m.name {
            Some(n) => format!("macro \"{n}\""),
            None => format!("sequence of {} steps", m.steps.len()),
        },
        Action::Brightness(crate::config::BrightnessChange::By(p)) => format!("brightness {p:+}%"),
        Action::Brightness(crate::config::BrightnessChange::To(p)) => format!("brightness {p}%"),
        Action::Backlight(c) => format!("backlight {c:?}").to_lowercase(),
        Action::Gesture(_) => "gesture".into(),
        Action::Disable => "nothing".into(),
    }
}

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_disabled_keys() {
        assert_eq!(disabled_key_names(0), "none");
        assert_eq!(disabled_key_names(0x09), "capslock, insert");
    }

    #[test]
    fn summarizes_actions() {
        assert_eq!(
            summarize(&Action::ChangeHost {
                target: HostTarget::Index(1),
                all: true
            }),
            "all devices to computer 2"
        );
        assert_eq!(
            summarize(&Action::Scroll {
                vertical: 0,
                horizontal: -3
            }),
            "scroll (0, -3)"
        );
    }
}
