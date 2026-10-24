//! Finding receivers/devices and exchanging HID++ frames with them.
//!
//! # How the hardware shows up
//!
//! A Logitech receiver exposes several HID interfaces. Only the vendor
//! specific one (usage page `0xFF00`) carries HID++; it contains two
//! *collections*: usage `1` for short reports (`0x10`) and usage `2` for long
//! reports (`0x11`). Bluetooth devices use usage page `0xFF43` instead.
//!
//! * **Linux** (hidraw): both collections live behind one device node, so a
//!   single path reads and writes every report size.
//! * **Windows**: each collection is a separate device path and only accepts
//!   its own report id. We therefore open every collection of one physical
//!   device, route each write to the right one and merge everything we read
//!   into one stream.
//!
//! [`group_devices`] turns the flat list of collections into one entry per
//! physical receiver or device, so several receivers can be served at once.
//!
//! # Threading model
//!
//! One background thread per path blocks on `read` and forwards every frame
//! to the transport's private channel, then calls the *waker* so the owner's
//! event loop knows something arrived (no polling). The owner consumes the
//! channel: frames that answer a pending request are returned to the caller,
//! all other frames (notifications) are queued and handed out by
//! [`Transport::try_next`]. Writing uses a *second* handle to the same path,
//! because a HID handle cannot be shared between threads.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use hidapi::{BusType, HidApi, HidDevice};
use log::{debug, trace, warn};

use super::frame::{Frame, HidppError, RECEIVER_INDEX, REPORT_LONG, REPORT_SHORT};
use crate::pcap::{Direction, PcapWriter};

/// Logitech's USB vendor id.
pub const LOGITECH_VID: u16 = 0x046D;

/// Known receivers (product id → marketing name).
pub const RECEIVERS: &[(u16, &str)] = &[
    (0xC548, "Bolt"),
    (0xC52B, "Unifying"),
    (0xC532, "Unifying"),
    (0xC52F, "Nano"),
    (0xC534, "Nano"),
    (0xC539, "Lightspeed"),
    (0xC53A, "Lightspeed"),
    (0xC53F, "Lightspeed"),
    (0xC541, "Lightspeed"),
    (0xC545, "Lightspeed"),
    (0xC547, "Lightspeed"),
];

/// How long a request waits for its answer before giving up.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1500);
/// Read timeout of the reader threads. Frames are delivered as soon as they
/// arrive; this only bounds how long closing a transport may take.
const READ_POLL_MS: i32 = 1000;

/// Called by reader threads whenever they queued something.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Marketing name of a receiver product id, if it is one.
pub fn receiver_name(pid: u16) -> Option<&'static str> {
    RECEIVERS.iter().find(|(p, _)| *p == pid).map(|(_, n)| *n)
}

/// One HID++ capable collection found on the system.
#[derive(Debug, Clone)]
pub struct HidInterface {
    /// Operating-system path used to open the collection.
    pub path: CString,
    /// USB product id.
    pub pid: u16,
    /// HID usage page (`0xFF00` / `0xFF43` for HID++).
    pub usage_page: u16,
    /// HID usage within the page (1 = short reports, 2 = long reports).
    pub usage: u16,
    /// USB interface number (-1 if unknown).
    pub interface: i32,
    /// Product name reported by the device.
    pub product: String,
    /// Connected over Bluetooth.
    pub bluetooth: bool,
}

impl HidInterface {
    /// The product id is a known receiver.
    pub fn is_receiver(&self) -> bool {
        receiver_name(self.pid).is_some()
    }

    /// Report ids this collection can carry. Unknown usage (some platforms do
    /// not report it) means "try everything".
    fn report_ids(&self) -> &'static [u8] {
        match (self.usage_page, self.usage) {
            (0xFF00, 1) => &[REPORT_SHORT],
            (0xFF00, 2) => &[REPORT_LONG],
            _ => &[REPORT_SHORT, REPORT_LONG],
        }
    }

    /// Identifies the physical device this collection belongs to.
    pub fn device_key(&self) -> String {
        device_key(&self.path.to_string_lossy())
    }
}

/// Normalises a HID path so that all collections of one physical device get
/// the same key.
///
/// A Windows path looks like
/// `\\?\HID#VID_046D&PID_C548&MI_02&Col01#8&2d1f3e4a&0&0000#{guid}`: the
/// collection number appears both as `&Col01` and as the last `&0000` of the
/// instance id, so both are removed. Linux paths are already one per device.
fn device_key(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    let mut parts: Vec<String> = lower.split('#').map(str::to_owned).collect();
    if parts.len() >= 3 {
        if let Some(pos) = parts[1].find("&col") {
            let end = parts[1][pos + 1..]
                .find('&')
                .map_or(parts[1].len(), |e| pos + 1 + e);
            parts[1].replace_range(pos..end, "");
        }
        if let Some(pos) = parts[2].rfind('&') {
            parts[2].truncate(pos);
        }
    }
    parts.join("#")
}

/// Lists every Logitech HID++ collection (receivers and direct devices).
///
/// On Linux the kernel also publishes one hidraw node per device paired to a
/// receiver (driver `hid-logitech-dj`). Those virtual nodes are skipped: the
/// devices are already reached through their receiver. They are recognisable
/// as USB devices whose product id is a *wireless* id (cabled Logitech
/// devices and receivers use ids `0xC0xx`–`0xCFxx`).
pub fn enumerate(api: &HidApi) -> Vec<HidInterface> {
    let mut out: Vec<HidInterface> = api
        .device_list()
        .filter(|d| d.vendor_id() == LOGITECH_VID)
        .filter(|d| {
            let (page, usage) = (d.usage_page(), d.usage());
            (page == 0xFF00 && (usage == 1 || usage == 2)) || page == 0xFF43
        })
        .filter(|d| {
            let wired_id = (0xC000..=0xCFFF).contains(&d.product_id());
            !matches!(d.bus_type(), BusType::Usb) || wired_id
        })
        .map(|d| HidInterface {
            path: d.path().to_owned(),
            pid: d.product_id(),
            usage_page: d.usage_page(),
            usage: d.usage(),
            interface: d.interface_number(),
            product: d.product_string().unwrap_or_default().to_owned(),
            bluetooth: matches!(d.bus_type(), BusType::Bluetooth),
        })
        .collect();
    // Receivers first, then by product id: deterministic ordering.
    out.sort_by_key(|i| (!i.is_receiver(), i.pid, i.usage));
    out
}

/// Groups collections by physical device: `(device key, its collections)`.
pub fn group_devices(interfaces: &[HidInterface]) -> Vec<(String, Vec<HidInterface>)> {
    let mut groups: Vec<(String, Vec<HidInterface>)> = Vec::new();
    for itf in interfaces {
        let key = itf.device_key();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(itf.clone()),
            None => groups.push((key, vec![itf.clone()])),
        }
    }
    groups
}

/// Something a reader thread reports.
#[derive(Debug)]
pub enum Incoming {
    /// A HID++ report arrived.
    Frame(Frame),
    /// The receiver/device disappeared (unplugged, driver reset...).
    Lost(String),
}

/// An open HID++ channel to one receiver (or one directly connected device).
pub struct Transport {
    /// Display name, e.g. "Bolt receiver (046d:c548)".
    pub name: String,
    /// USB product id of the receiver or device.
    pub pid: u16,
    /// A receiver (devices are reached through slots 1–6).
    pub is_receiver: bool,
    /// Software id stamped on our requests (see [`super::frame::SOFTWARE_ID`]).
    swid: u8,
    /// One write handle per opened path.
    writers: Vec<HidDevice>,
    /// Report id -> index into `writers`.
    routes: BTreeMap<u8, usize>,
    rx: Receiver<Incoming>,
    pending: VecDeque<Incoming>,
    stop: Arc<AtomicBool>,
    readers: Vec<JoinHandle<()>>,
    pcap: Option<Arc<Mutex<PcapWriter>>>,
    /// How long a request waits for its answer.
    pub timeout: Duration,
}

impl Transport {
    /// Opens all HID++ collections of one physical device (one entry of
    /// [`group_devices`]).
    ///
    /// * `swid` — software id for our requests; two programs talking to the
    ///   same receiver must use different values.
    /// * `waker` — called from reader threads after queueing a frame.
    pub fn open(
        api: &HidApi,
        collections: &[HidInterface],
        swid: u8,
        pcap: Option<Arc<Mutex<PcapWriter>>>,
        waker: Option<Waker>,
    ) -> Result<Self, HidppError> {
        let first = collections
            .first()
            .ok_or_else(|| HidppError::Io("no HID++ collection to open".into()))?;

        // Group collections by path (on Linux they all share one path).
        let mut paths: BTreeMap<CString, Vec<u8>> = BTreeMap::new();
        for itf in collections {
            let ids = paths.entry(itf.path.clone()).or_default();
            for id in itf.report_ids() {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
        }

        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut writers = Vec::new();
        let mut routes = BTreeMap::new();
        let mut readers = Vec::new();

        for (path, ids) in &paths {
            let open = |what| {
                api.open_path(path)
                    .map_err(|e| HidppError::Io(format!("cannot open {what} {path:?}: {e}")))
            };
            writers.push(open("for writing")?);
            for id in ids {
                routes.entry(*id).or_insert(writers.len() - 1);
            }
            let reader = open("for reading")?;
            readers.push(spawn_reader(
                reader,
                tx.clone(),
                Arc::clone(&stop),
                pcap.clone(),
                waker.clone(),
            ));
            debug!("opened {path:?} for report ids {ids:02X?}");
        }

        let pid = first.pid;
        let is_receiver = receiver_name(pid).is_some();
        let name = match receiver_name(pid) {
            Some(kind) => format!("{kind} receiver ({LOGITECH_VID:04x}:{pid:04x})"),
            None => {
                let link = if first.bluetooth { "Bluetooth" } else { "USB" };
                format!(
                    "{} via {link} ({LOGITECH_VID:04x}:{pid:04x})",
                    first.product
                )
            }
        };
        Ok(Self {
            name,
            pid,
            is_receiver,
            swid: swid & 0x0F,
            writers,
            routes,
            rx,
            pending: VecDeque::new(),
            stop,
            readers,
            pcap,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Whether we can send reports with this id.
    pub fn supports(&self, report_id: u8) -> bool {
        self.routes.contains_key(&report_id)
    }

    /// Sends one frame without waiting for an answer.
    pub fn send(&self, frame: &Frame) -> Result<(), HidppError> {
        let writer = self
            .routes
            .get(&frame.report_id())
            .map(|&i| &self.writers[i])
            .ok_or_else(|| {
                HidppError::Io(format!("report {:#04x} not supported", frame.report_id()))
            })?;
        trace!("-> {frame:?}");
        if let Some(p) = &self.pcap {
            if let Ok(mut p) = p.lock() {
                let _ = p.write(Direction::Out, frame.device_index(), frame.as_bytes());
            }
        }
        writer
            .write(frame.as_bytes())
            .map_err(|e| HidppError::Io(e.to_string()))?;
        Ok(())
    }

    /// Sends `frame` and waits for the matching answer.
    ///
    /// The answer matches when it comes from the same device index and
    /// echoes bytes 2 and 3. Every unrelated frame received meanwhile is kept
    /// for [`Transport::try_next`], so no notification is ever lost.
    pub fn request(&mut self, frame: &Frame) -> Result<Frame, HidppError> {
        let (index, b2, b3) = (frame.device_index(), frame.b2(), frame.b3());
        self.send(frame)?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(HidppError::Timeout);
            }
            match self.rx.recv_timeout(left) {
                Ok(Incoming::Frame(f)) if f.device_index() == index => {
                    if let Some(err) = f.error_for(b2, b3) {
                        return Err(err);
                    }
                    if f.b2() == b2 && f.b3() == b3 {
                        return Ok(f);
                    }
                    self.pending.push_back(Incoming::Frame(f));
                }
                Ok(other) => {
                    let lost = matches!(other, Incoming::Lost(_));
                    self.pending.push_back(other);
                    if lost {
                        return Err(HidppError::Io("device disconnected".into()));
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(HidppError::Timeout),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(HidppError::Io("all reader threads stopped".into()))
                }
            }
        }
    }

    /// Next queued notification, without waiting.
    pub fn try_next(&mut self) -> Option<Incoming> {
        if let Some(ev) = self.pending.pop_front() {
            return Some(ev);
        }
        match self.rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Incoming::Lost("reader threads stopped".into()))
            }
        }
    }

    /// Next notification, waiting up to `wait` (used by the CLI).
    pub fn next_event(&mut self, wait: Duration) -> Option<Incoming> {
        if let Some(ev) = self.pending.pop_front() {
            return Some(ev);
        }
        match self.rx.recv_timeout(wait) {
            Ok(ev) => Some(ev),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                Some(Incoming::Lost("reader threads stopped".into()))
            }
        }
    }

    /// Whether notifications were set aside by a request and are waiting.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    // ----------------------------------------------------------------------
    // HID++ 2.0 (features of a device)
    // ----------------------------------------------------------------------

    /// Calls `function` of the feature at `feature_index` on device `index`
    /// and returns the answer's parameter bytes.
    pub fn call(
        &mut self,
        index: u8,
        feature_index: u8,
        function: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, HidppError> {
        // Long reports work for every HID++ 2.0 call; short ones only when
        // the parameters fit in 3 bytes and the long collection is missing.
        let report = if params.len() <= 3 && !self.supports(REPORT_LONG) {
            REPORT_SHORT
        } else {
            REPORT_LONG
        };
        let mut payload = vec![feature_index, (function << 4) | self.swid];
        payload.extend_from_slice(params);
        let answer = self.request(&Frame::new(report, index, &payload))?;
        Ok(answer.params().to_vec())
    }

    /// Root feature, function 1 ("ping"): returns the HID++ protocol version
    /// as `(major, minor)` or `None` if nothing answers at that index.
    pub fn ping(&mut self, index: u8) -> Option<(u8, u8)> {
        const MARK: u8 = 0x5A;
        let p = self.call(index, 0x00, 1, &[0, 0, MARK]).ok()?;
        (p.len() >= 3 && p[2] == MARK).then(|| (p[0], p[1]))
    }

    // ----------------------------------------------------------------------
    // HID++ 1.0 (registers of the receiver)
    // ----------------------------------------------------------------------

    /// Reads a short receiver register.
    pub fn read_register(&mut self, address: u8, params: &[u8]) -> Result<Vec<u8>, HidppError> {
        self.register(0x81, address, params)
    }

    /// Writes a short receiver register.
    pub fn write_register(&mut self, address: u8, params: &[u8]) -> Result<Vec<u8>, HidppError> {
        self.register(0x80, address, params)
    }

    /// Reads a long receiver register (the answer is a 16-byte long report).
    pub fn read_long_register(
        &mut self,
        address: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, HidppError> {
        self.register(0x83, address, params)
    }

    /// Writes a long receiver register (parameters up to 16 bytes, sent in
    /// a long report). Used by Bolt pairing.
    pub fn write_long_register(
        &mut self,
        address: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, HidppError> {
        let mut payload = vec![0x82, address];
        payload.extend_from_slice(params);
        let answer = self.request(&Frame::new(REPORT_LONG, RECEIVER_INDEX, &payload))?;
        Ok(answer.params().to_vec())
    }

    fn register(&mut self, sub_id: u8, address: u8, params: &[u8]) -> Result<Vec<u8>, HidppError> {
        let mut payload = vec![sub_id, address];
        payload.extend_from_slice(params);
        let answer = self.request(&Frame::new(REPORT_SHORT, RECEIVER_INDEX, &payload))?;
        Ok(answer.params().to_vec())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for r in self.readers.drain(..) {
            let _ = r.join();
        }
    }
}

fn spawn_reader(
    dev: HidDevice,
    tx: Sender<Incoming>,
    stop: Arc<AtomicBool>,
    pcap: Option<Arc<Mutex<PcapWriter>>>,
    waker: Option<Waker>,
) -> JoinHandle<()> {
    let wake = move || {
        if let Some(w) = &waker {
            w();
        }
    };
    thread::Builder::new()
        .name("hid-reader".into())
        .spawn(move || {
            let mut buf = [0u8; 64];
            while !stop.load(Ordering::Relaxed) {
                match dev.read_timeout(&mut buf, READ_POLL_MS) {
                    Ok(0) => {}
                    Ok(n) => {
                        let Some(frame) = Frame::from_raw(&buf[..n]) else {
                            continue;
                        };
                        trace!("<- {frame:?}");
                        if let Some(p) = &pcap {
                            if let Ok(mut p) = p.lock() {
                                let _ =
                                    p.write(Direction::In, frame.device_index(), frame.as_bytes());
                            }
                        }
                        if tx.send(Incoming::Frame(frame)).is_err() {
                            break; // transport dropped
                        }
                        wake();
                    }
                    Err(e) => {
                        if !stop.load(Ordering::Relaxed) {
                            warn!("HID read failed: {e}");
                            let _ = tx.send(Incoming::Lost(e.to_string()));
                            wake();
                        }
                        break;
                    }
                }
            }
        })
        .expect("failed to spawn HID reader thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_collections_share_a_key() {
        let col1 = r"\\?\HID#VID_046D&PID_C548&MI_02&Col01#8&2d1f3e4a&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let col2 = r"\\?\HID#VID_046D&PID_C548&MI_02&Col02#8&2d1f3e4a&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let other = r"\\?\HID#VID_046D&PID_C548&MI_02&Col01#8&99999999&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(device_key(col1), device_key(col2));
        assert_ne!(device_key(col1), device_key(other));
    }

    #[test]
    fn linux_paths_are_kept() {
        assert_eq!(device_key("/dev/hidraw3"), "/dev/hidraw3");
        assert_ne!(device_key("/dev/hidraw3"), device_key("/dev/hidraw4"));
    }
}
