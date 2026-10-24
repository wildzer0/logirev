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
//! | `autostart` | "Start with Windows" (Windows only) |
//! | [`brightness`] | screen brightness (DDC/CI, WMI, Linux backlight) |
//! | [`guides`] | the guides of `docs/`, included in this documentation |
//! | [`daemon`] | the event loop: connections, profiles, button events, actions |
//! | [`config`] | reading and validating `logirev.toml` |
//! | [`foreground`] | which program is in front (per-application buttons) |
//! | [`icon`] | the application icon, drawn in code |
//! | `gui` | the settings window (feature `gui`) |
//! | [`http`] | HTTPS downloads (WinHTTP / curl) for updates |
//! | [`update`] | update check and installation from GitHub releases |
//! | [`sha256`] | checksum of downloaded updates |
//! | [`hidpp`] | the HID++ protocol: wire format, transport, features, receivers |
//! | [`input`] | synthetic keyboard/mouse input |
//! | [`instance`] | single-instance lock |
//! | [`ipc`] | local channel between the service and the settings window |
//! | [`logging`] | size-capped log file |
//! | [`pcap`] | size-capped Wireshark capture |
//! | [`service`] | start-at-login integration |
//! | [`status`] | what the service reports to the user interface |
//! | [`ui`] | tray icon and device window (Windows) |
//!
//! # Reading this documentation
//!
//! This site is generated from the source code with `cargo doc` (rustdoc,
//! the Rust counterpart of Doxygen). Every module, type and function is
//! documented; the compiler refuses undocumented public items
//! (`#![deny(missing_docs)]`). Start from:
//!
//! * [`guides`] — the user and developer guides (configuration, protocol,
//!   development, testing);
//! * [`daemon`] — how the service works;
//! * [`hidpp`] — the Logitech protocol;
//! * [`gui`] (when built with the window) — the settings window.
//!
//! To build it locally: `cargo doc --document-private-items --open`.

#![deny(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]
// The published documentation includes private items on purpose (it is the
// developer manual), so public docs may link to them.
#![allow(rustdoc::private_intra_doc_links)]

pub mod app;
#[cfg(windows)]
pub mod autostart;
pub mod brightness;
pub mod config;
pub mod daemon;
pub mod foreground;
#[cfg(feature = "gui")]
pub mod gui;
pub mod guides;
pub mod hidpp;
pub mod http;
pub mod icon;
pub mod input;
pub mod instance;
pub mod ipc;
pub mod logging;
pub mod pcap;
pub mod service;
pub mod sha256;
pub mod status;
pub mod ui;
pub mod update;
