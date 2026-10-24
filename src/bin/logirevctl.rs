//! `logirevctl` — command-line tools for logirev.
//!
//! * `scan` — show every receiver, its paired slots and, for connected
//!   devices, their features, settings and remappable buttons;
//! * `list` — show the raw HID++ interfaces found on the system;
//! * `init` — write a commented default configuration file;
//! * `run` — run the service in this terminal (same as `logirev`);
//! * `install-service` — start logirev at log-in (Linux, systemd);
//! * `pair` / `unpair <slot>` — add a device to the receiver / remove one.
//!
//! `scan` uses its own HID++ software id, so it works while the service is
//! running.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hidapi::HidApi;
use log::{error, LevelFilter};

use logirev::app::{self, ServiceOptions};
use logirev::config::{self, Config};
use logirev::hidpp::features::{
    control_name, feature_name, id, Device, FeatureTable, DISABLE_KEY_NAMES,
};
use logirev::hidpp::frame::RECEIVER_INDEX;
use logirev::hidpp::pairing::{mouse_clicks, unpair, Family, Pairing, Stage};
use logirev::hidpp::receiver::{paired_devices, MAX_SLOTS};
use logirev::hidpp::transport::{self, Transport};
use logirev::{logging, service};

/// Software id used by the CLI (the service uses 0x0A).
const CLI_SOFTWARE_ID: u8 = 0x0B;

#[derive(Parser)]
#[command(
    version,
    about = "Command-line tools for logirev (Logitech HID++ configuration)"
)]
struct Cli {
    /// Configuration file [default: logirev.toml next to the executable]
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Log level for this command (error, warn, info, debug, trace)
    #[arg(short, long, global = true)]
    log_level: Option<String>,

    /// Write a Wireshark capture of the HID++ traffic to this file
    #[arg(long, global = true)]
    pcap: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show receivers, paired devices, settings and remappable buttons
    Scan,
    /// List the HID++ interfaces visible to the system
    List,
    /// Create a commented configuration file
    Init {
        /// Overwrite an existing file
        #[arg(long)]
        force: bool,
    },
    /// Run the service in this terminal (Ctrl+C to stop)
    Run,
    /// Start logirev automatically at log-in (Linux: systemd user service)
    InstallService,
    /// Pair a new device with the receiver
    Pair,
    /// Remove the device in SLOT (1-6, see `scan`) from the receiver
    Unpair {
        /// Receiver slot of the device to remove
        slot: u8,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config.clone().unwrap_or_else(app::default_config_path);
    let result = match &cli.command {
        Cmd::Run => app::run_service(&ServiceOptions {
            config: Some(config_path),
            log_level: cli.log_level.clone(),
            pcap: cli.pcap.clone(),
            background: true,
            after_update: false,
        }),
        Cmd::Scan => console_logging(&cli).and_then(|()| cmd_scan(&cli, &config_path)),
        Cmd::List => console_logging(&cli).and_then(|()| cmd_list()),
        Cmd::Init { force } => cmd_init(&config_path, *force),
        Cmd::InstallService => cmd_install_service(&config_path),
        Cmd::Pair => console_logging(&cli).and_then(|()| cmd_pair(&cli)),
        Cmd::Unpair { slot } => console_logging(&cli).and_then(|()| cmd_unpair(&cli, *slot)),
    };
    match result {
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

fn cmd_init(config_path: &Path, force: bool) -> Result<(), String> {
    if config_path.exists() && !force {
        return Err(format!(
            "{} already exists (use --force to overwrite)",
            config_path.display()
        ));
    }
    std::fs::write(config_path, config::TEMPLATE)
        .map_err(|e| format!("cannot write {}: {e}", config_path.display()))?;
    println!("wrote {}", config_path.display());
    Ok(())
}

fn cmd_install_service(config_path: &Path) -> Result<(), String> {
    if cfg!(windows) {
        return Err("on Windows, use \"Start with Windows\" in the tray icon menu".into());
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("logirev")))
        .filter(|p| p.exists())
        .ok_or("the \"logirev\" program must be in the same folder as logirevctl")?;
    let config = std::fs::canonicalize(config_path).map_err(|e| {
        format!(
            "{}: {e} (run \"logirevctl init\" first)",
            config_path.display()
        )
    })?;
    let unit = service::install_systemd_unit(&exe, &config)?;
    println!("wrote {}", unit.display());
    println!(
        "now run:  systemctl --user daemon-reload && systemctl --user enable --now {}",
        service::UNIT_NAME
    );
    Ok(())
}

/// Opens the first receiver that can pair devices (Bolt or Unifying).
fn open_pairing_receiver(cli: &Cli) -> Result<Transport, String> {
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let groups = transport::group_devices(&transport::enumerate(&api));
    let (_, collections) = groups
        .into_iter()
        .find(|(_, c)| Family::of(c[0].pid).is_some())
        .ok_or("no Bolt or Unifying receiver found")?;
    let pcap = app::open_pcap(cli.pcap.as_deref(), 10 * 1024 * 1024)?;
    Transport::open(&api, &collections, CLI_SOFTWARE_ID, pcap, None)
        .map_err(|e| format!("{e} (close Logi Options+ and retry)"))
}

fn cmd_pair(cli: &Cli) -> Result<(), String> {
    let mut t = open_pairing_receiver(cli)?;
    // The receiver must report connections (the service also enables this).
    if let Ok(flags) = t.read_register(0x00, &[]) {
        if flags.len() >= 3 {
            let _ = t.write_register(0x00, &[flags[0], flags[1] | 0x09, flags[2]]);
        }
    }
    let known: Vec<u8> = paired_devices(&mut t).iter().map(|p| p.slot).collect();
    let mut pairing =
        Pairing::start(&mut t, known).map_err(|e| format!("cannot start pairing: {e}"))?;
    println!("{}: pairing mode on.", t.name);
    match pairing.family {
        Family::Unifying => {
            println!("Turn the device on, or press its connect button, within 30 seconds.")
        }
        Family::Bolt => println!(
            "Press and hold the device's connect / Easy-Switch button until it blinks fast (30 s)."
        ),
    }

    let mut shown = 0usize;
    let mut last_stage = pairing.stage.clone();
    loop {
        match t.next_event(std::time::Duration::from_millis(500)) {
            Some(transport::Incoming::Frame(f)) => {
                pairing.on_frame(&f);
            }
            Some(transport::Incoming::Lost(e)) => return Err(format!("receiver lost: {e}")),
            None => {}
        }
        pairing.check_timeout();
        // Bolt: report found devices; pair the first complete one.
        if pairing.family == Family::Bolt && pairing.stage == Stage::Waiting {
            for f in &pairing.found[shown..] {
                println!(
                    "Found: {} ({})",
                    f.name.as_deref().unwrap_or("unnamed device"),
                    f.kind
                );
            }
            shown = pairing.found.len();
            if let Some(i) = pairing
                .found
                .iter()
                .position(|f| f.address != [0; 6] && f.name.is_some())
            {
                pairing
                    .choose(&mut t, i)
                    .map_err(|e| format!("cannot pair: {e}"))?;
                println!("Pairing...");
            }
        }
        if pairing.stage != last_stage {
            last_stage = pairing.stage.clone();
            match &pairing.stage {
                Stage::Passkey {
                    passkey,
                    keyboard: true,
                    ..
                } => {
                    println!("On the keyboard, type {passkey} and press Enter.");
                }
                Stage::Passkey {
                    passkey,
                    keyboard: false,
                    ..
                } => {
                    let clicks: Vec<&str> = mouse_clicks(passkey)
                        .iter()
                        .map(|r| if *r { "right" } else { "left" })
                        .collect();
                    println!("On the mouse, click: {}", clicks.join(", "));
                    println!("then press the left and right buttons together.");
                }
                Stage::Paired { slot } => {
                    println!("Paired in slot {slot}.");
                    return Ok(());
                }
                Stage::Failed { reason } => return Err(format!("pairing failed: {reason}")),
                Stage::Cancelled => return Err("pairing cancelled".into()),
                Stage::Waiting | Stage::Pairing => {}
            }
        }
    }
}

fn cmd_unpair(cli: &Cli, slot: u8) -> Result<(), String> {
    if !(1..=MAX_SLOTS).contains(&slot) {
        return Err(format!("slot must be between 1 and {MAX_SLOTS}"));
    }
    let mut t = open_pairing_receiver(cli)?;
    let name = paired_devices(&mut t)
        .into_iter()
        .find(|p| p.slot == slot)
        .and_then(|p| p.name)
        .unwrap_or_else(|| format!("slot {slot}"));
    unpair(&mut t, slot).map_err(|e| format!("cannot remove {name}: {e}"))?;
    println!("Removed {name} from {}.", t.name);
    Ok(())
}

fn cmd_list() -> Result<(), String> {
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let groups = transport::group_devices(&transport::enumerate(&api));
    if groups.is_empty() {
        println!("No Logitech HID++ interface found.");
    }
    for (key, collections) in groups {
        println!("{key}");
        for i in collections {
            let what = transport::receiver_name(i.pid)
                .map(|n| format!("{n} receiver"))
                .unwrap_or_else(|| {
                    if i.bluetooth {
                        "Bluetooth device".into()
                    } else {
                        "device".into()
                    }
                });
            println!(
                "  {:04x}:{:04x}  {what:<18} usage {:04x}/{}  interface {}  {:?}  {}",
                transport::LOGITECH_VID,
                i.pid,
                i.usage_page,
                i.usage,
                i.interface,
                i.product,
                i.path.to_string_lossy()
            );
        }
    }
    Ok(())
}

fn cmd_scan(cli: &Cli, config_path: &Path) -> Result<(), String> {
    // The configuration is optional here: it is only used to show which
    // profile each device would get.
    let cfg = Config::load(config_path).ok();
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let groups = transport::group_devices(&transport::enumerate(&api));
    if groups.is_empty() {
        return Err("no Logitech receiver or device found".into());
    }
    let pcap = app::open_pcap(cli.pcap.as_deref(), 10 * 1024 * 1024)?;

    for (_, collections) in groups {
        let mut t = match Transport::open(&api, &collections, CLI_SOFTWARE_ID, pcap.clone(), None) {
            Ok(t) => t,
            Err(e) => {
                println!(
                    "\n{} ({:04x}): {e}",
                    collections[0].product, collections[0].pid
                );
                continue;
            }
        };
        println!("\n=== {}", t.name);
        if t.is_receiver {
            let paired = paired_devices(&mut t);
            for slot in 1..=MAX_SLOTS {
                if t.ping(slot).is_some() {
                    scan_device(&mut t, slot, cfg.as_ref());
                } else if let Some(p) = paired.iter().find(|p| p.slot == slot) {
                    let name = p.name.as_deref().unwrap_or("unnamed device");
                    println!(
                        "\n[slot {slot}] {name}  ({}, id {:04x}) - paired, not connected",
                        p.kind, p.wpid
                    );
                }
            }
            if paired.is_empty() {
                println!("  (the list of paired-but-sleeping devices is not available for this receiver)");
            }
        } else {
            scan_device(&mut t, RECEIVER_INDEX, cfg.as_ref());
        }
    }
    println!(
        "\nSleeping devices do not answer: move the mouse / press a key to wake them up and retry."
    );
    Ok(())
}

/// Prints everything readable about the device at `slot`.
fn scan_device(t: &mut Transport, slot: u8, cfg: Option<&Config>) {
    let Some((major, minor)) = t.ping(slot) else {
        return;
    };
    let label = if slot == RECEIVER_INDEX {
        "direct".to_owned()
    } else {
        format!("slot {slot}")
    };
    let features = match FeatureTable::discover(t, slot) {
        Ok(f) => f,
        Err(e) => {
            println!("\n[{label}] HID++ {major}.{minor}: cannot read features: {e}");
            return;
        }
    };
    let mut dev = Device::new(t, slot, &features);
    let name = dev.name().unwrap_or_default();
    let kind = dev
        .kind()
        .map(|k| k.to_string())
        .unwrap_or_else(|_| "?".into());
    println!("\n[{label}] {name}  ({kind}, HID++ {major}.{minor})");
    print_field("firmware", dev.firmware());
    print_field("battery", dev.battery());
    if features.has(id::POINTER_SPEED) {
        print_field(
            "pointer speed",
            dev.pointer_speed()
                .map(|s| format!("{:.2}", f32::from(s) / 256.0)),
        );
    }
    if features.has(id::ADJUSTABLE_DPI) {
        print_field("dpi", dev.dpi());
        print_field("dpi choices", dev.dpi_choices());
    }
    if features.has(id::HIRES_WHEEL) {
        print_field(
            "wheel mode",
            dev.wheel_mode()
                .map(|f| format!("high-res {}, inverted {}", f & 0x02 != 0, f & 0x04 != 0)),
        );
    }
    if features.has(id::SMART_SHIFT) || features.has(id::SMART_SHIFT_ENHANCED) {
        print_field("smartshift", dev.smart_shift());
    }
    if features.has(id::THUMB_WHEEL) {
        print_field(
            "thumb wheel",
            dev.thumb_wheel_inverted().map(|i| format!("inverted {i}")),
        );
    }
    if features.has(id::REPORT_RATE) {
        print_field(
            "report rate",
            dev.report_rate().map(|hz| format!("{hz} Hz")),
        );
        print_field(
            "rate choices",
            dev.report_rates().map(|r| format!("{r:?} Hz")),
        );
    }
    if [
        id::FN_INVERSION,
        id::FN_INVERSION_NEW,
        id::FN_INVERSION_K375S,
    ]
    .iter()
    .any(|f| features.has(*f))
    {
        print_field(
            "function keys",
            dev.fn_keys_special()
                .map(|s| if s { "special" } else { "standard" }),
        );
    }
    if features.has(id::DISABLE_KEYS) {
        print_field(
            "disable keys",
            dev.disableable_keys().map(|mask| {
                let names: Vec<&str> = DISABLE_KEY_NAMES
                    .iter()
                    .filter(|(_, b)| mask & b != 0)
                    .map(|(n, _)| *n)
                    .collect();
                format!("can disable {}", names.join(", "))
            }),
        );
    }
    if features.has(id::BACKLIGHT2) {
        print_field("backlight", dev.backlight());
        print_field("light levels", dev.backlight_levels());
    }
    if features.has(id::HOSTS_INFO) {
        print_field(
            "computer names",
            dev.host_names().map(|names| {
                names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| format!("{}: {}", i + 1, n.as_deref().unwrap_or("(empty)")))
                    .collect::<Vec<_>>()
                    .join(", ")
            }),
        );
    }
    if features.has(id::CHANGE_HOST) {
        print_field(
            "computers",
            dev.hosts()
                .map(|(n, cur)| format!("{n} (now on {})", cur + 1)),
        );
    }
    if let (Some(cfg), Ok(k)) = (cfg, dev.kind()) {
        let profile = cfg.profile_for(&name, k).map(|p| format!("\"{}\"", p.name));
        println!(
            "  {:<14} {}",
            "profile",
            profile.unwrap_or_else(|| "(none matches)".into())
        );
    }

    println!("  features:");
    for (fid, index) in features.iter() {
        println!("    {index:>3}  {fid:#06x}  {}", feature_name(fid));
    }
    if features.has(id::REPROG_CONTROLS_V4) {
        println!("  buttons (config name, id, remappable, default task):");
        match dev.controls() {
            Ok(controls) => {
                for c in controls {
                    let alias = control_name(c.cid).map_or("", |(a, _)| a);
                    let desc = logirev::hidpp::controls::name(c.cid, false).unwrap_or("");
                    let alias = if alias.is_empty() {
                        format!("\"{:#06x}\"", c.cid)
                    } else {
                        alias.to_owned()
                    };
                    let mut tags = vec![if c.is_divertable() { "yes" } else { "no" }];
                    if c.is_virtual() {
                        tags.push("virtual");
                    }
                    if c.supports_raw_xy() {
                        tags.push("gestures");
                    }
                    if c.is_mouse_button() {
                        tags.push("mouse button");
                    }
                    println!(
                        "    {alias:<16} {:#06x}  {:<32} task {:#06x}  {desc}",
                        c.cid,
                        tags.join(", "),
                        c.task
                    );
                }
            }
            Err(e) => println!("    cannot list buttons: {e}"),
        }
    }
}

fn print_field<T: std::fmt::Display, E: std::fmt::Display>(label: &str, value: Result<T, E>) {
    match value {
        Ok(v) => println!("  {label:<14} {v}"),
        Err(e) => println!("  {label:<14} ({e})"),
    }
}

/// Console-only logging for the interactive commands (warnings by default).
fn console_logging(cli: &Cli) -> Result<(), String> {
    let level = match &cli.log_level {
        Some(l) => config::level_filter(l)?,
        None => LevelFilter::Warn,
    };
    logging::init(level, None, true).map_err(|e| e.to_string())
}
