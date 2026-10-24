//! Starting the service: shared by the `logirev` and `logirevctl` programs.
//!
//! [`run_service`] does, in order: create the configuration file if it is
//! missing, read it, open the log file and the optional capture, make sure no
//! other copy is running, then hand over to [`crate::daemon::run`] until the
//! user presses Ctrl+C (or the session ends).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use log::info;

use crate::config::{self, Config};
use crate::daemon::{self, Event};
use crate::pcap::PcapWriter;
use crate::{instance, logging};

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
}

/// `logirev.toml` in the same folder as the executable (portable layout).
pub fn default_config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CONFIG_FILE)
}

/// Runs the background service until Ctrl+C.
pub fn run_service(opts: &ServiceOptions) -> Result<(), String> {
    let config_path = opts.config.clone().unwrap_or_else(default_config_path);
    if !config_path.exists() {
        std::fs::write(&config_path, config::TEMPLATE)
            .map_err(|e| format!("cannot create {}: {e}", config_path.display()))?;
        eprintln!("created default configuration {}", config_path.display());
    }
    let cfg = Config::load(&config_path)?;

    let level = config::level_filter(opts.log_level.as_deref().unwrap_or(&cfg.logging.level))?;
    let log_file = cfg.logging.file.as_ref().map(|f| cfg.resolve(f));
    logging::init(
        level,
        log_file.as_deref().map(|p| (p, cfg.logging.max_size)),
        true,
    )
    .map_err(|e| format!("cannot open log file: {e}"))?;

    let _guard = instance::acquire().map_err(|e| format!("{e}"))?;
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

    let (tx, rx) = mpsc::channel();
    let quit = tx.clone();
    ctrlc::set_handler(move || {
        let _ = quit.send(Event::Quit);
    })
    .map_err(|e| format!("cannot install Ctrl+C handler: {e}"))?;
    info!("running; press Ctrl+C to stop");
    daemon::run(cfg, &config_path, pcap, tx, rx)
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
