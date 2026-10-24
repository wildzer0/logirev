//! Wheel speed and smooth scrolling for every mouse.
//!
//! A low-level mouse hook sees each wheel movement before programs do. When
//! the speed is not 100 % or smooth scrolling is on, the original movement
//! is swallowed and a worker thread sends the adjusted one (see
//! [`crate::input::wheel`]). Movements sent by programs (including logirev
//! itself) are marked "injected" and pass untouched, so nothing loops.
//!
//! The hook is installed only while needed, on the UI thread (which runs the
//! message loop Windows requires for it).

use std::cell::Cell;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use log::{info, warn};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, LLMHF_INJECTED, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
};

use crate::input::wheel::Wheel;

/// Pause between two steps of a glide.
const STEP: Duration = Duration::from_millis(10);

static PERCENT: AtomicU16 = AtomicU16::new(100);
static SMOOTH: AtomicBool = AtomicBool::new(false);
/// Wheel movements for the worker: `(axis, delta)`.
static WORKER: OnceLock<Mutex<Sender<(usize, i32)>>> = OnceLock::new();

thread_local! {
    static HOOK: Cell<HHOOK> = const { Cell::new(std::ptr::null_mut()) };
}

/// Applies the settings; installs or removes the hook as needed. Must be
/// called on the UI thread.
pub fn configure(percent: u16, smooth: bool) {
    PERCENT.store(percent.clamp(25, 500), Ordering::Relaxed);
    SMOOTH.store(smooth, Ordering::Relaxed);
    let wanted = percent != 100 || smooth;
    let hook = HOOK.with(Cell::get);
    if wanted && hook.is_null() {
        WORKER.get_or_init(|| Mutex::new(spawn_worker()));
        // SAFETY: a process-wide low-level hook whose procedure is a plain
        // function of this module.
        let h = unsafe {
            SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(hook_proc),
                GetModuleHandleW(std::ptr::null()),
                0,
            )
        };
        if h.is_null() {
            warn!(
                "cannot watch the mouse wheel: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        HOOK.with(|c| c.set(h));
        info!(
            "wheel: speed {percent}%, smooth {}",
            if smooth { "on" } else { "off" }
        );
    } else if !wanted && !hook.is_null() {
        // SAFETY: the hook was installed by this thread.
        unsafe { UnhookWindowsHookEx(hook) };
        HOOK.with(|c| c.set(std::ptr::null_mut()));
        info!("wheel: back to normal");
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let msg = wparam as u32;
    if code >= 0 && (msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL) {
        // SAFETY: for WH_MOUSE_LL, lparam points to an MSLLHOOKSTRUCT.
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        if info.flags & LLMHF_INJECTED == 0 {
            let delta = i32::from((info.mouseData >> 16) as u16 as i16);
            let axis = usize::from(msg == WM_MOUSEHWHEEL);
            let sent = WORKER
                .get()
                .and_then(|w| w.lock().ok())
                .is_some_and(|w| w.send((axis, delta)).is_ok());
            if sent {
                return 1; // swallowed: the worker sends the adjusted one
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// The thread that sends the adjusted wheel movements.
fn spawn_worker() -> Sender<(usize, i32)> {
    let (tx, rx) = mpsc::channel::<(usize, i32)>();
    let _ = std::thread::Builder::new()
        .name("wheel".into())
        .spawn(move || {
            let mut wheel = Wheel::default();
            loop {
                let next = if wheel.gliding() {
                    rx.recv_timeout(STEP)
                } else {
                    rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
                };
                match next {
                    Ok((axis, delta)) => {
                        let (percent, smooth) = (
                            PERCENT.load(Ordering::Relaxed),
                            SMOOTH.load(Ordering::Relaxed),
                        );
                        if let Some(units) = wheel.push(axis, delta, percent, smooth) {
                            send_wheel(axis, units);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        for (axis, units) in wheel.tick() {
                            send_wheel(axis, units);
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
    tx
}

fn send_wheel(axis: usize, units: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: units as u32,
                dwFlags: if axis == 0 {
                    MOUSEEVENTF_WHEEL
                } else {
                    MOUSEEVENTF_HWHEEL
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: one fully initialised INPUT of the size we pass.
    unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) };
}
