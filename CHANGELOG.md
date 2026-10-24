# Changelog

All notable changes are listed here. Versions follow
[Semantic Versioning](https://semver.org).

## [0.3.0] — unreleased

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
