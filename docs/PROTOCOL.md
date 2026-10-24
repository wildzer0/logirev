# How logirev talks to your device

This page explains, without assuming any programming knowledge, what logirev
actually sends to a Logitech device and why. The examples use real values
from an **MX Ergo S** paired to a **Logi Bolt** receiver (slot 2).

---

## 1. The big picture

```text
 ┌────────────┐   USB   ┌──────────────┐  radio  ┌───────────────┐
 │  Computer  │◀───────▶│ Bolt receiver│◀───────▶│  MX Ergo S    │
 │ (logirev)  │         │  ("dongle")  │         │  (slot 2)     │
 └────────────┘         └──────────────┘         └───────────────┘
```

- The receiver is a small **switchboard** with up to **6 slots**: up to six
  devices can be paired to it, each in its own numbered slot (1–6).
- Two kinds of data travel over the same cable:
  1. **normal input** — "the ball moved 3 dots right", "left button down".
     Windows/Linux handle this by themselves, no software needed;
  2. **configuration messages** in Logitech's own language, **HID++**.
     This is what Logi Options+ — and logirev — use.

---

## 2. What a HID++ message looks like

Every message is a short row of bytes (numbers from `00` to `FF`, written in
hexadecimal). There are two sizes: **short** (7 bytes) and **long** (20
bytes). The first four bytes always have the same meaning:

```text
  11      02      0A        3A        00 53 03 00 00 ...
  │       │       │         │         └─ parameters (what to do exactly)
  │       │       │         └─ function number (3) + "signature" (A)
  │       │       └─ which feature of the device (feature number 10)
  │       └─ which slot (device 2; FF = the receiver itself)
  └─ message size: 10 = short, 11 = long
```

**Features.** A device does not have a fixed list of commands. Instead it
offers a list of *features*, each with a world-wide id. For example:

| Feature id | Name | What it is for |
|---|---|---|
| `0x0000` | Root | "which features do you have?", "are you there?" |
| `0x0005` | DeviceNameType | name ("MX Ergo S") and type (trackball) |
| `0x1004` | UnifiedBattery | battery level |
| `0x1B04` | ReprogControlsV4 | the buttons, and taking them over |
| `0x2201` | AdjustableDpi | sensor resolution (speed) |

Each device stores its features in its own order, so the *number* used in
messages (byte 3) changes from model to model. On the MX Ergo S,
`ReprogControlsV4` is number **10** (`0A`) and `AdjustableDpi` is number
**15** (`0F`). logirev asks for this list every time a device connects —
that is what `logirevctl scan` prints under *features*.

**The signature.** The low half of byte 4 is a "signature" chosen by the
program that sends the question (logirev uses `A`). The device copies it into
its answer, so logirev knows the answer is meant for it. Messages the device
sends **on its own** (a button press, a battery change) carry signature `0`.

---

## 3. A conversation, step by step

Arrows: `→` computer to device, `←` device to computer.

### Step 1 — "A device has connected"

When the trackball wakes up, the **receiver** tells the computer:

```text
← 10 02 41 ...        slot 2: link established
```

Every time this message arrives, logirev starts the steps below. This is the
whole reason logirev must keep running: the device **forgets every setting**
when it sleeps, so the settings must be sent again on each connection.

### Step 2 — "Are you there? Which protocol do you speak?"

```text
→ 11 02 00 1A 00 00 5A          Root (00), function 1, signature A, marker 5A
← 11 02 00 1A 04 05 5A          "HID++ version 4.5", marker 5A echoed back
```

### Step 3 — "Where is feature 0x1B04?"

```text
→ 11 02 00 0A 1B 04             Root, function 0: look up feature 1B04
← 11 02 00 0A 0A ...            "it is my feature number 10"
```

logirev repeats this for every feature and learns the device's whole list.

### Step 4 — Applying settings (example: 400 DPI)

```text
→ 11 02 0F 3A 00 01 90          AdjustableDpi (15), function 3 "set DPI",
                                sensor 0, value 0190 (hex) = 400 DPI
← 11 02 0F 3A 00 01 90          the device confirms by echoing
```

### Step 5 — "Diverting" a button

To remap the *Back* button (control id `0053`), logirev asks the device to
stop handling it by itself and to *report* it instead:

```text
→ 11 02 0A 3A 00 53 03          ReprogControls (10), function 3,
                                button 0053, flags 03 = "divert: yes"
← 11 02 0A 3A 00 53 03          confirmed
```

From now on pressing Back does **nothing** on the computer by itself...

### Step 6 — ...instead the device reports it

```text
← 11 02 0A 00 00 53 00 00 ...   signature 0 = spontaneous message:
                                "buttons held now: 0053"
← 11 02 0A 00 00 00 00 00 ...   "buttons held now: none" (released)
```

logirev compares the list with the previous one: `0053` appeared → **press**;
disappeared → **release**. It then performs your action, e.g. it presses
`Alt+Left` on a virtual keyboard (Windows `SendInput`, Linux `uinput`).

### Step 7 — When logirev stops

It sends Step 5 again with flags `02` ("divert: no") for every button, so the
device behaves normally again.

---

## 4. Pairing a new device

The receiver keeps a list of up to six paired devices. Adding one is also
done with configuration messages, this time addressed to the receiver
itself (slot `FF`) and its *registers* (numbered settings of the receiver):

**Unifying receivers** have a "door" that can be opened for 30 seconds:

```text
→ 10 FF 80 B2 01 00 1E        register B2: open the door (1E = 30 seconds)
← 10 FF 4A 01 00 ...          "door open"
← 10 03 41 ...                a device connected in slot 3: the new one
```

**Bolt receivers** are more careful, because a radio is involved and a
stranger nearby could otherwise pair their keyboard with your computer:

1. *Look around* — register `C0` starts a search; every device in pairing
   mode answers (`4F` messages) with its radio address, type and name.
2. *Ask to pair* — register `C1` with the chosen device's address.
3. *Prove it* — the receiver makes up a code (`4D` message) that must be
   entered **on the new device**: typed on a keyboard, or as a sequence of
   left/right clicks on a mouse (the code written in binary: 0 = left,
   1 = right). Only someone holding the device can do that.
4. *Result* — a `54` message says "paired in slot N".

Removing a device writes action `03` and the slot number to the same
registers (`B2` for Unifying, `C1` for Bolt).

## 5. Errors

If a question cannot be answered, the reply has `FF` (or `8F` from the
receiver) in byte 3, followed by the question and an error code:

```text
← 11 02 FF 0F 3A 02             error 02 "InvalidArgument" for feature 15,
                                function 3 (e.g. a DPI value not supported)
```

logirev writes these in plain words in its log, e.g.
`cannot set DPI: HID++ 2.0 error 0x02 (InvalidArgument)`.

---

## 6. Reading a capture in Wireshark

1. In `logirev.toml` set `pcap = "logirev.pcap"` (or run with
   `--pcap logirev.pcap`), start logirev, use the device, stop logirev.
2. Open `logirev.pcap` with Wireshark. Each line is one message:
   *Source/Destination* tell the direction, the bottom pane shows the bytes
   (look for `11 02 ...`).
3. Optional: install `tools/hidpp.lua` (see the README) and type `hidpp` in
   the filter bar. The *Info* column then reads, for example,
   `<- dev 0x02  Buttons held: 0x0053`.

---

## 7. Glossary

| Term | Meaning |
|---|---|
| **HID** | the standard way keyboards and mice talk to computers |
| **HID++** | Logitech's extra language on top of HID, for configuration |
| **Receiver / dongle** | the USB stick; Bolt and Unifying have 6 slots |
| **Slot / device index** | number 1–6 of a paired device; `FF` = the receiver |
| **Feature** | a group of related functions offered by a device |
| **Function** | one command of a feature (e.g. "get DPI" = 2, "set DPI" = 3) |
| **Control id (CID)** | number identifying a button, e.g. `0053` = Back |
| **Divert** | ask the device to report a button to software instead of acting |
| **Notification** | a message the device sends by itself (signature `0`) |
| **Software id / signature** | marker telling whose question an answer belongs to |
| **Volatile setting** | a setting lost when the device sleeps or switches off |
