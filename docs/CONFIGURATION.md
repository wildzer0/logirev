# Configuration reference

> **Most people never need this file:** everything here can be set from the
> logirev window (left-click the icon next to the clock). This page is for
> those who prefer editing text, or want the details. Note that the window
> rewrites the file when it saves, so comments you add by hand are not kept.

logirev is configured with one text file, **`logirev.toml`**, which sits next
to the program. It is created automatically, with explanations, the first
time logirev starts. Open it with any text editor (Notepad is fine).

- Lines starting with `#` are comments: they are ignored.
- Text values go between quotes: `name = "MX Ergo"`.
- **Changes are applied by themselves** about two seconds after you save.
  If you made a mistake, logirev keeps the previous settings and writes the
  problem in the log, for example:
  `profile "MX Ergo": button "back": unknown key "Altt" in "Altt+Left"`.
  (Changing `file` or `pcap` in `[logging]` needs a restart.)

---

## 1. A complete example

```toml
[general]
product_id = "auto"

[logging]
level = "info"
file = "logirev.log"
max_size = 1048576
# pcap = "logirev.pcap"

[[profile]]
name = "MX Ergo"
match = { name = "MX Ergo", kind = "trackball" }
dpi = 750

[profile.buttons]
back       = { action = "keys", keys = "Alt+Left" }
forward    = { action = "keys", keys = "Alt+Right" }
tilt_left  = { action = "keys", keys = "Ctrl+Shift+Tab" }
tilt_right = { action = "keys", keys = "Ctrl+Tab" }
precision  = { action = "toggle-dpi", dpi = 400 }

[[profile]]
name = "Any other mouse"
match = { kind = "mouse" }
```

---

## 2. `[general]`

| Setting | Meaning | Default |
|---|---|---|
| `product_id` | `"auto"` uses every Logitech receiver and device found. Write a USB product id (e.g. `"c548"` = Bolt, `"c52b"` = Unifying) to use only that kind. | `"auto"` |
| `check_updates` | look for a new version on GitHub once a day (`true` / `false`) | `true` |
| `osd` | show a short message on screen when a button changes the DPI, precision mode, computer or backlight (Windows) | `true` |
| `tray_battery` | show the lowest battery level as a bar on the tray icon (Windows) | `true` |
| `scroll_speed` | wheel speed for every mouse, from `0.25` to `5` (`1.0` = normal) (Windows) | `1.0` |
| `smooth_scrolling` | scroll smoothly instead of in steps, every mouse (Windows) | `false` |
| `developer` | developer mode: a *Technical* tab on every device (identity, firmware components, features, live HID++ messages) (`true` / `false`) | `false` |
| `language` | settings window language: `"auto"`, `"en"`, `"it"` | `"auto"` |
| `theme` | settings window look: `"system"`, `"light"`, `"dark"` | `"system"` |
| `battery_warning` | show a notification when a battery drops to this percentage; `0` = never. You are warned once, and again only after charging. | `15` |

---

## 3. `[logging]`

| Setting | Meaning | Default |
|---|---|---|
| `level` | how much detail: `error`, `warn`, `info`, `debug`, `trace` (trace = every message to the device) | `"info"` |
| `file` | log file name; remove the line to disable the file | `"logirev.log"` |
| `max_size` | maximum size of the log file, in bytes | `1048576` (1 MB) |
| `pcap` | Wireshark capture file; off unless you write it | off |
| `pcap_max_size` | maximum size of the capture, in bytes | `10485760` (10 MB) |

When a file reaches its maximum size it is renamed with `.1` (for example
`logirev.1.log`, replacing an older one) and a fresh file starts, so the
files never grow without limit.

---

## 4. Profiles — "configure the device according to what it is"

Each `[[profile]]` block describes one kind of device. When a device
connects, logirev uses the **first** profile whose `match` fits, so put
specific profiles first and generic ones last.

| Rule | Meaning | Example |
|---|---|---|
| `name` | text contained in the device name (upper/lower case ignored) | `match = { name = "MX Ergo" }` |
| `kind` | type of device | `match = { kind = "trackball" }` |
| both | both must fit | `match = { name = "MX", kind = "mouse" }` |
| none | fits every device | *(leave out `match`)* |

Kinds: `mouse`, `trackball`, `trackpad`, `keyboard`, `numpad`, `presenter`,
`remote`, `headset`, `webcam`, `other`. `logirevctl scan` shows the exact
name and kind of each device.

### Device settings

All optional: leave one out and the device keeps its own value.

| Setting | Values | Notes |
|---|---|---|
| `dpi` | e.g. `750`, `1600` | sensor resolution. `logirevctl scan` lists the accepted values (*dpi choices*). The MX Ergo S changes speed this way. |
| `pointer_speed` | `0.18` … `1.99` (1.0 = normal) | only devices that have the *PointerSpeed* feature (the MX Ergo S does not). |
| `hires_wheel` | `true` / `false` | high-resolution scrolling. Without Options+, Windows may scroll **very fast**: try it first. |
| `invert_wheel` | `true` / `false` | reverses the scroll direction. |
| `smartshift` | `{ mode = "smart", threshold = 10 }`, `{ mode = "ratchet" }`, `{ mode = "free" }` | wheels with SmartShift (MX Master): *smart* clicks, but spins freely when you scroll fast (lower threshold = frees sooner); *ratchet* always clicks; *free* always spins. |
| `thumb_wheel_invert` | `true` / `false` | reverses the thumb wheel (MX Master). |
| `report_rate` | `125` … `1000` (Hz) | how often the mouse reports; gaming mice only. `scan` lists the accepted values. |
| `function_keys` | `"standard"` / `"special"` | keyboards: whether F1–F12 act as F-keys (`standard`) or as media keys (`special`) without holding Fn. |
| `disable_keys` | e.g. `["capslock", "insert"]` | keyboards: keys that do nothing. Names: `capslock`, `numlock`, `scrolllock`, `insert`, `windows`. |
| `backlight` | `{ mode = "off" }`, `{ mode = "on" }`, `{ mode = "auto" }`, `{ mode = "manual", level = 3 }` | backlit keyboards (MX Keys...): *auto* follows the room light, *manual* keeps a fixed level (`scan` shows how many levels there are, counted from 0). |

---

## 5. Buttons

Write one line per button you want to change, under `[profile.buttons]`:

```toml
[profile.buttons]
back = { action = "keys", keys = "Alt+Left" }
```

### Button names

| Name | Button |
|---|---|
| `middle` | wheel click |
| `back`, `forward` | side buttons |
| `tilt_left`, `tilt_right` | pushing the wheel sideways |
| `precision` (or `dpi_switch`) | the button below the wheel on the MX Ergo S |
| `gesture` | gesture button (MX Master) |
| `smartshift` | wheel-mode button (MX Master) |

Any other button can be written with the id shown by `logirevctl scan`, in
quotes: `"0x00C3" = { ... }`. Only buttons marked **remappable: yes** in the
scan can be changed (left and right click never are).

Write each button **once**: an alias and its number are the same control, so
`back` together with `"0x53"` (or `precision` together with `dpi_switch`) is
rejected with an error naming both.

### Actions

| Action | What it does | Example |
|---|---|---|
| `keys` | presses a key or shortcut, for as long as you hold the button | `{ action = "keys", keys = "Ctrl+Shift+T" }` |
| `click` | acts as another mouse button: `left`, `right`, `middle`, `back`, `forward` | `{ action = "click", button = "middle" }` |
| `command` | starts a program | `{ action = "command", command = "calc.exe" }` |
| `toggle-dpi` | slow mode: first press switches to this DPI, second press back | `{ action = "toggle-dpi", dpi = 400 }` |
| `toggle-pointer-speed` | the same, for devices with *PointerSpeed* | `{ action = "toggle-pointer-speed", speed = 0.4 }` |
| `cycle-dpi` | each press moves to the next DPI of the list | `{ action = "cycle-dpi", dpi = [400, 800, 1600] }` |
| `scroll` | scrolls `up`, `down`, `left` or `right` by `amount` wheel notches | `{ action = "scroll", direction = "right", amount = 3 }` |
| `change-host` | Easy-Switch: sends the device to computer `1`, `2`, `3` or the `"next"` one; with `all = true` **every** connected device goes (e.g. keyboard and mouse together) | `{ action = "change-host", host = 2, all = true }` |
| `brightness` | screen brightness: `change` by a percentage, or `set` a percentage | `{ action = "brightness", change = -10 }` |
| `backlight` | keyboard backlight: `up`, `down` or `toggle` | `{ action = "backlight", change = "up" }` |
| `desktop` | changes virtual desktop: `direction = "next"` or `"prev"` (Windows: Win+Ctrl+←/→; Linux: Ctrl+Alt+←/→) | `{ action = "desktop", direction = "next" }` |
| `lock` | locks the screen (Windows `LockWorkStation`; on Linux the session manager or screen locker) | `{ action = "lock" }` |
| `macro` | runs a macro defined in `[macros]` (see below); `repeat = 100` repeats it every 100 ms while held | `{ action = "macro", name = "signature" }` |
| `sequence` | several steps in a row (a simple macro, see below) | `{ action = "sequence", steps = [ { keys = "Ctrl+C" } ] }` |
| `gesture` | hold the button and move the pointer (see below) | `{ action = "gesture", up = { ... } }` |
| `radial` | opens a radial menu under the pointer (any button of a pointing device, Windows; see below) | `{ action = "radial", menu = "work" }` |
| `disable` | the button does nothing | `{ action = "disable" }` |
| `default` | normal behaviour (same as not listing the button) | `{ action = "default" }` |

A program with arguments:
`{ action = "command", command = "notepad.exe", args = ["C:\\notes.txt"] }`
— in Windows paths write `\\` for every `\`.

### Macros and sequences

A **sequence** is a list of `steps` run in order. Each step is exactly one
of:

| Step | Meaning |
|---|---|
| `{ keys = "Ctrl+C" }` | press and release a shortcut |
| `{ press = "Shift" }` | press keys and keep them down... |
| `{ release = "Shift" }` | ...until released (anything still down is released at the end) |
| `{ text = "Hello" }` | type text |
| `{ click = "left" }` | click a mouse button |
| `{ delay = 200 }` | wait, in milliseconds (at most 2000 per step, 5000 in total) |

A **macro** is a sequence with a name, written once in a `[macros.<name>]`
block and used by as many buttons as you like:

```toml
[macros.signature]
steps = [ { text = "Best regards," }, { keys = "Enter" }, { text = "Your Name" } ]

[[profile]]
name = "MX Ergo"
[profile.buttons]
back = { action = "macro", name = "signature" }
```

Add `repeat = <milliseconds>` to repeat a macro (or a sequence) for as long
as the button is held, e.g. `{ action = "macro", name = "next", repeat = 150 }`.

A sequence can also be written directly on a button:

Short sequences fit on one line:

```toml
[profile.buttons]
back = { action = "sequence", steps = [ { keys = "Ctrl+A" }, { keys = "Ctrl+C" } ] }
```

Longer ones are easier to read written as their own block:

```toml
[profile.buttons.back]
action = "sequence"
steps = [
    { keys = "Ctrl+A" },
    { keys = "Ctrl+C" },
    { delay = 100 },
    { keys = "Alt+Tab" },
]
```

(A `{ ... }` group must stay on one line; when it gets long, use a block
like the one above.)

On Linux, `text` can only type the characters of a US keyboard (letters,
digits, common punctuation).

### Gestures

Hold a gesture button and move the pointer up, down, left or right: each
direction runs its own action, and a press without movement runs `click`.
While the button is held the pointer stays still. Any direction can be left
out; `threshold` (default 50) is how far you must move.

```toml
[profile.buttons.gesture]
action = "gesture"
up    = { action = "keys", keys = "Super+Tab" }          # task view
down  = { action = "keys", keys = "Super+D" }            # show desktop
left  = { action = "keys", keys = "Ctrl+Super+Left" }    # previous desktop
right = { action = "keys", keys = "Ctrl+Super+Right" }   # next desktop
click = { action = "click", button = "middle" }          # no movement
```

Only buttons marked **gestures** by `logirevctl scan` can follow the
movement; on other buttons a gesture only performs its `click` action.

### Radial menus (Windows)

A radial menu is a ring of up to **eight** actions that opens **under the
pointer**. Assign it to a button of a pointing device (mouse, trackball or
trackpad) with the `radial` action; the button itself does not need gestures.

```toml
[menus.work]
title = "Work"                       # optional, shown in the middle

[[menus.work.items]]                 # clockwise, first item at the top
label = "VS Code"
icon  = "code"                       # optional
action = { action = "command", command = "code.exe" }

[[menus.work.items]]
label = "Next desktop"
icon = "desktop"
action = { action = "desktop", direction = "next" }

[[menus.work.items]]
label = "Copy"
action = { action = "keys", keys = "Ctrl+C" }

[[profile]]
name = "MX Ergo"
[profile.buttons]
precision = { action = "radial", menu = "work" }
```

- Items are drawn clockwise from 12 o'clock; `label` is shown under the icon
  and is shortened with `…` when it does not fit. `title` appears in the
  middle, under the close X.
- Every item can run **any action** except `gesture`, `radial` (no nested
  menus) and `default`: shortcuts, clicks, programs, macros, sequences,
  `change-host`, brightness, `desktop`, `disable`...
- Icons are optional; names: `app`, `browser`, `calculator`, `camera`,
  `check`, `close`, `code`, `copy`, `crosshair`, `cut`, `desktop`, `file`,
  `folder`, `grid`, `info`, `keyboard`, `left`, `light`, `lock`, `magic`,
  `mute`, `next`, `paste`, `play`, `power`, `prev`, `refresh`, `right`,
  `save`, `settings`, `stack`, `sun`, `terminal`, `undo`, `volume`,
  `warning`, `windows`.
- It can be assigned to **any configurable button** of a pointing device
  (side buttons, wheel tilt, precision, gesture, any id shown by
  `logirevctl scan` that is remappable), not just to one dedicated key.
- Two ways to use it:
  - **click:** press the button, the menu stays open; click a sector (or
    click the X in the middle / anywhere outside to close);
  - **hold, move, release:** keep the button pressed, move over a sector and
    release it to run that item at once. Releasing in the middle keeps the
    menu open for a click; releasing outside the ring cancels.
  A release within about 0.2 s counts as a click, so a quick press never
  chooses by accident.
- The menu does not take the focus from the program you are using.
- It is available in the Windows application (the one with the tray icon);
  on Linux the service logs a warning and ignores it. To look at it without
  a receiver: `logirev.exe --radial-demo`.
- A menu can have no items while you are editing it, but a `radial` action
  pointing at an empty menu does nothing.
- The *Radial menus* page shows a **live preview** under each menu: hover a
  wedge to see the highlight exactly as it will appear. The page also shows
  the item count (`n/8`); eight is the maximum, because more would make the
  labels unreadable.

### Key names

Upper/lower case does not matter. Combine keys with `+`.

- **Modifiers:** `Ctrl`, `Shift`, `Alt`, `Super` (the Windows key; also
  `Win`), `RightCtrl`, `RightShift`, `RightAlt` (also `AltGr`)
- **Letters and digits:** `A` … `Z`, `0` … `9`
- **Function keys:** `F1` … `F24`
- **Editing:** `Enter`, `Escape` (`Esc`), `Tab`, `Space`, `Backspace`,
  `Delete` (`Del`), `Insert`, `CapsLock`
- **Navigation:** `Left`, `Right`, `Up`, `Down`, `Home`, `End`, `PageUp`,
  `PageDown`, `PrintScreen`, `ScrollLock`, `Pause`, `Menu`
- **Punctuation** (position on a US keyboard): `Minus`, `Equal`,
  `LeftBracket`, `RightBracket`, `Backslash`, `Semicolon`, `Apostrophe`,
  `Grave`, `Comma`, `Period`, `Slash`
- **Numeric keypad:** `Num0` … `Num9`
- **Media / browser:** `VolumeUp`, `VolumeDown`, `VolumeMute`,
  `MediaPlayPause`, `MediaNext`, `MediaPrevious`, `MediaStop`,
  `BrowserBack`, `BrowserForward`, `BrowserRefresh`, `BrowserHome`, `Mail`,
  `Calculator`

---

## 6. Different buttons in different programs (Windows)

After a profile's `[profile.buttons]`, add one `[[profile.app]]` block per
group of programs. While one of those programs is the window in front, its
buttons replace (or add to) the profile's buttons; when you switch to another
program, the profile's own buttons come back.

```toml
[[profile]]
name = "MX Ergo"
match = { name = "MX Ergo" }

[profile.buttons]
back = { action = "keys", keys = "Alt+Left" }

[[profile.app]]
match = ["photoshop.exe"]
buttons.back = { action = "keys", keys = "Ctrl+Z" }        # undo
buttons.tilt_left = { action = "keys", keys = "LeftBracket" }   # smaller brush

[[profile.app]]
match = ["chrome.exe", "msedge.exe", "firefox.exe"]
buttons.back = { action = "default" }   # the browser handles Back itself
```

- `match` lists program file names, as shown in Task Manager → *Details*
  (`.exe` may be left out; upper/lower case does not matter).
- The first `[[profile.app]]` that lists the program is used.
- `{ action = "default" }` inside an app block means "normal behaviour in
  this program", even if the profile remaps the button.
- Programs started "as administrator" are not recognised (Windows does not
  tell other programs about them); the profile's own buttons apply there.

## 7. Screen brightness

The `brightness` action changes **all** screens:

- external monitors through **DDC/CI**, the monitor's own control channel
  (enable "DDC/CI" in the monitor's on-screen menu if nothing happens);
- laptop screens through Windows' brightness control (or, on Linux, the
  kernel backlight; see the README for the permission rule).

```toml
tilt_left  = { action = "brightness", change = -10 }   # darker
tilt_right = { action = "brightness", change = 10 }    # brighter
```

## 8. Wireshark capture

Set `pcap = "logirev.pcap"` (or start with `--pcap logirev.pcap`) to record
every message exchanged with the devices. Open the file with
[Wireshark](https://www.wireshark.org). For readable decoding, copy
`tools/hidpp.lua` into the folder shown under *Help → About Wireshark →
Folders → Personal Lua Plugins*, restart Wireshark and type `hidpp` in the
filter bar. What the messages mean is explained in [PROTOCOL.md](PROTOCOL.md).
