//! Knowing which program is in front, for per-application buttons.
//!
//! * **Windows** — a "WinEvent" hook tells us each time another window comes
//!   to the foreground (no polling). The program's file name (e.g.
//!   `chrome.exe`) is sent to the daemon as [`Event::Foreground`].
//! * **Other systems** — not available; profiles use their default buttons.
//!
//! The hook runs on its own small thread with a message loop, so it works
//! both for the tray application and for `logirevctl run` in a terminal.

use std::sync::mpsc::Sender;

use crate::daemon::Event;

/// Starts watching the foreground window. Calling it again does nothing.
pub fn watch(tx: Sender<Event>) {
    imp::watch(tx);
}

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::Sender;
    use std::sync::OnceLock;

    use log::{debug, warn};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
        TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG, WINEVENT_OUTOFCONTEXT,
        WINEVENT_SKIPOWNPROCESS,
    };

    use crate::daemon::Event;

    /// Where the hook callback (a plain function) sends its results.
    static SENDER: OnceLock<Sender<Event>> = OnceLock::new();

    pub fn watch(tx: Sender<Event>) {
        if SENDER.set(tx).is_err() {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("foreground".into())
            .spawn(|| {
                // SAFETY: standard out-of-context WinEvent hook; the callback is
                // a `extern "system"` function with the documented signature and
                // the hook lives as long as this thread's message loop.
                unsafe {
                    let hook = SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        std::ptr::null_mut(),
                        Some(on_event),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                    );
                    if hook.is_null() {
                        warn!(
                            "cannot watch the foreground window; per-application buttons disabled"
                        );
                        return;
                    }
                    report(GetForegroundWindow());
                    let mut msg: MSG = std::mem::zeroed();
                    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            });
        if let Err(e) = spawned {
            warn!("cannot start the foreground watcher: {e}");
        }
    }

    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        report(hwnd);
    }

    fn report(hwnd: HWND) {
        if let (Some(name), Some(tx)) = (program_of(hwnd), SENDER.get()) {
            debug!("foreground window belongs to {name}");
            let _ = tx.send(Event::Foreground(name));
        }
    }

    /// File name of the program owning `hwnd`, e.g. `chrome.exe`.
    fn program_of(hwnd: HWND) -> Option<String> {
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        // SAFETY: plain query on a window handle (may be stale: then pid = 0).
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == 0 {
            return None;
        }
        // SAFETY: limited-information access is granted for most processes;
        // the handle is closed below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None; // e.g. elevated programs
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` UTF-16 units.
        let ok = unsafe {
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
        };
        // SAFETY: we own the process handle.
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(str::to_owned)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::mpsc::Sender;

    use crate::daemon::Event;

    pub fn watch(_tx: Sender<Event>) {
        log::debug!("per-application buttons are only available on Windows");
    }
}
