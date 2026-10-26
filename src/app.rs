//! Starting the service: shared by `logirev`, its tray interface and
//! `logirevctl run`.
//!
//! [`prepare`] does, in order: create the configuration file if it is
//! missing, read it, open the log file and the optional capture, and make
//! sure no other copy is running. [`run_service`] then hands over to
//! [`crate::daemon::run`] until the user presses Ctrl+C; the Windows tray
//! interface calls [`prepare`] itself and runs the daemon on a thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use log::info;

use crate::config::{self, Config};
use crate::daemon::{self, Event};
use crate::instance::{self, InstanceGuard};
use crate::ipc;
use crate::logging;
use crate::pcap::PcapWriter;
use crate::status::UiHooks;

/// Name of the configuration file looked up next to the executable.
pub const CONFIG_FILE: &str = "logirev.toml";

/// Command-line overrides for the service.
#[derive(Debug, Default, Clone)]
pub struct ServiceOptions {
    /// Configuration file; `None` = [`default_config_path`].
    pub config: Option<PathBuf>,
    /// Overrides `[logging] level`.
    pub log_level: Option<String>,
    /// Overrides `[logging] pcap`.
    pub pcap: Option<PathBuf>,
    /// Started automatically (at log-in): do not open the settings window.
    pub background: bool,
    /// Restarting after an update: wait for the previous copy to exit.
    pub after_update: bool,
    /// Show the radial menu with sample items (Windows; checks the look).
    pub radial_demo: bool,
}

/// Everything the daemon needs, ready to go.
pub struct Prepared {
    /// The validated configuration.
    pub cfg: Config,
    /// Where the configuration was read from (watched for changes).
    pub config_path: PathBuf,
    /// Resolved log file path, if logging to a file.
    pub log_file: Option<PathBuf>,
    /// The open Wireshark capture, if one was requested.
    pub pcap: Option<Arc<Mutex<PcapWriter>>>,
    /// Held for the lifetime of the service (single instance).
    pub instance: InstanceGuard,
}

/// `logirev.toml` in the same folder as the executable (portable layout).
pub fn default_config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CONFIG_FILE)
}

/// Loads the configuration, opens logs and capture, takes the instance lock.
///
/// `console` — also print log lines on standard error (false for the tray
/// program, which has no console).
pub fn prepare(opts: &ServiceOptions, console: bool) -> Result<Prepared, String> {
    crate::update::cleanup_old_files();
    let config_path = opts.config.clone().unwrap_or_else(default_config_path);
    if !config_path.exists() {
        std::fs::write(&config_path, config::TEMPLATE)
            .map_err(|e| format!("cannot create {}: {e}", config_path.display()))?;
        if console {
            eprintln!("created default configuration {}", config_path.display());
        }
    }
    let cfg = Config::load(&config_path)?;

    let level = config::level_filter(opts.log_level.as_deref().unwrap_or(&cfg.logging.level))?;
    let log_file = cfg.logging.file.as_ref().map(|f| cfg.resolve(f));
    logging::init(
        level,
        log_file.as_deref().map(|p| (p, cfg.logging.max_size)),
        console,
    )
    .map_err(|e| format!("cannot open log file: {e}"))?;

    let instance = acquire_instance(opts.after_update)?;
    info!(
        "logirev {} starting, configuration {}",
        env!("CARGO_PKG_VERSION"),
        config_path.display()
    );

    let pcap_path = opts
        .pcap
        .clone()
        .or_else(|| cfg.logging.pcap.as_ref().map(|p| cfg.resolve(p)));
    let pcap = open_pcap(pcap_path.as_deref(), cfg.logging.pcap_max_size)?;
    Ok(Prepared {
        cfg,
        config_path,
        log_file,
        pcap,
        instance,
    })
}

/// Takes the single-instance lock. After an update the previous copy may
/// still be shutting down, so wait for it (up to 15 seconds).
fn acquire_instance(after_update: bool) -> Result<InstanceGuard, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        match instance::acquire() {
            Ok(g) => return Ok(g),
            Err(_) if after_update && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Runs the background service in the terminal until Ctrl+C.
pub fn run_service(opts: &ServiceOptions) -> Result<(), String> {
    let p = prepare(opts, true)?;
    let (tx, rx) = mpsc::channel();
    let quit = tx.clone();
    ctrlc::set_handler(move || {
        let _ = quit.send(Event::Quit);
    })
    .map_err(|e| format!("cannot install Ctrl+C handler: {e}"))?;
    crate::foreground::watch(tx.clone());
    if p.cfg.general.check_updates {
        crate::update::spawn_checker(tx.clone());
    }
    let ipc = start_ipc(&tx);
    let hooks = ipc.clone().map(|server| UiHooks {
        observer: Box::new(move |update| server.publish(&update)),
        hotplug_events: false,
    });
    info!("running; press Ctrl+C to stop");
    let result = daemon::run(p.cfg, &p.config_path, p.pcap, tx, rx, hooks);
    if let Some(server) = ipc {
        server.close();
    }
    drop(p.instance);
    result
}

/// Opens the channel used by the settings window. A failure only disables
/// the window's live view, so it is logged and otherwise ignored.
pub fn start_ipc(tx: &mpsc::Sender<Event>) -> Option<ipc::Server> {
    match ipc::Server::start(tx.clone()) {
        Ok(server) => Some(server),
        Err(e) => {
            log::warn!("settings window channel unavailable: {e}");
            None
        }
    }
}

/// Opens the size-limited capture file, if one is requested.
pub fn open_pcap(
    path: Option<&Path>,
    max_size: u64,
) -> Result<Option<Arc<Mutex<PcapWriter>>>, String> {
    let Some(path) = path else { return Ok(None) };
    let writer = PcapWriter::create(path, max_size)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    info!("capturing HID++ traffic to {}", path.display());
    Ok(Some(Arc::new(Mutex::new(writer))))
}
