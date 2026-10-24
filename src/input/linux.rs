//! Input injection on Linux through a `uinput` virtual device.
//!
//! The kernel exposes `/dev/uinput`: writing `input_event` records to it makes
//! a virtual device that the desktop treats like any physical keyboard or
//! mouse. Access usually requires a udev rule (see the README).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::time::Duration;

use super::keys::{char_to_key, lookup, KEYS};
use super::{Injector, Key, MouseButton};

// ioctl request numbers from <linux/uinput.h> (`_IO`/`_IOW` on 'U').
const UI_DEV_CREATE: u64 = 0x5501;
const UI_DEV_DESTROY: u64 = 0x5502;
const UI_DEV_SETUP: u64 = 0x405C_5503; // _IOW('U', 3, struct uinput_setup)
const UI_SET_EVBIT: u64 = 0x4004_5564;
const UI_SET_KEYBIT: u64 = 0x4004_5565;
const UI_SET_RELBIT: u64 = 0x4004_5566;

// Event types and codes from <linux/input-event-codes.h>.
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const SYN_REPORT: u16 = 0;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_HWHEEL: u16 = 0x06;
const REL_WHEEL: u16 = 0x08;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;
const BUS_VIRTUAL: u16 = 0x06;

/// `struct uinput_setup` from <linux/uinput.h>.
#[repr(C)]
struct UinputSetup {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; 80],
    ff_effects_max: u32,
}

/// A virtual keyboard + mouse named "logirev virtual input".
pub struct Uinput {
    file: File,
}

impl Uinput {
    pub fn new() -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("cannot open /dev/uinput ({e}); see README, section Linux permissions"),
                )
            })?;
        let fd = file.as_raw_fd();
        let ioctl = |request: u64, arg: libc::c_ulong| -> io::Result<()> {
            // SAFETY: valid fd and request numbers taken from the kernel headers.
            if unsafe { libc::ioctl(fd, request as _, arg) } < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        };

        ioctl(UI_SET_EVBIT, EV_KEY.into())?;
        for key in KEYS {
            ioctl(UI_SET_KEYBIT, key.evdev.into())?;
        }
        for btn in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_SIDE, BTN_EXTRA] {
            ioctl(UI_SET_KEYBIT, btn.into())?;
        }
        // Relative axes make desktops classify the device as a mouse too, so
        // injected clicks are honoured.
        ioctl(UI_SET_EVBIT, EV_REL.into())?;
        for axis in [REL_X, REL_Y, REL_HWHEEL, REL_WHEEL] {
            ioctl(UI_SET_RELBIT, axis.into())?;
        }

        let mut setup = UinputSetup {
            bustype: BUS_VIRTUAL,
            vendor: 0x046D,
            product: 0x0001,
            version: 1,
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name = b"logirev virtual input";
        setup.name[..name.len()].copy_from_slice(name);
        ioctl(UI_DEV_SETUP, &setup as *const UinputSetup as libc::c_ulong)?;
        ioctl(UI_DEV_CREATE, 0)?;
        // Give the compositor/X server a moment to pick the device up, or the
        // very first injected events could be dropped.
        std::thread::sleep(Duration::from_millis(200));
        Ok(Self { file })
    }

    fn emit(&mut self, kind: u16, code: u16, value: i32) -> io::Result<()> {
        // SAFETY: input_event is plain old data; zeroed is a valid timestamp
        // (the kernel fills in the real one).
        let mut ev: libc::input_event = unsafe { std::mem::zeroed() };
        ev.type_ = kind;
        ev.code = code;
        ev.value = value;
        // SAFETY: we view the struct as its raw bytes for the duration of the write.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &ev as *const libc::input_event as *const u8,
                size_of::<libc::input_event>(),
            )
        };
        self.file.write_all(bytes)
    }

    fn press(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.emit(EV_KEY, code, i32::from(down))?;
        self.emit(EV_SYN, SYN_REPORT, 0)
    }
}

impl Injector for Uinput {
    fn key(&mut self, key: &Key, down: bool) -> io::Result<()> {
        self.press(key.evdev, down)
    }

    fn scroll(&mut self, vertical: i32, horizontal: i32) -> io::Result<()> {
        if vertical != 0 {
            self.emit(EV_REL, REL_WHEEL, vertical)?;
        }
        if horizontal != 0 {
            self.emit(EV_REL, REL_HWHEEL, horizontal)?;
        }
        self.emit(EV_SYN, SYN_REPORT, 0)
    }

    fn text(&mut self, text: &str) -> io::Result<()> {
        // Characters are typed as US-layout key presses (see `char_to_key`).
        let shift = lookup("Shift").expect("Shift is in the key table");
        for c in text.chars() {
            let Some((key, shifted)) = char_to_key(c) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("cannot type '{c}' on Linux (only US keyboard characters)"),
                ));
            };
            if shifted {
                self.press(shift.evdev, true)?;
            }
            self.press(key.evdev, true)?;
            self.press(key.evdev, false)?;
            if shifted {
                self.press(shift.evdev, false)?;
            }
        }
        Ok(())
    }

    fn button(&mut self, button: MouseButton, down: bool) -> io::Result<()> {
        let code = match button {
            MouseButton::Left => BTN_LEFT,
            MouseButton::Right => BTN_RIGHT,
            MouseButton::Middle => BTN_MIDDLE,
            MouseButton::Back => BTN_SIDE,
            MouseButton::Forward => BTN_EXTRA,
        };
        self.press(code, down)
    }
}

impl Drop for Uinput {
    fn drop(&mut self) {
        // SAFETY: valid fd; destroying the virtual device on exit is best effort.
        unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY as _, 0) };
    }
}
