# Changelog

All notable changes are listed here. Versions follow
[Semantic Versioning](https://semver.org).

## [Unreleased]

### Added
- Battery refresh: the daemon re-reads a connected device's battery every
  minute, and the device page / device rows have a **Refresh battery**
  button. The UI shows how old the reading is.

### Fixed
- The battery level no longer stays at the value read when the device
  connected: spontaneous reports are still honoured, but a periodic
  `0x1004` query keeps the level, tray bar and low-battery warning current.

## [1.5.1] — 2026-10-26

### Changed
- The settings window has a fixed size, so the layout no longer moves while
  dragging the window borders.
- The radial-menu preview uses a lighter anti-aliasing pass (the real
  overlay is unchanged), making hovering in the Menus page smoother.

### Fixed
- The service-status indicator next to Search is now a painted dot instead
  of a `●` character that some fonts render as a green square.

## [1.5.0] — 2026-10-26

### Added
- **Receiver** page: a slot-by-slot view of Bolt/Unifying receivers with
  pair, unpair, rescan and per-device status.
- **Advanced** page (opt-in, read-only): feature list from the device,
  a `logirevctl scan --hidden` explorer for hidden features, a JSON
  diagnostics snapshot and OnboardProfiles (`0x8100`) capability detection.
  Writes and raw arbitrary memory access are intentionally not exposed.

### Changed
- The settings window uses a compact **top navigation** instead of the left
  sidebar, with a `Ctrl+K` command palette (pages and connected devices).
- Devices and receiver slots are compact rows instead of large cards;
  tighter spacing, smaller headings.

## [1.4.4] — 2026-10-26

### Fixed
- Opening the radial menu no longer leaves the Windows busy / app-starting
  cursor on screen: the overlay now forces the normal arrow cursor when it
  appears and handles `WM_SETCURSOR`.

## [1.4.3] — 2026-10-26

### Changed
- The *Radial menus* page is now an accordion: the menus are a compact list
  and only the selected one shows its details, so the last menu is always
  reachable without scrolling through all the others.
- The item rows of a menu are a minimal icon + label + action line with
  small edit and delete buttons.

## [1.4.2] — 2026-10-26

### Fixed
- The radial menu now closes when clicking **outside** the wheel, including
  on the transparent shadow: Windows does not always deliver an outside
  click to a window that never takes the focus, so a short fallback timer
  watches the mouse buttons while the menu is open.

### Added
- New `lock` action (and the *Lock the PC* preset) that locks the screen
  through the system (`LockWorkStation` on Windows, the session manager or
  the usual screen lockers on Linux) instead of simulating `Win+L`, which
  Windows ignores when it comes from `SendInput`.

### Changed
- The *Radial menus* page is now a graphical editor: the wheel is shown at
  the top; click a sector to edit it and drag a sector onto another to move
  it. The up/down arrows are gone.
- A menu can have up to **ten** items (was eight); the page shows `n/10`.

## [1.4.1] — 2026-10-26

### Added
- **Live preview** of a radial menu under each menu in the *Radial menus*
  page: the wheel, icons, labels and title are drawn with the same painter
  as the Windows overlay; hover a wedge to see the highlight.

### Changed
- Radial menu redesign: a frosted panel with separated wedges, larger
  labels and icons and a clearer hover rim.
- The menus page shows the item count against the limit (`n/8`).

### Fixed
- Radial menu items are now chosen when the left mouse button is **released**
  (previously on press): the injected click or shortcut no longer starts
  while the physical button is still held, which made Back / Forward / click
  actions unreliable.
- Holding the button and releasing without moving keeps the menu open even
  when the menu had to be moved away from a screen edge (no accidental
  selection).
- A choice still runs its global action (shortcut, click, program, macro) if
  the device fell asleep while the menu was open.

## [1.4.0] — 2026-10-26

### Added
- **Radial menus** (Windows): a ring of up to eight custom actions that
  opens under the mouse pointer when any configurable button of a mouse,
  trackball or trackpad is pressed. Each item can run any action (program,
  shortcut, macro, desktop...); the menu does not take the focus. It can be
  used with a **click** (opens and stays open) or by **holding the button,
  moving over a sector and releasing**; a quick click never chooses by
  accident. New *Radial menus* page in the settings window.
- `logirev.exe --radial-demo` shows a sample menu without a receiver.
- New `desktop` action (`next` / `prev`) to change virtual desktop,
  available in buttons, gestures and menu items.

### Fixed
- Pointer speed: 2.0× was offered by the window but rejected by the firmware;
  the accepted range is now 0.18–1.99 everywhere.
- The configuration is rejected with a clear message for an invalid
  `product_id`, `language` or `theme`, and when the same button is written
  twice (for example `back` and `"0x53"`).
- The button editor finds actions written with a numeric control id
  (`"0x00C3"`) instead of showing them as "default".
- Turning *Check for updates* off or on works without restarting the
  service, and the update message follows the interface language.
- *Restart service* no longer freezes the window for 1.5 seconds.
- A HID reader that cannot start is reported instead of panicking; an
  unreadable backlight no longer stops the others; `logirevctl scan` reports
  a configuration file that does not load.
- The settings-window channel writes its discovery file in a private folder
  (0700/0600), uses a random token from the operating system, refuses more
  than eight connections and limits the time a client may take to
  authenticate. The Linux instance lock uses a private folder and file too.
- Update versions are compared as semver (`rc.10` is newer than `rc.9`) and
  downloads are capped at 64 MiB.
- Device setup uses a shorter timeout (a sleeping device no longer blocks
  the event loop for seconds per message), the file is not reloaded twice
  after saving, and the window clones the snapshot only when it changes.

### Changed
- The button editor offers the *Radial menu* action only for pointing
  devices, and the ready-made actions include *Next/Previous desktop*.

## [1.4.0-rc.1] — 2026-10-26

Release candidate for testing the radial menu; superseded by 1.4.0.

## [1.3.1] — 2026-10-24

### Fixed
- Macros could not be renamed: the name field discarded every key typed.

## [1.3.0] — 2026-10-24

### Added
- **On-screen messages**: a short message at the bottom of the screen when
  a button changes the DPI, precision mode, computer or keyboard backlight
  (`[general] osd`).
- **Battery on the tray icon**: a coloured bar with the lowest battery
  level (`tray_battery`), and a notification when charging is complete.
- **Scrolling** for every mouse (Windows): wheel speed and smooth scrolling
  (`scroll_speed`, `smooth_scrolling`).
- **Backup**: export and import all settings from the settings window.

### Changed
- Battery and on-screen notifications follow the interface language.

### Fixed
- Turning developer mode on or off in the settings window is now saved.

## [1.2.0] — 2026-10-24

### Added
- **Developer mode** (*Settings → Developer mode*): a *Technical* tab on
  every device with its identity (serial number, unit and model ids),
  firmware components, every feature with its version and type (hidden,
  engineering...), the live HID++ messages, and *Copy / Save report*.
  Read-only; the service records messages only while it is on.
- `logirevctl scan` shows each feature's version and type;
  `logirevctl scan --hidden` also shows identity and firmware components
  and lists the features Logitech keeps hidden (without using them).

### Changed
- **Button editor**: ready-made actions (Back, Copy, Paste, Task view,
  Play / Pause, Volume...) are applied with one click; the other actions
  are tiles that open their settings. A new shortcut starts recording at
  once, and *Restore default* resets the button in one click. The whole
  button row opens the editor.
- **Clearer choices**: on/off settings are three labelled buttons
  ("Don't change", "On", "Off"); DPI and pointer speed say whether the
  device keeps its own value (showing the current one) or uses a chosen
  value, with one-click common DPI values.
- **Saving** shows a clear message at the bottom of the window
  ("Applying settings..." then "Settings applied").

### Fixed
- **Saving no longer disconnects the devices** for a moment: the service
  applies the new settings to the devices already connected instead of
  setting them up again from scratch.
- **Pairing a Bolt keyboard**: the device list no longer grows forever when
  the keyboard announces itself repeatedly; the list scrolls.
- **Device list**: cards wrap onto several rows instead of being cut off.
- **Keyboard keys** are shown by name (e.g. "Play / Pause", "Emoji",
  "Backlight up") in English and Italian, instead of a hexadecimal number.

## [1.1.0] — 2026-10-24

### Added
- **Pairing**: add devices to a Bolt or Unifying receiver without Logi
  Options+ or the Logi web app. In the window: *Pair a new device* next to
  the receiver, with a guided assistant (Bolt discovery, typed code for
  keyboards, click sequence for mice). From the command line:
  `logirevctl pair`.
- **Remove pairing** (right-click a device card, or the device page;
  `logirevctl unpair <slot>`).
- **Code documentation** generated with rustdoc, including the guides, and
  published on GitHub Pages; the build now refuses undocumented items.

## [1.0.0] — 2026-10-24

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
- **Automatic updates** from GitHub releases: daily check by the service
  (notification), check on opening the window, *Update now* downloads the
  new `logirev.exe` / `logirevctl.exe`, verifies them against
  `SHA256SUMS.txt`, replaces them and restarts. `[general] check_updates`
  turns the check off. Releases now also publish the bare `.exe` files.
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
  window; tray submenu *Send `<device>` to ▸ Computer N*; `change-host` with
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
