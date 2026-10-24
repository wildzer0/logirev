# logirev

**A tiny program that sets up your Logitech mouse, trackball or keyboard —
without installing Logi Options+.**

It works with devices connected through a **Logi Bolt** or **Unifying**
receiver (the small USB "dongle"), and with devices connected directly by
cable or Bluetooth. It was built and tested with an **MX Ergo S** trackball.

- **Portable:** two small programs and one text file. No installation, no
  account, no internet, no telemetry. Works from a USB stick.
- **Light:** about 3 MB of memory and no noticeable CPU use.
- **Plug & play:** plug in the receiver or wake up the device and its
  settings are applied by themselves.
- Remap buttons to **shortcuts**, **mouse clicks**, **scrolling**,
  **programs**, **macros** (also repeating while held), **gestures** (hold a
  button and move), **screen brightness**, **keyboard backlight** or
  **switching computer** (Easy-Switch) — one device or all of them together.
- **Easy-Switch from the icon menu:** *Send MX Ergo S to ▸ Computer 2*.
- **Different buttons in different programs** (Windows), e.g. the side
  buttons do *undo / redo* only in Photoshop.
- A **slow mode** button for precise work, or a button that **cycles DPI**.
- Set **DPI / speed**, the **scroll wheel**, **SmartShift**, the **thumb
  wheel**; on keyboards the **backlight**, the **Fn keys** and **disabling
  Caps Lock** & co.
- An **icon next to the clock** (Windows) showing your devices and their
  battery, with a *Start with Windows* option, and a **warning when a
  battery is low**.
- A **log file** that never grows beyond 1 MB, and an optional recording of
  the traffic that you can open in **Wireshark**.

> **Status:** version 0.3. Developed with an MX Ergo S on a Bolt receiver on
> Windows; features of other devices (MX Master wheels, keyboards) follow
> Logitech's protocol but have not been tried on real hardware yet.

---

## Contents

1. [Download](#download)
2. [Getting started on Windows](#getting-started-on-windows)
3. [Getting started on Linux](#getting-started-on-linux)
4. [Changing the settings](#changing-the-settings)
5. [Questions and answers](#questions-and-answers)
6. [When something does not work](#when-something-does-not-work)
7. [What's next](#whats-next)
8. [More documentation](#more-documentation)

---

## Download

Open the **Releases** page of this repository and download:

- **Windows:** `logirev-<version>-windows-x64.zip`
- **Linux:** `logirev-<version>-linux-x64.tar.gz`

Unpack it into any folder you like, for example `C:\Tools\logirev\`. Inside
you will find:

| File | What it is |
|---|---|
| `logirev` (`logirev.exe`) | the program that runs in the background and keeps your devices set up |
| `logirevctl` (`logirevctl.exe`) | a helper to look at your devices and solve problems |
| `logirev.example.toml` | an example of the settings file |
| `README.md`, `docs/` | this guide and the detailed documentation |
| `LICENSE.md` | the terms of use (free for non-commercial use) |
| `tools/` | Wireshark plugin and (Linux) permission rules |

---

## Getting started on Windows

1. **Quit Logi Options+** if you have it (right-click its icon near the
   clock → *Quit*). Two programs cannot control the same device at the same
   time.

2. **Look at your devices.** Open the folder, click the address bar, type
   `cmd` and press Enter. In the black window type:

   ```
   logirevctl scan
   ```

   You will see your receiver, the devices paired to it (up to 6) and, for
   each device that is awake, what it can do:

   ```
   === Bolt receiver (046d:c548)

   [slot 2] MX Ergo S  (trackball, HID++ 4.5)
     firmware       RBM 26.01.B0013
     battery        30%
     dpi            750
     profile        "MX Ergo"
     buttons (config name, id, remappable, default task):
       middle           0x0052  yes, mouse button   ...
       back             0x0053  yes, mouse button   ...
       precision        0x00fd  yes, mouse button   ...
   ```

   A device that is asleep does not answer: move it and run the command again.

3. **Start logirev:** double-click `logirev.exe`. No window opens: a small
   blue round **icon appears next to the clock**. (If you do not see it,
   click the `^` arrow next to the clock: Windows may have hidden it there.
   You can drag it next to the clock to keep it always visible.)

   The first time, logirev creates **`logirev.toml`** — your settings — in
   the same folder, already prepared for the MX Ergo / MX Ergo S.

4. **Use the icon.** Hover it to see your connected devices and their
   battery. **Left-click** opens the *devices* window (all paired devices,
   connected or not, with battery, DPI and profile). **Right-click** opens
   the menu:

   | Menu item | What it does |
   |---|---|
   | *Slot 1, Slot 2...* | your paired devices; a tick ✓ means connected |
   | *Show devices...* | opens the devices window |
   | *Edit settings* | opens `logirev.toml` in your text editor |
   | *Reload settings* | applies the settings now (they also apply by themselves when you save) |
   | *Open log* | opens `logirev.log` |
   | *Open logirev folder* | opens the folder in Explorer |
   | *Start with Windows* | tick it to start logirev automatically when you log in |
   | *Quit* | stops logirev; your buttons go back to normal |

   When something needs your attention (for example a mistake in the
   settings file) a notification appears.

> Put the logirev folder somewhere permanent (e.g. `C:\Tools\logirev\`)
> before ticking *Start with Windows*. If you move it later, just start it
> once from the new place: the start-up entry follows it.

---

## Getting started on Linux

1. Unpack the archive and open a terminal in that folder.
2. Allow your user to talk to the receiver and to simulate key presses:

   ```bash
   sudo cp tools/42-logirev.rules /etc/udev/rules.d/
   sudo udevadm control --reload && sudo udevadm trigger
   ```

   To let the `brightness` action change a laptop screen, also run
   `sudo usermod -aG video $USER`. Unplug and re-plug the receiver, then log
   out and back in.
3. Quit Solaar if it is running.
4. Look at your devices: `./logirevctl scan`
5. Create the settings file and start at every log-in:

   ```bash
   ./logirevctl init
   ./logirevctl install-service
   systemctl --user daemon-reload && systemctl --user enable --now logirev.service
   ```

   To try it in the terminal instead: `./logirev` (Ctrl+C to stop).

---

## Changing the settings

Open `logirev.toml` with any text editor (Notepad is fine). It explains
itself with comments. Save the file and **the new settings are applied in a
couple of seconds** — no restart needed. If you make a typo, logirev keeps
the previous settings and tells you what is wrong in its window and in
`logirev.log`.

A typical profile looks like this:

```toml
[[profile]]
name = "MX Ergo"
match = { name = "MX Ergo", kind = "trackball" }   # which devices it is for
dpi = 750                                           # speed

[profile.buttons]
back      = { action = "keys", keys = "Alt+Left" }        # browser back
tilt_left = { action = "keys", keys = "Ctrl+Shift+Tab" }  # previous tab
precision = { action = "toggle-dpi", dpi = 400 }          # slow mode on/off
```

You can write several profiles — for example one for your trackball and one
for "any other mouse" — and each device gets the first profile that fits it.

Every setting, button name, action and key name is listed in
**[docs/CONFIGURATION.md](docs/CONFIGURATION.md)**.

---

## Questions and answers

**Why must logirev keep running? Can't the settings be saved in the device?**
Most office devices (MX Ergo S included) have no memory for your
preferences: each time they sleep or are switched off they return to factory
behaviour. logirev notices when the device comes back and sends the settings
again, within a fraction of a second. Logi Options+ does exactly the same.

**How many devices can I use?** A Bolt or Unifying receiver holds up to **6**
paired devices. logirev serves all of them, and several receivers at once.

**Does it slow my computer down?** No. It sleeps until a device sends
something; it uses about 3 MB of memory.

**Does a window have to stay open?** No. On Windows logirev lives in the
icon next to the clock; on Linux it runs as a background service.

**Can it start by itself when I log in?** Yes: tick *Start with Windows* in
the icon menu. On Linux, see step 5 of the Linux instructions.

**Can buttons do different things in different programs?** Yes (on
Windows): see [per-program buttons](docs/CONFIGURATION.md#6-different-buttons-in-different-programs-windows).

**Will it tell me when the battery is low?** Yes: a notification appears
when a battery drops to 15% (change it with `battery_warning`).

**What happens when I quit logirev?** It gives the buttons back to the
device, so everything works as without logirev.

**Can I use `logirevctl scan` while logirev is running?** Yes.

**Is it safe?** logirev only changes temporary settings. It never writes to
the device's permanent memory or firmware: switching the device off always
brings back the factory behaviour.

**How does it work?** In plain words, with real examples:
[docs/PROTOCOL.md](docs/PROTOCOL.md).

---

## When something does not work

| What you see | What to do |
|---|---|
| `No Logitech HID++ interface found` | Is the receiver plugged in? On Linux, install the permission rules (step 2). |
| `cannot open ...` | Quit Logi Options+ / Solaar. On Linux, check the permission rules. |
| `logirev is already running` | logirev is already open: look for its icon next to the clock (or behind the `^` arrow). |
| No icon appears | Click the `^` arrow next to the clock. If it is not there either, start `logirevctl run` in a terminal to see why. |
| The device is not listed by `scan` | It is asleep: move it or press a button, then retry. |
| `button ... cannot be remapped` | That button cannot be changed on this device; pick another one. |
| A remapped button does nothing | Quit logirev from its icon, then run `logirevctl run --log-level debug` in a terminal and look for `button ... pressed`. |
| Shortcuts do not reach a program on Windows | Windows blocks simulated keys in programs started "as administrator", unless logirev is started as administrator too. |
| Linux: `cannot open /dev/uinput` | Install the permission rules and log in again. |

If you report a problem, attach `logirev.log` and, if possible, a capture:
quit logirev, run `logirevctl run --pcap logirev.pcap`, reproduce the
problem, press Ctrl+C.

---

## What's next

Ideas for later versions:

- per-program buttons on Linux;
- editing buttons from the devices window instead of the text file.

---

## More documentation

| Document | For whom |
|---|---|
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | everyone: every setting explained |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | the curious: how logirev talks to the device |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | developers: building, code layout, releasing |
| [docs/TESTING.md](docs/TESTING.md) | testers: the checklist used before each release |
| [CHANGELOG.md](CHANGELOG.md) | what changed in each version |

---

## License

logirev is free for **personal and non-commercial use**: you may use it,
study it, change it and share it, as long as it is not for a commercial
purpose. Charities, schools, research and public institutions may use it too.
See [LICENSE.md](LICENSE.md) (PolyForm Noncommercial 1.0.0) for the exact
terms.

logirev is an independent project, not affiliated with or endorsed by
Logitech. "Logitech", "Logi", "Bolt", "Unifying" and "MX" are trademarks of
Logitech.
