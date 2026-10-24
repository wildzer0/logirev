# Changelog

All notable changes are listed here. Versions follow
[Semantic Versioning](https://semver.org).

## [1.0.0] — unreleased

### Added
- **Settings window** (`logirev --settings`, opened by the tray icon, by
  starting `logirev.exe` by hand, and on first run): every device with
  battery and status; per-device pages for buttons (action editor with a
  shortcut recorder), pointer and wheel, keyboard, per-program buttons
  (pick from running programs) and Easy-Switch computers; macro editor;
  settings (language, light/dark theme, start with Windows, battery
  warning, log level). Changes are saved and applied automatically.
- English and Italian interface, following the system language.
- New application icon (stylised trackball), also embedded in the `.exe`.
- Local channel between the service and the window (127.0.0.1 + secret).
- `--background` flag (used by *Start with Windows*: no window at log-in).
- Demo mode for the window (`LOGIREV_DEMO=1`), used for screenshots.

### Changed
- The configuration file is rewritten by the window (comments of a
  hand-written file are not kept).
- Minimum Rust version 1.95 (required by the window toolkit).
- The Linux download has no window (static build); build from source for it.

### Added in the 0.4 development line
- Keyboard backlight (feature 0x1982): `backlight` profile setting
  (`off`, `on`, `auto`, `manual` + `level`) and `backlight` action
  (`up`, `down`, `toggle`).
- Screen brightness action `brightness` (`change` by a percentage or `set`):
  external monitors via DDC/CI, laptop panels via Windows WMI, Linux
  kernel backlight.
- Easy-Switch: computer names (feature 0x1815) in `scan` and in the devices
  window; tray submenu *Send <device> to ▸ Computer N*; `change-host` with
  `all = true` sends every connected device at once.
- Named macros in `[macros.<name>]`, used with `{ action = "macro" }`;
  `press` / `release` steps to hold keys; `repeat = <ms>` repeats a macro or
  sequence while the button is held.

## [0.3.0] — 2026-10-24

### Added
- Windows: `logirev.exe` runs without a console window and shows an icon in
  the notification area. Hovering shows the connected devices and their
  battery; left-click opens a window listing every paired device (status,
  battery, DPI, profile, remapped buttons); right-click opens a menu (show
  devices, edit settings, reload, open log, open folder, start with
  Windows, quit).
- Windows: *Start with Windows* option (per-user `Run` registry entry, no
  administrator rights; follows the program if it is moved).
- Windows: receivers are detected the moment they are plugged in (device
  notifications instead of periodic scanning).
- Notifications for problems that need attention (settings file with
  mistakes, receiver that cannot be opened).
- "Reload settings" command.
- Windows application manifest: modern controls and sharp rendering on
  high-resolution screens.

- Battery: the level shown is kept up to date from the device's own reports;
  a notification warns once when it drops to `battery_warning` (default 15%)
  and again only after charging.
- Per-program buttons (Windows): `[[profile.app]]` blocks change buttons
  while given programs are in the foreground (event-driven, no polling).
- New actions: `scroll`, `cycle-dpi`, `change-host` (Easy-Switch),
  `sequence` (simple macros: keys, text, clicks, pauses) and `gesture`
  (hold a button and move: up / down / left / right / click).
- New settings: `smartshift`, `thumb_wheel_invert`, `report_rate`,
  `function_keys`, `disable_keys`.
- `scan` shows SmartShift, thumb wheel, report rate, function keys,
  disableable keys, Easy-Switch computers, and which buttons support
  gestures.

### Changed
- Windows: to watch the log in a terminal, use `logirevctl run`.

## [0.2.0] — 2026-10-24

### Added
- Two programs: `logirev` (the service) and `logirevctl` (tools: `scan`,
  `list`, `init`, `run`, `install-service`).
- Several receivers and directly connected (USB / Bluetooth) devices are
  served at the same time.
- `scan` lists every paired slot of a Bolt / Unifying receiver, including
  devices that are switched off.
- The configuration is reloaded automatically when the file is saved; a
  file with mistakes is rejected and the previous settings stay active.
- Only one copy of the service can run at a time.
- `logirevctl scan` works while the service is running.
- Linux: `logirevctl install-service` starts logirev at log-in (systemd).
- `toggle-dpi` action (slow mode for devices that change speed through DPI,
  such as the MX Ergo S) and `dpi choices` in `scan`.
- Documentation: `docs/PROTOCOL.md`, `docs/CONFIGURATION.md`,
  `docs/DEVELOPMENT.md`.
- GitHub workflows: CI on every push, automatic release on version tags.

### Changed
- The service sleeps until something happens (no polling of the
  receiver): about 3 MB of memory and no measurable CPU when idle.
- The MX Ergo S precision button is `0x00FD` (alias `precision`).

## [0.1.0]

### Added
- First version: profiles, button remapping (keys, clicks, programs, slow
  mode), DPI / pointer speed / wheel settings, size-capped log file and
  Wireshark capture.
