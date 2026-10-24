//! `logirev` — the background service.
//!
//! Start it once and leave it running: it configures every Logitech device
//! as soon as it connects or wakes up. Diagnostic commands live in the
//! companion program `logirevctl`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use log::error;

use logirev::app::{self, ServiceOptions};

#[derive(Parser)]
#[command(
    version,
    about = "Keep Logitech HID++ devices configured (see also: logirevctl)"
)]
struct Cli {
    /// Configuration file [default: logirev.toml next to the executable]
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Override the log level (error, warn, info, debug, trace)
    #[arg(short, long)]
    log_level: Option<String>,

    /// Write a Wireshark capture of the HID++ traffic to this file
    #[arg(long)]
    pcap: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let opts = ServiceOptions {
        config: cli.config,
        log_level: cli.log_level,
        pcap: cli.pcap,
    };
    match app::run_service(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            // Before the logger is installed, print directly; afterwards the
            // logger writes to both the console and the log file.
            if log::max_level() == log::LevelFilter::Off {
                eprintln!("error: {msg}");
            } else {
                error!("{msg}");
                log::logger().flush();
            }
            ExitCode::FAILURE
        }
    }
}
