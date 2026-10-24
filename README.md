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
- Remap buttons to **shortcuts**, **mouse clicks** or **programs**.
- A **slow mode** button for precise work.
- Set **DPI / speed** and the **scroll wheel**.
- A **log file** that never grows beyond 1 MB, and an optional recording of
  the traffic that you can open in **Wireshark**.

> **Status:** early version (0.2). Reading devices is verified on an
> MX Ergo S with a Bolt receiver on Windows. A tray icon and "start with
> Windows" are coming in the next version — see [What's next](#whats-next).

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

3. **Start logirev:** double-click `logirev.exe`. A window opens and shows
   what logirev is doing. **Keep this window open** (you can minimise it):
   closing it stops logirev and your buttons go back to normal.

   The first time, logirev creates **`logirev.toml`** — your settings — in
   the same folder, already prepared for the MX Ergo / MX Ergo S.

4. **Start it automatically at log-in** (until the tray icon arrives):
   press `Win+R`, type `shell:startup`, press Enter. In the folder that opens,
   right-click → *New → Shortcut* → choose `logirev.exe`. Then right-click the
   new shortcut → *Properties* → *Run:* **Minimised**.

---

## Getting started on Linux

1. Unpack the archive and open a terminal in that folder.
2. Allow your user to talk to the receiver and to simulate key presses:

   ```bash
   sudo cp tools/42-logirev.rules /etc/udev/rules.d/
   sudo udevadm control --reload && sudo udevadm trigger
   ```

   Unplug and re-plug the receiver, then log out and back in.
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

**What happens when I close logirev?** It gives the buttons back to the
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
| `logirev is already running` | logirev is already open (maybe minimised, or started at log-in). |
| The device is not listed by `scan` | It is asleep: move it or press a button, then retry. |
| `button ... cannot be remapped` | That button cannot be changed on this device; pick another one. |
| A remapped button does nothing | Start with `logirev --log-level debug` and look for `button ... pressed` in the window. |
| Shortcuts do not reach a program on Windows | Windows blocks simulated keys in programs started "as administrator", unless logirev is started as administrator too. |
| Linux: `cannot open /dev/uinput` | Install the permission rules and log in again. |

If you report a problem, attach `logirev.log` and, if possible, a capture:
start logirev with `--pcap logirev.pcap`, reproduce the problem, stop it.

---

## What's next

1. **Tray icon** near the clock with your devices, battery levels and a
   *Start with Windows* option — no more window to keep open.
2. **Battery warnings** and **per-application buttons** (e.g. tilt changes
   tabs only in the browser).
3. **More actions:** scrolling, switching computer (Easy-Switch), simple
   macros, gestures.
4. **More devices:** SmartShift and thumb wheel (MX Master), keyboard
   backlight and Fn keys.

---

## More documentation

| Document | For whom |
|---|---|
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | everyone: every setting explained |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | the curious: how logirev talks to the device |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | developers: building, code layout, releasing |
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
