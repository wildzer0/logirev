//! How the settings window talks to the running service.
//!
//! The service (tray icon / background process) owns the devices; the
//! settings window is a separate process that is started only while the user
//! needs it, so the service itself stays tiny. They talk over a local TCP
//! connection:
//!
//! * the service listens on `127.0.0.1` (never reachable from the network)
//!   on a port chosen by the system, and writes the port and a random secret
//!   into a *discovery file* in the user's private application folder;
//! * the window reads that file, connects and first sends the secret, so no
//!   other program can drive the service;
//! * then both sides exchange JSON messages, one per line: the service sends
//!   a [`Snapshot`] every time something changes, the window sends commands
//!   ([`ToService`]).

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, warn};
use serde::{Deserialize, Serialize};

use crate::daemon::Event;
use crate::status::{Snapshot, UiUpdate};

/// Messages from the service to the window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToClient {
    /// The current state of every receiver and device.
    Snapshot {
        /// The state.
        snapshot: Snapshot,
    },
    /// Sent every few seconds so both sides notice a dead connection.
    Ping,
}

/// Messages from the window to the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ToService {
    /// First message: proves the window may talk to this service.
    Auth {
        /// The secret from the discovery file.
        token: String,
    },
    /// Re-read the configuration file now.
    Reload,
    /// Look for receivers again.
    Rescan,
    /// Send a device to another computer (Easy-Switch).
    ChangeHost {
        /// Receiver, as `LinkStatus::key`.
        link: String,
        /// Receiver slot of the device.
        slot: u8,
        /// Target computer, counted from 0.
        host: u8,
    },
    /// Put a receiver in pairing mode.
    StartPairing {
        /// Receiver, as `LinkStatus::key`.
        link: String,
    },
    /// Bolt: pair the `index`-th device found.
    ChoosePairing {
        /// Receiver, as `LinkStatus::key`.
        link: String,
        /// Position in the list of devices found.
        index: usize,
    },
    /// Stop pairing / forget a finished session.
    StopPairing {
        /// Receiver, as `LinkStatus::key`.
        link: String,
    },
    /// Remove a device from a receiver.
    Unpair {
        /// Receiver, as `LinkStatus::key`.
        link: String,
        /// Receiver slot of the device to remove.
        slot: u8,
    },
    /// Stop the service.
    Quit,
}

/// Where the service publishes its port and secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Discovery {
    port: u16,
    token: String,
    pid: u32,
}

/// The per-user folder for runtime files (`%LOCALAPPDATA%\logirev` on
/// Windows, `$XDG_RUNTIME_DIR/logirev` on Linux).
pub fn runtime_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("logirev")
    }
    #[cfg(target_os = "linux")]
    {
        // Without a per-session runtime directory (cron, some SSH sessions)
        // fall back to a folder that only this user can enter, never a
        // shared /tmp/logirev.
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) => PathBuf::from(dir).join("logirev"),
            None => std::env::temp_dir().join(format!("logirev-{}", unsafe { libc::getuid() })),
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("logirev")
    }
}

/// Creates the runtime directory, private to this user (0700 on Linux).
fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} is not a private directory", path.display()),
                ));
            }
            if meta.uid() != unsafe { libc::getuid() } {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} belongs to another user", path.display()),
                ));
            }
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::fs::create_dir_all(path)
    }
}

/// Writes the discovery file, readable only by this user (0600 on Linux).
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        file.write_all(data)
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::fs::write(path, data)
    }
}

/// Most settings windows (and probes) served at once.
const MAX_CLIENTS: usize = 8;

fn discovery_path() -> PathBuf {
    runtime_dir().join("service.json")
}

/// A random secret from the operating system, or a hasher/time fallback.
fn random_token() -> String {
    let mut bytes = [0u8; 16];
    if os_random(&mut bytes) {
        return bytes.iter().map(|b| format!("{b:02x}")).collect();
    }
    let part = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        h.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

#[cfg(target_os = "linux")]
fn os_random(buf: &mut [u8]) -> bool {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .is_ok()
}

#[cfg(windows)]
fn os_random(buf: &mut [u8]) -> bool {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    // SAFETY: `buf` is valid for `buf.len()` bytes and the flags are the
    // documented system RNG selector.
    unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        ) >= 0
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn os_random(_buf: &mut [u8]) -> bool {
    false
}

/// Compares two secrets without stopping at the first different byte.
fn tokens_equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ---------------------------------------------------------------------------
// Service side
// ---------------------------------------------------------------------------

/// Latest snapshot as a ready-to-send JSON line, with a version counter.
struct Shared {
    latest: Mutex<(u64, Option<String>)>,
    changed: Condvar,
}

/// Decrements the open-connection count however `serve` ends.
struct ClientGuard(Arc<AtomicUsize>);

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The listening side, owned by the service. Cheap to clone.
#[derive(Clone)]
pub struct Server {
    shared: Arc<Shared>,
    /// Connections accepted so far (including one still authenticating).
    clients: Arc<AtomicUsize>,
}

impl Server {
    /// Starts listening and writes the discovery file. Commands received
    /// from windows are forwarded to the daemon through `tx`.
    pub fn start(tx: Sender<Event>) -> io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let token = random_token();
        let info = Discovery {
            port,
            token: token.clone(),
            pid: std::process::id(),
        };
        create_private_dir(&runtime_dir())?;
        write_private(
            &discovery_path(),
            &serde_json::to_vec(&info).map_err(io::Error::other)?,
        )?;
        debug!("settings window channel on 127.0.0.1:{port}");

        let shared = Arc::new(Shared {
            latest: Mutex::new((0, None)),
            changed: Condvar::new(),
        });
        let clients = Arc::new(AtomicUsize::new(0));
        let server = Self {
            shared,
            clients: Arc::clone(&clients),
        };
        let accepting = server.clone();
        thread::Builder::new().name("ipc".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                if clients.fetch_add(1, Ordering::Relaxed) >= MAX_CLIENTS {
                    clients.fetch_sub(1, Ordering::Relaxed);
                    warn!("refused a settings-window connection: too many open");
                    continue;
                }
                let (s, tx, token, count) = (
                    accepting.clone(),
                    tx.clone(),
                    token.clone(),
                    Arc::clone(&clients),
                );
                let _ = thread::Builder::new()
                    .name("ipc-client".into())
                    .spawn(move || s.serve(stream, &tx, &token, &count));
            }
        })?;
        Ok(server)
    }

    /// Records a new state for the connected windows.
    pub fn publish(&self, update: &UiUpdate) {
        // Serialising a large snapshot for nobody is wasted work (developer
        // mode publishes every 400 ms).
        if self.clients.load(Ordering::Relaxed) == 0 {
            return;
        }
        let UiUpdate::Snapshot(snapshot) = update else {
            return;
        };
        let msg = ToClient::Snapshot {
            snapshot: snapshot.clone(),
        };
        let Ok(line) = serde_json::to_string(&msg) else {
            return;
        };
        if let Ok(mut latest) = self.shared.latest.lock() {
            latest.0 += 1;
            latest.1 = Some(line);
            self.shared.changed.notify_all();
        }
    }

    /// One window: authenticate, then send snapshots and receive commands.
    fn serve(
        &self,
        stream: TcpStream,
        tx: &Sender<Event>,
        token: &str,
        clients: &Arc<AtomicUsize>,
    ) {
        let _guard = ClientGuard(Arc::clone(clients));
        if stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .is_err()
        {
            return;
        }
        // Only the first line is time-limited: after authentication the
        // window may stay idle for a long time.
        if stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .is_err()
        {
            return;
        }
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut lines = BufReader::new(read_half).lines();
        let authorised = matches!(
            lines.next().and_then(Result::ok).and_then(|l| serde_json::from_str::<ToService>(&l).ok()),
            Some(ToService::Auth { token: t }) if tokens_equal(&t, token)
        );
        if !authorised {
            warn!("refused a settings-window connection with a wrong secret");
            return;
        }
        let _ = stream.set_read_timeout(None);
        debug!("settings window connected");
        let alive = Arc::new(AtomicBool::new(true));

        // Writer: the latest snapshot whenever it changes, pings meanwhile.
        let (shared, writer_alive) = (Arc::clone(&self.shared), Arc::clone(&alive));
        let mut out = stream;
        let _ = thread::Builder::new()
            .name("ipc-writer".into())
            .spawn(move || {
                let mut sent = 0u64;
                while writer_alive.load(Ordering::Relaxed) {
                    let line = {
                        let Ok(guard) = shared.latest.lock() else {
                            return;
                        };
                        let (guard, _) = shared
                            .changed
                            .wait_timeout_while(guard, Duration::from_secs(5), |l| l.0 == sent)
                            .unwrap_or_else(|e| e.into_inner());
                        if guard.0 != sent {
                            sent = guard.0;
                            guard.1.clone()
                        } else {
                            serde_json::to_string(&ToClient::Ping).ok()
                        }
                    };
                    if let Some(line) = line {
                        if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                            break;
                        }
                    }
                }
                writer_alive.store(false, Ordering::Relaxed);
            });

        // Reader: commands.
        for line in lines {
            let Ok(line) = line else { break };
            match serde_json::from_str::<ToService>(&line) {
                Ok(cmd) => {
                    let event = match cmd {
                        ToService::Auth { .. } => continue,
                        ToService::Reload => Event::Reload,
                        ToService::Rescan => Event::Rescan,
                        ToService::ChangeHost { link, slot, host } => {
                            Event::ChangeHost { link, slot, host }
                        }
                        ToService::StartPairing { link } => Event::StartPairing { link },
                        ToService::ChoosePairing { link, index } => {
                            Event::ChoosePairing { link, index }
                        }
                        ToService::StopPairing { link } => Event::StopPairing { link },
                        ToService::Unpair { link, slot } => Event::Unpair { link, slot },
                        ToService::Quit => Event::Quit,
                    };
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(e) => debug!("ignoring unknown message from the settings window: {e}"),
            }
        }
        alive.store(false, Ordering::Relaxed);
        debug!("settings window disconnected");
    }

    /// Removes the discovery file (on clean exit).
    pub fn close(&self) {
        let path = discovery_path();
        let ours = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Discovery>(&b).ok())
            .is_some_and(|d| d.pid == std::process::id());
        if ours {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Window side
// ---------------------------------------------------------------------------

/// A connection from the settings window to the service.
pub struct Client {
    writer: Mutex<TcpStream>,
    /// `(version, snapshot)`: the version changes with every update, so the
    /// window can avoid cloning an unchanged snapshot on every frame.
    latest: Arc<Mutex<(u64, Option<Snapshot>)>>,
    connected: Arc<AtomicBool>,
}

impl Client {
    /// Connects to the running service, if any. `on_update` is called (from
    /// a background thread) whenever a new snapshot arrives or the
    /// connection drops.
    pub fn connect(on_update: impl Fn() + Send + 'static) -> io::Result<Self> {
        let raw = std::fs::read(discovery_path())?;
        let info: Discovery = serde_json::from_slice(&raw).map_err(io::Error::other)?;
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, info.port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))?;
        let auth = serde_json::to_string(&ToService::Auth { token: info.token })
            .map_err(io::Error::other)?;
        writeln!(stream, "{auth}")?;

        let latest = Arc::new(Mutex::new((0u64, None)));
        let connected = Arc::new(AtomicBool::new(true));
        let (reader, l, c) = (
            BufReader::new(stream.try_clone()?),
            Arc::clone(&latest),
            Arc::clone(&connected),
        );
        thread::Builder::new()
            .name("ipc-reader".into())
            .spawn(move || {
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if let Ok(ToClient::Snapshot { snapshot }) = serde_json::from_str(&line) {
                        if let Ok(mut slot) = l.lock() {
                            slot.0 = slot.0.saturating_add(1);
                            slot.1 = Some(snapshot);
                        }
                        on_update();
                    }
                }
                c.store(false, Ordering::Relaxed);
                on_update();
            })?;
        Ok(Self {
            writer: Mutex::new(stream),
            latest,
            connected,
        })
    }

    /// Whether the connection to the service is still open.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// The most recent state received.
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.latest.lock().ok().and_then(|s| s.1.clone())
    }

    /// The state if it changed since `seen`, with its new version.
    pub fn snapshot_since(&self, seen: u64) -> Option<(u64, Snapshot)> {
        let latest = self.latest.lock().ok()?;
        if latest.0 == seen {
            return None;
        }
        latest.1.clone().map(|snapshot| (latest.0, snapshot))
    }

    /// Sends a command to the service.
    pub fn send(&self, msg: &ToService) -> io::Result<()> {
        let line = serde_json::to_string(msg).map_err(io::Error::other)?;
        let mut w = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("connection poisoned"))?;
        writeln!(w, "{line}")?;
        w.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let cmd = ToService::ChangeHost {
            link: "k".into(),
            slot: 2,
            host: 1,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(
            json,
            r#"{"cmd":"change_host","link":"k","slot":2,"host":1}"#
        );
        assert_eq!(serde_json::from_str::<ToService>(&json).unwrap(), cmd);
        let snap = ToClient::Snapshot {
            snapshot: Snapshot::default(),
        };
        assert!(serde_json::to_string(&snap)
            .unwrap()
            .starts_with(r#"{"type":"snapshot""#));
    }

    #[test]
    fn tokens_are_random() {
        assert_ne!(random_token(), random_token());
        assert_eq!(random_token().len(), 32);
    }

    #[test]
    fn compares_tokens_without_short_circuiting() {
        assert!(tokens_equal("abc", "abc"));
        assert!(!tokens_equal("abc", "abd"));
        assert!(!tokens_equal("abc", "ab"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runtime_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("logirev-ipc-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        create_private_dir(&dir).unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = dir.join("service.json");
        write_private(&file, b"{}").unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
