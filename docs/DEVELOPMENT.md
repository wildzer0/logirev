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
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings   # Windows lints from Linux
cargo build --release --target x86_64-unknown-linux-musl                    # fully static Linux binary
```

Hardware cannot be reached from WSL without
[usbipd-win](https://github.com/dorssel/usbipd-win); test on Windows directly.

## Programs

| Program | Source | Purpose |
|---|---|---|
| `logirev` | `src/main.rs` | the background service |
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
| `src/logging.rs` | size-capped rotating log file |
| `src/pcap.rs` | size-capped Wireshark capture (USBPcap link type) |
| `tools/hidpp.lua` | Wireshark dissector |
| `config/logirev.toml` | commented default configuration (embedded in the binary) |

## Architecture in brief

```
 reader threads (1 per HID path) ──frame──▶ Transport queue ──wake──▶ ┐
 Ctrl+C handler ─────────────────────────────── Event::Quit ────────▶ ├─▶ daemon event loop
 (future) tray / foreground-window hooks ──── Event::... ──────────▶ ┘     (single thread)
```

- One **`Link`** per open receiver or direct device, holding a `Transport`
  and one **`Session`** per connected device (feature table, diverted
  buttons, slow-mode state).
- The loop sleeps on a channel until an event, a scheduled device setup or
  the 2-second housekeeping tick (config file changed? new receiver?).
- Requests are synchronous: `Transport::request` waits for the answer whose
  device index and bytes 2–3 match, setting other frames aside for the loop.
- The service uses software id `0x0A`, `logirevctl` uses `0x0B`, so both can
  talk to the same receiver at once.

The protocol itself is described in plain language in
[PROTOCOL.md](PROTOCOL.md).

## Adding a setting or a feature

Nothing is hard-coded per model: devices are identified by name and kind and
their features are discovered at runtime. To support a new setting:

1. add the feature's functions to `Device` in `src/hidpp/features.rs`
   (and its name to `feature_name`);
2. add the field to `RawProfile` / `Profile` in `src/config.rs`, with
   validation and a test;
3. apply it in `apply_settings` in `src/daemon.rs`;
4. show it in `scan_device` in `src/bin/logirevctl.rs`;
5. document it in `docs/CONFIGURATION.md` and `config/logirev.toml`.

## Releasing

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
