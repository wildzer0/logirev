//! **logirev** — a small, portable replacement for Logi Options+.
//!
//! It talks HID++ (Logitech's own protocol) to devices paired with a Bolt or
//! Unifying receiver, or connected directly, and keeps them configured
//! according to a plain-text profile file. See `README.md` for the user guide
//! and `docs/PROTOCOL.md` for a plain-language description of the protocol.
//!
//! The crate builds two programs:
//!
//! * `logirev` — the background service;
//! * `logirevctl` — command-line tools (`scan`, `list`, `init`, ...).
//!
//! Module map:
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`app`] | starting the service (config, logs, single instance, Ctrl+C) |
//! | [`brightness`] | screen brightness (DDC/CI, WMI, Linux backlight) |
//! | [`daemon`] | the event loop: connections, profiles, button events, actions |
//! | [`config`] | reading and validating `logirev.toml` |
//! | [`foreground`] | which program is in front (per-application buttons) |
//! | [`hidpp`] | the HID++ protocol: wire format, transport, features, receivers |
//! | [`input`] | synthetic keyboard/mouse input |
//! | [`instance`] | single-instance lock |
//! | [`logging`] | size-capped log file |
//! | [`pcap`] | size-capped Wireshark capture |
//! | [`service`] | start-at-login integration |
//! | [`status`] | what the service reports to the user interface |
//! | [`ui`] | tray icon and device window (Windows) |

pub mod app;
pub mod brightness;
pub mod config;
pub mod daemon;
pub mod foreground;
pub mod hidpp;
pub mod input;
pub mod instance;
pub mod logging;
pub mod pcap;
pub mod service;
pub mod status;
pub mod ui;
