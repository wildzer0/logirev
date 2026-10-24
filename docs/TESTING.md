# Manual test plan

Automatic tests (`cargo test`) cover parsing, configuration and protocol
decoding. Everything that needs real hardware or a real Windows desktop is
checked by hand with this list before a release. Each test says what to do
and what you should see. Write down the number of any test that fails and
attach `logirev.log` (start logirev with `level = "debug"` in
`[logging]` for these tests).

Setup: Windows, Logi Options+ closed, the logirev folder in a normal Windows
folder (e.g. `C:\Tools\logirev\`, not a `\\wsl.localhost` path).

## A. Start, icon, window

| # | Do | Expect |
|---|---|---|
| A1 | Double-click `logirev.exe` | No console window; a blue round icon appears next to the clock (or under `^`). |
| A2 | Hover the icon | Tooltip `logirev — MX Ergo S 30%` (your devices and battery). |
| A3 | Left-click the icon | The devices window opens: every paired slot, connected ones with battery, DPI, profile, number of remapped buttons. |
| A4 | Close the window, right-click the icon | Menu: logirev version, receiver, one line per slot (✓ = connected), commands. |
| A5 | Double-click `logirev.exe` again | Message "logirev is already running". |
| A6 | Task Manager → Details → `logirev.exe` | Memory under ~10 MB, CPU 0 % while idle. |

## B. Plug & play

| # | Do | Expect |
|---|---|---|
| B1 | Leave the trackball idle until it sleeps, then move it | Log: `disconnected`, then `connected ... applying profile`; buttons work again within a second. |
| B2 | Unplug the receiver | Log: `lost connection`; tooltip `no receiver`. |
| B3 | Plug it back | Within about a second: `using Bolt receiver`, device configured again. |
| B4 | Switch the trackball off and on | As B1. |

## C. Buttons (MX Ergo S profile of the default configuration)

| # | Do | Expect |
|---|---|---|
| C1 | In a browser press Back / Forward | Previous / next page (`Alt+Left` / `Alt+Right`). |
| C2 | Tilt the wheel left / right | Previous / next browser tab. |
| C3 | Press the button below the wheel, move, press again | Pointer slow (400 DPI), then normal again. Log: `slow mode on/off`. |
| C4 | Middle click | Normal middle click (not remapped). |
| C5 | Right-click the icon → *Quit* | Back/Forward behave as normal mouse buttons again. |

## D. Settings file

| # | Do | Expect |
|---|---|---|
| D1 | Menu → *Edit settings* | `logirev.toml` opens in an editor (Notepad if nothing else). |
| D2 | Change `back` to `{ action = "keys", keys = "Ctrl+C" }`, save | Within ~2 s log `configuration reloaded`; Back now copies. |
| D3 | Write `keys = "Altt+Left"`, save | Notification "Settings not applied" naming the mistake; the old settings keep working. |
| D4 | Fix the mistake, save | Reloaded, no notification. |
| D5 | Menu → *Reload settings* | Log `configuration reloaded`. |
| D6 | Menu → *Open log* / *Open logirev folder* | The log opens / Explorer opens the folder. |

## E. Start with Windows

| # | Do | Expect |
|---|---|---|
| E1 | Menu → *Start with Windows* (tick it) | Menu shows the tick next time. |
| E2 | Sign out and in again | The icon appears by itself. |
| E3 | Untick it, sign out and in | logirev does not start. |

## F. Per-program buttons

Un-comment the `[[profile.app]]` example for `explorer.exe` in
`logirev.toml` and save.

| # | Do | Expect |
|---|---|---|
| F1 | In File Explorer press Back | Goes up one folder (`Alt+Up`). |
| F2 | Tilt the wheel in Explorer | Scrolls sideways (where possible). |
| F3 | Switch to the browser, press Back / tilt | Browser back / change tab again (profile buttons). |
| F4 | Log with `level = "debug"` | `foreground program: explorer` / `chrome` lines when switching. |

## G. New actions

Change buttons one at a time (save after each) and try them.

| # | Setting | Expect |
|---|---|---|
| G1 | `tilt_right = { action = "scroll", direction = "down", amount = 3 }` | Each tilt scrolls 3 notches down. |
| G2 | `precision = { action = "cycle-dpi", dpi = [400, 800, 1600] }` | Each press changes speed; window shows the DPI. |
| G3 | `back = { action = "sequence", steps = [ { text = "Hello" }, { delay = 300 }, { keys = "Enter" } ] }` | In Notepad types "Hello", waits, presses Enter. |
| G4 | `forward = { action = "command", command = "calc.exe" }` | Calculator opens. |
| G5 | `middle = { action = "change-host", host = "next" }` (only if the trackball is paired to a second computer) | The trackball switches to the other computer. |
| G6 | `logirevctl scan`: is any button marked **gestures**? If yes, give it a `gesture` action (see CONFIGURATION.md) | Hold + move up/down/left/right runs the four actions; a press without moving runs `click`. |

## H. Battery

| # | Do | Expect |
|---|---|---|
| H1 | Set `battery_warning = 50` (above the current level), save | A "Battery low" notification appears once. |
| H2 | Save again without changes / let the device sleep and wake | No repeated notification. |
| H3 | Set it back to 15 | — |

## I. Keyboard (Wave Keys)

| # | Do | Expect |
|---|---|---|
| I1 | `logirevctl scan` | The keyboard shows *function keys* and/or *disable keys* lines if it supports them. |
| I2 | In the "Keyboard" profile set `disable_keys = ["capslock"]` | Caps Lock does nothing. |
| I3 | Set `function_keys = "special"` then `"standard"` | F1–F12 switch between media and F-key behaviour. |

## J. Command-line tools

| # | Do | Expect |
|---|---|---|
| J1 | `logirevctl scan` while logirev runs | Works; lists all slots including sleeping ones. |
| J2 | Quit logirev; `logirevctl run --log-level debug --pcap test.pcap`, use the buttons, Ctrl+C | Live log in the terminal; `test.pcap` opens in Wireshark. |
| J3 | With `tools/hidpp.lua` installed in Wireshark | Packets decoded as HID++ (filter `hidpp`). |
