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

use crate::config::{self, Action, Config, Profile, SlowMode};
use crate::hidpp::features::{control_name, id, Device, DeviceKind, FeatureTable};
use crate::hidpp::frame::{Frame, HidppError, RECEIVER_INDEX, REPORT_SHORT, SOFTWARE_ID};
use crate::hidpp::receiver::{paired_devices, PairedDevice};
use crate::hidpp::transport::{self, Incoming, Transport, Waker};
use crate::input::{self, Injector};
use crate::pcap::PcapWriter;
use crate::status::{DeviceStatus, LinkStatus, Snapshot, UiHooks, UiUpdate};

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

/// Something the event loop must react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A reader thread queued a HID++ frame or a disconnection.
    Hid,
    /// Look for receivers/devices (e.g. the OS reported a new device).
    Rescan,
    /// Re-read the configuration file now.
    Reload,
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
    };
    daemon.rescan();
    daemon.publish();
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
}

/// What a [`Link`] needs from the daemon while handling events.
struct Ctx<'a> {
    cfg: &'a Config,
    injector: Option<&'a mut (dyn Injector + 'static)>,
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
            .flat_map(|l| l.pending.values().map(|p| p.due))
            .chain(std::iter::once(self.next_housekeeping))
            .chain(self.rescan_due)
            .min()
            .unwrap_or(self.next_housekeeping)
    }

    /// Handles every queued frame and every due setup, on every link.
    fn pump(&mut self) {
        let mut ctx = Ctx {
            cfg: &self.cfg,
            injector: self.injector.as_deref_mut(),
        };
        for link in &mut self.links {
            link.pump(&mut ctx);
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
        let mut ctx = Ctx {
            cfg: &self.cfg,
            injector: self.injector.as_deref_mut(),
        };
        for link in &mut self.links {
            link.reset_all(&mut ctx);
        }
        self.cfg = new;
        self.dirty = true;
        info!("configuration reloaded; re-applying profiles (log file and capture settings need a restart)");
    }

    /// Sends a snapshot to the interface if anything changed.
    fn publish(&mut self) {
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
        let mut ctx = Ctx {
            cfg: &self.cfg,
            injector: self.injector.as_deref_mut(),
        };
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
    battery: Option<String>,
    /// Name of the applied profile, if one matched.
    profile: Option<String>,
    features: FeatureTable,
    /// Index of the ReprogControls feature (diverted button events use it).
    reprog_index: Option<u8>,
    /// Index of WirelessDeviceStatus (direct connections announce reconnects).
    status_index: Option<u8>,
    /// Buttons currently diverted to us and their actions.
    diverted: BTreeMap<u16, Action>,
    /// Buttons reported as held down in the last event.
    pressed: Vec<u16>,
    /// Pointer speed / DPI to return to when slow mode is switched off.
    base_speed: Option<u16>,
    base_dpi: Option<u16>,
    /// The slow setting currently active, if slow mode is on.
    slow: Option<SlowMode>,
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
    /// Set when the reader threads report the device is gone.
    lost: Option<String>,
    /// Something the interface shows changed.
    changed: bool,
}

impl Link {
    fn new(key: String, t: Transport) -> Self {
        Self {
            key,
            t,
            sessions: BTreeMap::new(),
            pending: BTreeMap::new(),
            paired: Vec::new(),
            lost: None,
            changed: true,
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
                battery: s.battery.clone(),
                dpi: s.base_dpi,
                profile: s.profile.clone(),
                remapped: s.diverted.len(),
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
                    dpi: None,
                    remapped: 0,
                });
            }
        }
        devices.sort_by_key(|d| d.slot);
        LinkStatus {
            name: self.t.name.clone(),
            is_receiver: self.t.is_receiver,
            devices,
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
            // Setups and actions may have set notifications aside.
            if !self.t.has_pending() {
                return;
            }
        }
    }

    fn on_frame(&mut self, ctx: &mut Ctx, f: &Frame) {
        let slot = f.device_index();

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
        if Some(f.b2()) == session.reprog_index && f.function() == 0 {
            self.on_buttons(ctx, slot, f.params());
        } else if Some(f.b2()) == session.status_index && f.function() == 0 && f.params()[0] == 1 {
            // WirelessDeviceStatus "reconnection": the device lost its settings.
            info!("slot {slot}: device reports a reconnection, re-applying profile");
            self.schedule(slot, SETUP_BACKOFF[0]);
        }
    }

    /// Handles a "diverted buttons" event: the list of buttons held now.
    fn on_buttons(&mut self, ctx: &mut Ctx, slot: u8, params: &[u8]) {
        let now: Vec<u16> = params
            .chunks_exact(2)
            .take(4)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
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
            self.perform(ctx, slot, &action, false);
        }
        for (cid, action) in pressed {
            info!(
                "slot {slot}: button {} pressed -> {}",
                describe(cid),
                summarize(&action)
            );
            self.perform(ctx, slot, &action, true);
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
            Action::Command { program, args } if down => spawn(program, args),
            Action::Toggle(slow) if down => {
                self.toggle_slow_mode(slot, *slow);
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
            }
            Err(e) => warn!("slot {slot}: cannot switch slow mode: {e}"),
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
            match self.setup(ctx.cfg, slot) {
                Ok(session) => {
                    self.pending.remove(&slot);
                    self.sessions.insert(slot, session);
                    self.changed = true;
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
    fn setup(&mut self, cfg: &Config, slot: u8) -> Result<Session, HidppError> {
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
        let battery_level = dev.battery().ok();
        let battery = battery_level
            .as_ref()
            .map(|b| format!(", battery {b}"))
            .unwrap_or_default();

        let mut session = Session {
            reprog_index: features.index_of(id::REPROG_CONTROLS_V4),
            status_index: features.index_of(id::WIRELESS_DEVICE_STATUS),
            name: name.clone(),
            kind,
            battery: battery_level,
            profile: None,
            features: features.clone(),
            diverted: BTreeMap::new(),
            pressed: Vec::new(),
            base_speed: None,
            base_dpi: None,
            slow: None,
        };

        let Some(profile) = cfg.profile_for(&name, kind) else {
            info!("slot {slot}: \"{name}\" ({kind}) connected{battery}; no profile matches, leaving it unchanged");
            return Ok(session);
        };
        info!(
            "slot {slot}: \"{name}\" ({kind}) connected{battery}; applying profile \"{}\"",
            profile.name
        );
        session.profile = Some(profile.name.clone());
        apply_settings(&mut dev, &features, profile, slot);
        if features.has(id::POINTER_SPEED) {
            session.base_speed = profile.pointer_speed.or_else(|| dev.pointer_speed().ok());
        }
        if features.has(id::ADJUSTABLE_DPI) {
            session.base_dpi = profile.dpi.or_else(|| dev.dpi().ok());
        }
        session.diverted = divert_buttons(&mut dev, &features, profile, slot)?;
        Ok(session)
    }

    /// Gives every diverted button back to the firmware and undoes slow mode.
    fn restore_all(&mut self, ctx: &mut Ctx) {
        self.release_all(ctx);
        for (slot, s) in &self.sessions {
            let mut dev = Device::new(&mut self.t, *slot, &s.features);
            for cid in s.diverted.keys() {
                if let Err(e) = dev.set_diverted(*cid, false) {
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

    /// Restores every device, then configures them again (new configuration).
    fn reset_all(&mut self, ctx: &mut Ctx) {
        self.restore_all(ctx);
        let slots: Vec<u8> = std::mem::take(&mut self.sessions).into_keys().collect();
        for slot in slots {
            self.schedule(slot, Duration::ZERO);
        }
    }

    /// Releases any key/click still held (avoids "stuck" keys).
    fn release_all(&mut self, ctx: &mut Ctx) {
        for s in self.sessions.values_mut() {
            release_session(ctx, s);
            s.pressed.clear();
        }
    }
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

/// Applies speed, DPI and wheel settings. Failures are logged, not fatal:
/// a profile may list settings that only some devices support.
fn apply_settings(dev: &mut Device, features: &FeatureTable, profile: &Profile, slot: u8) {
    if let Some(speed) = profile.pointer_speed {
        if features.has(id::POINTER_SPEED) {
            match dev.set_pointer_speed(speed) {
                Ok(()) => info!(
                    "slot {slot}: pointer speed set to {:.2}",
                    f32::from(speed) / 256.0
                ),
                Err(e) => warn!("slot {slot}: cannot set pointer speed: {e}"),
            }
        } else {
            warn!("slot {slot}: device has no adjustable pointer speed, ignoring pointer_speed");
        }
    }
    if let Some(dpi) = profile.dpi {
        if features.has(id::ADJUSTABLE_DPI) {
            if let Ok(choices) = dev.dpi_choices() {
                if !choices.contains(dpi) {
                    warn!("slot {slot}: {dpi} DPI is not supported (valid: {choices})");
                }
            }
            match dev.set_dpi(dpi) {
                Ok(()) => info!("slot {slot}: DPI set to {dpi}"),
                Err(e) => warn!("slot {slot}: cannot set DPI: {e}"),
            }
        } else {
            warn!("slot {slot}: device has no adjustable DPI, ignoring dpi");
        }
    }
    if profile.hires_wheel.is_some() || profile.invert_wheel.is_some() {
        if !features.has(id::HIRES_WHEEL) {
            warn!("slot {slot}: device has no configurable wheel, ignoring wheel settings");
            return;
        }
        let result = dev.wheel_mode().and_then(|mut flags| {
            flags &= !0x01; // never divert the wheel itself
            if let Some(on) = profile.hires_wheel {
                flags = if on { flags | 0x02 } else { flags & !0x02 };
            }
            if let Some(on) = profile.invert_wheel {
                flags = if on { flags | 0x04 } else { flags & !0x04 };
            }
            dev.set_wheel_mode(flags).map(|()| flags)
        });
        match result {
            Ok(flags) => info!(
                "slot {slot}: wheel high-resolution {}, inverted {}",
                on_off(flags & 0x02 != 0),
                on_off(flags & 0x04 != 0)
            ),
            Err(e) => warn!("slot {slot}: cannot set wheel mode: {e}"),
        }
    }
}

/// Diverts the profile's buttons; returns the ones actually diverted.
fn divert_buttons(
    dev: &mut Device,
    features: &FeatureTable,
    profile: &Profile,
    slot: u8,
) -> Result<BTreeMap<u16, Action>, HidppError> {
    let mut diverted = BTreeMap::new();
    if profile.buttons.is_empty() {
        return Ok(diverted);
    }
    if !features.has(id::REPROG_CONTROLS_V4) {
        warn!("slot {slot}: device has no reprogrammable buttons, ignoring the buttons section");
        return Ok(diverted);
    }
    let controls = dev.controls()?;
    for (cid, action) in &profile.buttons {
        match controls.iter().find(|c| c.cid == *cid) {
            None => warn!("slot {slot}: this device has no button {}", describe(*cid)),
            Some(c) if !c.is_divertable() => {
                warn!("slot {slot}: button {} cannot be remapped", describe(*cid))
            }
            Some(_) => match dev.set_diverted(*cid, true) {
                Ok(()) => {
                    debug!(
                        "slot {slot}: button {} -> {}",
                        describe(*cid),
                        summarize(action)
                    );
                    diverted.insert(*cid, action.clone());
                }
                Err(e) => warn!("slot {slot}: cannot remap button {}: {e}", describe(*cid)),
            },
        }
    }
    info!("slot {slot}: {} button(s) remapped", diverted.len());
    Ok(diverted)
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

fn summarize(action: &Action) -> String {
    match action {
        Action::Keys(keys) => keys.iter().map(|k| k.name).collect::<Vec<_>>().join("+"),
        Action::Click(b) => format!("{b:?} click").to_lowercase(),
        Action::Command { program, .. } => format!("run \"{program}\""),
        Action::Toggle(SlowMode::PointerSpeed(s)) => {
            format!("toggle speed {:.2}", f32::from(*s) / 256.0)
        }
        Action::Toggle(SlowMode::Dpi(d)) => format!("toggle {d} DPI"),
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
