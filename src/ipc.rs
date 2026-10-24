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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    base.unwrap_or_else(std::env::temp_dir).join("logirev")
}

fn discovery_path() -> PathBuf {
    runtime_dir().join("service.json")
}

/// A random secret, from the operating system's random hasher keys.
fn random_token() -> String {
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

// ---------------------------------------------------------------------------
// Service side
// ---------------------------------------------------------------------------

/// Latest snapshot as a ready-to-send JSON line, with a version counter.
struct Shared {
    latest: Mutex<(u64, Option<String>)>,
    changed: Condvar,
}

/// The listening side, owned by the service. Cheap to clone.
#[derive(Clone)]
pub struct Server {
    shared: Arc<Shared>,
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
        std::fs::create_dir_all(runtime_dir())?;
        std::fs::write(
            discovery_path(),
            serde_json::to_vec(&info).map_err(io::Error::other)?,
        )?;
        debug!("settings window channel on 127.0.0.1:{port}");

        let shared = Arc::new(Shared {
            latest: Mutex::new((0, None)),
            changed: Condvar::new(),
        });
        let server = Self { shared };
        let accepting = server.clone();
        thread::Builder::new().name("ipc".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let (s, tx, token) = (accepting.clone(), tx.clone(), token.clone());
                let _ = thread::Builder::new()
                    .name("ipc-client".into())
                    .spawn(move || s.serve(stream, &tx, &token));
            }
        })?;
        Ok(server)
    }

    /// Records a new state for the connected windows.
    pub fn publish(&self, update: &UiUpdate) {
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
    fn serve(&self, stream: TcpStream, tx: &Sender<Event>, token: &str) {
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut lines = BufReader::new(read_half).lines();
        let authorised = matches!(
            lines.next().and_then(Result::ok).and_then(|l| serde_json::from_str::<ToService>(&l).ok()),
            Some(ToService::Auth { token: t }) if t == token
        );
        if !authorised {
            warn!("refused a settings-window connection with a wrong secret");
            return;
        }
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
    latest: Arc<Mutex<Option<Snapshot>>>,
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

        let latest = Arc::new(Mutex::new(None));
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
                            *slot = Some(snapshot);
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
        self.latest.lock().ok().and_then(|s| s.clone())
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
}
