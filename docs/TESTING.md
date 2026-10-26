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
| A3 | Left-click the icon | The settings window opens on *Devices*: a card for every paired slot, connected ones with battery and remapped buttons. (In a build without the window, `--no-default-features`, the native device list opens instead.) |
| A4 | Close the window, right-click the icon | Menu: logirev version, receiver, one line per slot (✓ = connected), commands. |
| A5 | Double-click `logirev.exe` again | The settings window opens (or comes to the front); only one tray icon and one service remain. In a build without the window it shows "logirev is already running". |
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

## K. Brightness, backlight, Easy-Switch, macros (0.4)

| # | Do | Expect |
|---|---|---|
| K1 | `tilt_left = { action = "brightness", change = -10 }`, `tilt_right = ... change = 10` | Each tilt makes the screen(s) darker / brighter. If nothing happens on an external monitor, enable DDC/CI in its menu. |
| K2 | Menu → *Send MX Ergo S to ▸ Computer 2* (device paired to a second computer) | The trackball moves to the other computer; back with its Easy-Switch button. |
| K3 | `middle = { action = "change-host", host = 2, all = true }` | Trackball **and** keyboard go to computer 2 together. |
| K4 | `logirevctl scan` | *computer names* line lists the paired computers. |
| K5 | Add `[macros.sig]` with `steps = [ { text = "Hi" }, { keys = "Enter" } ]` and `back = { action = "macro", name = "sig" }` | In Notepad Back types "Hi" + Enter. |
| K6 | `forward = { action = "macro", name = "sig", repeat = 300 }`, hold Forward 2 s | "Hi" is typed repeatedly until you release. |
| K7 | Macro with `{ press = "Shift" }, { click = "left" }` (no release) | Shift is released at the end (no stuck Shift). |
| K8 | Backlit keyboard only: `backlight = { mode = "manual", level = 2 }` and a `backlight` action | Light level changes. |

## L. Settings window (1.0)

| # | Do | Expect |
|---|---|---|
| L1 | First start: unzip into a new folder, double-click `logirev.exe` | Tray icon appears **and** the window opens on *Devices*. |
| L2 | Close the window, double-click `logirev.exe` again | The window opens again (no "already running" message); only one tray icon. |
| L3 | Left-click the tray icon | The window opens (or comes to the front if already open). |
| L4 | *Devices* page | Cards for each paired slot; connected ones show battery icon and %; asleep ones are faded. |
| L5 | Click *MX Ergo S* → *Buttons* → *Change* on Back → *Keyboard shortcut* → *Record*, press Ctrl+Shift+T → *Save* | Row shows "Ctrl + Shift + T"; within ~2 s Back reopens the last closed browser tab. |
| L6 | *Pointer & wheel*: untick "Keep the device's own value", move the DPI slider | Pointer speed changes a second later. |
| L7 | *Per program*: pick a running program, change a button for it | Works only while that program is in front. |
| L8 | *Computers* (device paired to 2 computers): *Switch* | Device moves to the other computer. |
| L9 | *Macros*: *New macro*, edit steps; assign it to a button with the *Macro* action | The button runs the steps. |
| L10 | *Settings*: Italiano / English, Light / Dark | The window changes language / look immediately. |
| L11 | *Settings*: tick *Start with Windows*, sign out and in | Tray icon appears, **no** window opens. |
| L12 | *Settings*: *Stop service* | Window shows "Service not running" with *Start*; *Start* brings it back. |
| L13 | Quit from the tray while the window is open | The window switches to "Service not running". |
| L14 | Task Manager | `logirev.exe` (service) a few MB; the window process exits when closed. |
| L15 | Explorer | `logirev.exe` shows the new round blue icon. |

## M. Updates

To test, run a build whose version in `Cargo.toml` is *lower* than the
latest published release (e.g. set `version = "0.9.0"` locally).

| # | Do | Expect |
|---|---|---|
| M1 | Open the window | *Devices* shows "Version X is available" with *Update now* / *Later*. |
| M2 | Wait ~1 minute with the window closed | A notification "Update available". |
| M3 | *Settings → Updates → What's new* | The release notes. |
| M4 | *Update now* | "Downloading and installing...", then the window closes and reopens; the tray icon comes back; *About* shows the new version. |
| M5 | Explorer, logirev folder | `logirev.exe` replaced; `logirev.exe.old` disappears at the next start. |
| M6 | Untick *Check for updates automatically* | No more update notifications; *Check now* still works. |
| M7 | Read-only folder (e.g. under `C:\Program Files`) | *Update now* fails with a clear message; the old version keeps working. |

## N. Pairing (1.1)

| # | Do | Expect |
|---|---|---|
| N1 | *Devices* → *Pair a new device* next to the Bolt receiver | Assistant opens: "Press and hold the connect button..." with a spinner. |
| N2 | Hold the Easy-Switch button of a device paired elsewhere (or just unpaired) until it blinks fast | The device appears in the list with its name; *Pair*. |
| N3 | Keyboard | A 6-digit code appears; typing it + Enter → "Done! ... slot N"; the counter advances while typing. |
| N4 | Mouse / trackball | A sequence of 10 Left/Right boxes; clicking them turns them green; then left+right together → "Done!". |
| N5 | After pairing | The new device appears in the list and is configured (log `connected ... applying profile`). |
| N6 | *Cancel* during discovery | Assistant closes; the receiver stops listening. |
| N7 | Wait 30 s without a device | "The pairing did not work: no device answered in time" with *Try again*. |
| N8 | Right-click the duplicate *Wave Keys* card → *Remove pairing* → confirm | The slot disappears from the list (and from `logirevctl scan`). |
| N9 | `logirevctl unpair 3` / `logirevctl pair` (service running) | Same results from the terminal; the window updates. |
| N10 | Unifying receiver (if available) | *Pair a new device* → turn on the device → "Done!". |

## O. Developer mode

| # | Do | Expect |
|---|---|---|
| O1 | *Settings* → *Developer mode* → tick *Show technical details*; open the MX Ergo S | A *Technical* tab appears (after a moment: "Reading the technical details..."). |
| O2 | *Technical* tab | Serial number, unit id, model ids per connection, firmware components (application running, bootloader...), feature list with *hidden* / *engineering* labels. |
| O3 | Press a remapped button, change the DPI | New lines in *HID++ messages* within half a second; *Pause* freezes the list; *Only this device* hides other slots. |
| O4 | *Copy report*, paste in a text editor | JSON with the device and the recent messages. *Save report...* writes the same to a file. |
| O5 | Untick developer mode | The tab disappears; the service stops recording messages (no extra CPU). |
| O6 | `logirevctl scan --hidden` | Identity and firmware components per device, then "hidden features (unlocked for a moment, then locked again)" with the list, or "none beyond the list above". The device keeps working normally afterwards. |

## P. On-screen messages, tray battery, scrolling, backup

| # | Do | Expect |
|---|---|---|
| P1 | Press the precision-mode button | A dark box at the bottom of the screen: "Modalità precisione attiva · 400 DPI" for about a second; again → "...disattivata". It never steals the focus or blocks clicks. |
| P2 | *Settings → Battery and messages* → untick *Show changes on screen* | No box any more. |
| P3 | Tray icon | A small bar at the bottom of the icon: green above 50 %, amber above 20 %, red below; cyan while charging. Untick *Show the battery level...* → plain icon. |
| P4 | Charge the device until full | Notification "Carica completa / Fully charged". |
| P5 | *Settings → Scrolling* → speed 200 % | One wheel notch scrolls twice as far, in every program and with every mouse; *Normal* resets it. |
| P6 | Tick *Smooth scrolling* | Scrolling glides instead of jumping (best seen in a browser or Notepad). Untick both → the wheel is untouched (the hook is removed: log "wheel: back to normal"). |
| P7 | *Backup → Export settings...*, change something, *Import settings...* the exported file | "Settings imported and applied"; the change is undone. Importing a broken file shows the error and changes nothing. |

## J. Command-line tools

| # | Do | Expect |
|---|---|---|
| J1 | `logirevctl scan` while logirev runs | Works; lists all slots including sleeping ones. |
| J2 | Quit logirev; `logirevctl run --log-level debug --pcap test.pcap`, use the buttons, Ctrl+C | Live log in the terminal; `test.pcap` opens in Wireshark. |
| J3 | With `tools/hidpp.lua` installed in Wireshark | Packets decoded as HID++ (filter `hidpp`). |

## Q. Radial menus (Windows)

| # | Do | Expect |
|---|---|---|
| Q1 | Run `logirev.exe --radial-demo` (quit logirev first only if it complains) | A dark ring appears under the pointer with six example items, icons and the title "logirev"; it fades in, does not steal the focus; clicking a sector or the X closes it. |
| Q2 | Hover the sectors | The sector under the pointer turns cyan/blue; labels stay readable; the X in the middle is visible. |
| Q3 | *Radial menus* → *New menu* → name it `work` → add two items: "Calculator" (`command = calc.exe`) and "Next desktop" (`desktop`, next); assign it to Back in the MX Ergo page (*Buttons* → Back → *Radial menu* → `work`) | Within a second Back opens the menu under the pointer; clicking "Calculator" opens it; clicking "Next desktop" switches desktop; clicking outside or the X closes without doing anything. |
| Q4 | Set the item icon (e.g. `code`), rename the menu, drag a sector onto another, delete one | The menu reflects the changes after the automatic save; renaming also updates the button that opens it. |
| Q5 | Assign a radial menu to a keyboard button by hand in `logirev.toml` | The log warns that only pointing devices can open it and the button does not open anything. |
| Q6 | Ten items; use a long label | All sectors are visible; the long label is shortened with `…`; no sector overlaps. |
| Q7 | Display at 150 %/200 % and a monitor at the right/bottom edge | The ring is sharp and stays completely on screen (it shifts away from the edge). |
| Q8 | `logirev.exe --radial-demo` on Linux (if built with the window) | The program exits with "the radial menu is only available on Windows". |
| Q9 | Test with the "Desktop next/previous" preset in the button editor | On Windows Win+Ctrl+←/→ switches virtual desktop; on Linux Ctrl+Alt+←/→ (X11). |
| Q10 | Assign the same menu to `tilt_left` and to the middle button | Both open the menu: it works on **any** configurable button of the pointing device, not just one dedicated key. |
| Q11 | Press and hold the button, move over a sector, release | The item runs immediately; releasing in the middle keeps the menu open; releasing outside the ring closes it. |
| Q12 | Quick click on the button (release in less than ~0.2 s) | The menu stays open for a click instead of choosing a sector by accident. |
| Q13 | *Radial menus* page with several menus | The menus are a compact list; clicking a header opens only that menu and collapses the previous one, so the last menu is reachable without scrolling through all details. The open menu shows the live wheel (`n/10`); clicking a sector edits it and dragging a sector onto another moves it. Item rows are a minimal icon + label + action line with small edit/delete buttons. |
| Q14 | Choose "Back", "Forward" and a middle click from the menu in a browser / editor | Each action runs after the left button is released; no action starts while the mouse button is still down. |
| Q15 | Add items up to the limit | At ten the *Add item* button is replaced by "A menu can have at most ten items"; the configuration refuses an eleventh item too. |
| Q16 | Add the *Lock the PC* preset to a menu (or a button) and choose it | Windows locks at once; on Linux the session manager or screen locker locks. Unlike the old `Win+L` simulation, it works from a simulated action. |
| Q17 | Open the menu and click on another window, on the desktop, or on the transparent shadow around the wheel | The menu closes without choosing anything; the click still reaches the window under the pointer. |
