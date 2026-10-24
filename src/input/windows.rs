//! Input injection on Windows through `SendInput`.

use std::io;
use std::mem::size_of;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput as send_input, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
};

use super::{Injector, Key, MouseButton};

/// `XBUTTON1` / `XBUTTON2` (side buttons) as expected in `mouseData`.
const XBUTTON1: u32 = 0x0001;
const XBUTTON2: u32 = 0x0002;

/// Stateless wrapper around `SendInput`.
pub struct SendInput;

impl SendInput {
    fn send(input: INPUT) -> io::Result<()> {
        // SAFETY: `input` is a fully initialised INPUT and we pass its exact size.
        let sent = unsafe { send_input(1, &input, size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

impl Injector for SendInput {
    fn key(&mut self, key: &Key, down: bool) -> io::Result<()> {
        let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
        if key.extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        Self::send(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key.vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }

    fn button(&mut self, button: MouseButton, down: bool) -> io::Result<()> {
        let (flags, data) = match (button, down) {
            (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
            (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
            (MouseButton::Back, false) => (MOUSEEVENTF_XUP, XBUTTON1),
            (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
            (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, XBUTTON2),
        };
        Self::send(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }
}
