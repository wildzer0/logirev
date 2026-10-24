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
| `pointer_speed` | `0.18` … `2.0` (1.0 = normal) | only devices that have the *PointerSpeed* feature (the MX Ergo S does not). |
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
| `macro` | runs a macro defined in `[macros]` (see below); `repeat = 100` repeats it every 100 ms while held | `{ action = "macro", name = "signature" }` |
| `sequence` | several steps in a row (a simple macro, see below) | `{ action = "sequence", steps = [ { keys = "Ctrl+C" } ] }` |
| `gesture` | hold the button and move the pointer (see below) | `{ action = "gesture", up = { ... } }` |
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
