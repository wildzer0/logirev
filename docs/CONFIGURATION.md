# Configuration reference

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
| `disable` | the button does nothing | `{ action = "disable" }` |
| `default` | normal behaviour (same as not listing the button) | `{ action = "default" }` |

A program with arguments:
`{ action = "command", command = "notepad.exe", args = ["C:\\notes.txt"] }`
— in Windows paths write `\\` for every `\`.

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

## 6. Wireshark capture

Set `pcap = "logirev.pcap"` (or start with `--pcap logirev.pcap`) to record
every message exchanged with the devices. Open the file with
[Wireshark](https://www.wireshark.org). For readable decoding, copy
`tools/hidpp.lua` into the folder shown under *Help → About Wireshark →
Folders → Personal Lua Plugins*, restart Wireshark and type `hidpp` in the
filter bar. What the messages mean is explained in [PROTOCOL.md](PROTOCOL.md).
