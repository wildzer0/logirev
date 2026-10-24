# Development guide

For contributors. Users only need the [README](../README.md).

## Building and testing

Requirements: [Rust](https://rustup.rs) (stable). No C compiler and no system
libraries are needed: the HID backends are pure Rust (`hidapi` with
`linux-native-basic-udev` on Linux, `windows-native` on Windows).

```bash
cargo build --release                 # target/release/logirev{,ctl}
cargo test                            # unit tests, no hardware needed
cargo fmt --check                     # formatting
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings   # without the window
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings   # Windows lints from Linux
cargo build --release --target x86_64-unknown-linux-musl                    # fully static Linux binary
```

Hardware cannot be reached from WSL without
[usbipd-win](https://github.com/dorssel/usbipd-win); test on Windows directly.

## Programs

| Program | Source | Purpose |
|---|---|---|
| `logirev` | `src/main.rs` | the background service (Windows: tray application without console) |
| `logirevctl` | `src/bin/logirevctl.rs` | `scan`, `list`, `init`, `run`, `install-service` |

Both are thin wrappers around the library in `src/lib.rs`.

## Source map

| File | Responsibility |
|---|---|
| `src/app.rs` | service start-up: config, logging, single instance, Ctrl+C |
| `src/daemon.rs` | the event loop: links, sessions, profiles, button events, actions, config reload |
| `src/config.rs` | reading and validating `logirev.toml` |
| `src/hidpp/frame.rs` | HID++ wire format and error codes |
| `src/hidpp/transport.rs` | enumerating/grouping HID collections, reader threads, request/response matching |
| `src/hidpp/features.rs` | HID++ 2.0 features: name, battery, DPI, speed, wheel, buttons |
| `src/hidpp/receiver.rs` | receiver registers: paired devices |
| `src/input/` | synthetic keyboard/mouse: `SendInput` (Windows), `uinput` (Linux) |
| `src/instance.rs` | single-instance lock |
| `src/service.rs` | start at log-in (systemd user unit) |
| `src/status.rs` | snapshots and notices the daemon sends to the interface |
| `src/foreground.rs` | foreground-program watcher (WinEvent hook) for per-program buttons |
| `src/brightness.rs` | screen brightness: DDC/CI (dxva2), WMI fallback, Linux backlight |
| `src/ipc.rs` | local channel service ↔ settings window (127.0.0.1 + secret, JSON lines) |
| `src/icon.rs` | the application icon drawn in code, `.ico` encoder (used by `build.rs`) |
| `src/gui/` | settings window (egui/eframe, feature `gui`): `app.rs` pages, `form.rs` action editor, `style.rs` theme and icons, `i18n.rs` texts |
| `src/ui/view.rs` | content of the tray menu and device window (portable, tested) |
| `src/ui/windows/` | Win32 tray icon, menu, device window, autostart, device notifications |
| `build.rs`, `res/logirev.manifest` | Windows manifest (Common Controls 6, per-monitor DPI) embedded at link time |
| `src/logging.rs` | size-capped rotating log file |
| `src/pcap.rs` | size-capped Wireshark capture (USBPcap link type) |
| `tools/hidpp.lua` | Wireshark dissector |
| `config/logirev.toml` | commented default configuration (embedded in the binary) |

## Architecture in brief

```
 reader threads (1 per HID path) ──frame──▶ Transport queue ──wake──▶ ┐
 Ctrl+C handler (terminal) ────────────────────── Event::Quit ──────▶ ├─▶ daemon event loop
 UI thread: menu, WM_DEVICECHANGE ─── Event::Reload/Rescan/Quit ────▶ ┤     (own thread)
 foreground thread (WinEvent hook) ──────── Event::Foreground ──────▶ ┘
                                                                            │
 UI thread (Win32 message loop) ◀── WM_APP_UPDATE + queue ◀── UiUpdate ─────┘
```

On Windows the main thread runs the Win32 message loop (tray icon, menu,
device window) and the daemon runs on a second thread. The daemon publishes
a `status::Snapshot` whenever something visible changes; the UI thread never
talks to devices. Window-procedure state lives in a `RefCell` that is never
held across nested message loops (menus, message boxes).

- One **`Link`** per open receiver or direct device, holding a `Transport`
  and one **`Session`** per connected device (feature table, diverted
  buttons, slow-mode state).
- The loop sleeps on a channel until an event, a scheduled device setup or
  the 2-second housekeeping tick (config file changed? new receiver?).
- Requests are synchronous: `Transport::request` waits for the answer whose
  device index and bytes 2–3 match, setting other frames aside for the loop.
- The service uses software id `0x0A`, `logirevctl` uses `0x0B`, so both can
  talk to the same receiver at once.
- Buttons are diverted according to `Profile::buttons_for(program)`; when the
  foreground program changes, `Link::refresh_buttons` only diverts / gives
  back the buttons whose mapping changed (`apply_buttons`).
- Gesture buttons are diverted with *raw XY* reporting: while held, the
  device sends movement (`ReprogControlsV4` function 1) instead of moving
  the pointer; `Gesture::pick` chooses the action on release.
- Battery reports (function 0 of `0x1004` / `0x1000`) update the session;
  `Link::check_battery` warns once per discharge (hysteresis of 5 points).

The protocol itself is described in plain language in
[PROTOCOL.md](PROTOCOL.md).

## The settings window

`logirev --settings` runs [eframe](https://github.com/emilk/egui) with the
glow (OpenGL) renderer in its own process. It connects to the service
through `ipc::Client` (port and secret in
`%LOCALAPPDATA%\logirev\service.json`), shows `status::Snapshot`s and
edits `config::RawConfig`, which it saves 600 ms after the last change
(`RawConfig::save` validates first and writes atomically); the service picks
the file up by itself and the window also sends `Reload`.

Device edits go to a profile matching the device's exact name, created on
the first change as a copy of the profile that applied before.

To see the window without hardware (also on Linux/WSLg):

```bash
LOGIREV_DEMO=1 cargo run -- --settings            # sample devices
LOGIREV_DEMO=1 LOGIREV_PAGE=editor \
  EFRAME_SCREENSHOT_TO=shot.png cargo run --features screenshot -- --settings
```

`LOGIREV_PAGE` can be `devices`, `device`, `pointer`, `computers`,
`editor`, `macros`, `settings`, `about`. The screenshots in `docs/images`
are made this way.

New texts go into the `texts!` table of `src/gui/i18n.rs` (English and
Italian side by side; a missing translation does not compile).

## Adding a setting or a feature

Nothing is hard-coded per model: devices are identified by name and kind and
their features are discovered at runtime. To support a new setting:

1. add the feature's functions to `Device` in `src/hidpp/features.rs`
   (and its name to `feature_name`);
2. add the field to `RawProfile` / `Profile` in `src/config.rs`, with
   validation and a test;
3. apply it in `apply_settings` in `src/daemon.rs`;
4. show it in `scan_device` in `src/bin/logirevctl.rs`, publish what the
   window needs in `status::DeviceDetails` (`collect_details` in
   `daemon.rs`) and add the control to `src/gui/app.rs`;
5. document it in `docs/CONFIGURATION.md` and `config/logirev.toml`.

## Releasing

0. Run the manual checks in [TESTING.md](TESTING.md) on real hardware.
1. Update `version` in `Cargo.toml` and the `CHANGELOG.md` entry.
2. Commit, then tag and push:
   ```bash
   git tag v0.2.0
   git push origin main v0.2.0
   ```
3. The **Release** workflow (`.github/workflows/release.yml`) checks that the
   tag matches `Cargo.toml`, builds Windows (`x86_64-pc-windows-msvc`) and
   Linux (`x86_64-unknown-linux-musl`, static), and publishes
   `logirev-<tag>-windows-x64.zip`, `logirev-<tag>-linux-x64.tar.gz` and
   `SHA256SUMS.txt` on the Releases page. Tags containing `-` (e.g.
   `v0.3.0-beta.1`) are published as pre-releases.

The **CI** workflow (`.github/workflows/ci.yml`) runs formatting, lints and
tests on Linux and Windows for every push to `main` and every pull request.

## Code conventions

- Documentation comments in English on every module and public item,
  written for readers who do not know HID++.
- No `unwrap()` on data coming from devices: malformed answers become
  `HidppError::MalformedResponse`.
- Device errors are logged and never stop the service; only start-up errors
  (bad configuration, no permission) are fatal.
