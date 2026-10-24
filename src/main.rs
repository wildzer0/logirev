//! `logirev` — the background service.
//!
//! Start it once and leave it running: it configures every Logitech device
//! as soon as it connects or wakes up.
//!
//! * **Windows** — runs without a console window and shows an icon next to
//!   the clock (see [`logirev::ui`]). To watch its log in a terminal, use
//!   `logirevctl run` instead.
//! * **Linux** — runs in the terminal or as a systemd user service.
//!
//! `logirev --settings` opens the settings window instead (feature `gui`).
//!
//! Diagnostic commands live in the companion program `logirevctl`.

// No console window on Windows: the program lives in the notification area.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use logirev::app::ServiceOptions;

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

    /// Open the settings window instead of starting the service
    #[arg(long)]
    settings: bool,

    /// Start without opening the settings window (used at log-in)
    #[arg(long)]
    background: bool,

    /// Restart after an update (waits for the previous copy to exit)
    #[arg(long, hide = true)]
    after_update: bool,
}

/// Opens the settings window, if this build has one.
fn settings(opts: &ServiceOptions) -> ExitCode {
    #[cfg(feature = "gui")]
    return logirev::gui::run(opts);
    #[cfg(not(feature = "gui"))]
    {
        let _ = opts;
        eprintln!("this build of logirev has no settings window (feature \"gui\")");
        ExitCode::FAILURE
    }
}

impl Cli {
    fn options(self) -> ServiceOptions {
        ServiceOptions {
            config: self.config,
            log_level: self.log_level,
            pcap: self.pcap,
            background: self.background,
            after_update: self.after_update,
        }
    }
}

#[cfg(windows)]
fn main() -> ExitCode {
    // Without a console, argument errors and --help are shown in a window.
    match Cli::try_parse() {
        Ok(cli) if cli.settings => settings(&cli.options()),
        Ok(cli) => logirev::ui::run(&cli.options()),
        Err(e) => {
            let ok = !e.use_stderr();
            logirev::ui::message_box("logirev", &e.to_string(), !ok);
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.settings {
        return settings(&cli.options());
    }
    let opts = cli.options();
    match logirev::app::run_service(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            // Before the logger is installed, print directly; afterwards the
            // logger writes to both the console and the log file.
            if log::max_level() == log::LevelFilter::Off {
                eprintln!("error: {msg}");
            } else {
                log::error!("{msg}");
                log::logger().flush();
            }
            ExitCode::FAILURE
        }
    }
}
