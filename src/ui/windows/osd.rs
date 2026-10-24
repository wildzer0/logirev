//! The on-screen message ("OSD"): a small dark box near the bottom of the
//! screen that says what a button just changed ("800 DPI", "Precision mode
//! on"...), then disappears after a moment.
//!
//! The window never takes the focus and lets clicks through, so it cannot
//! get in the way of what the user is doing.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
    EndPaint, FillRect, GetDC, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DT_CALCRECT, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    HFONT, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, KillTimer, RegisterClassW,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, HTTRANSPARENT, HWND_TOPMOST,
    LWA_ALPHA, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_NCHITTEST, WM_PAINT, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

use super::util::wide;

/// How long the message stays on screen, in milliseconds.
const VISIBLE_MS: u32 = 1400;
const TIMER_ID: usize = 1;
/// Background (dark slate) and text colours, as `0x00BBGGRR`.
const BACKGROUND: u32 = 0x002B_221E;
const TEXT: u32 = 0x00FF_FFFF;

thread_local! {
    /// The text being shown (UTF-16) and the font, read by the painter.
    static CONTENT: RefCell<(Vec<u16>, HFONT)> =
        const { RefCell::new((Vec::new(), std::ptr::null_mut())) };
}

/// The on-screen message window (one per program, on the UI thread).
pub struct Osd {
    hwnd: HWND,
}

impl Osd {
    /// Creates the (hidden) window.
    pub fn create(instance: windows_sys::Win32::Foundation::HINSTANCE) -> Option<Self> {
        let class = wide("logirev.osd");
        // SAFETY: window creation with valid, NUL-terminated strings.
        let hwnd = unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(osd_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT,
                class.as_ptr(),
                wide("logirev").as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        // SAFETY: our own layered window; slightly see-through.
        unsafe { SetLayeredWindowAttributes(hwnd, 0, 235, LWA_ALPHA) };
        Some(Self { hwnd })
    }

    /// Shows `text` near the bottom of the screen the mouse is on.
    pub fn show(&self, text: &str) {
        let hwnd = self.hwnd;
        // SAFETY: plain Win32 calls on our own window; every structure is
        // initialised before use and every GDI object created is freed.
        unsafe {
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            let scale = |v: i32| v * dpi / 96;
            let font = CreateFontW(
                -scale(22),
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                wide("Segoe UI").as_ptr(),
            );
            let mut text16: Vec<u16> = text.encode_utf16().collect();
            if text16.is_empty() {
                text16.push(b' '.into());
            }

            // Measure the text.
            let dc = GetDC(hwnd);
            let old = SelectObject(dc, font);
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            DrawTextW(
                dc,
                text16.as_ptr(),
                text16.len() as i32,
                &mut r,
                DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
            );
            SelectObject(dc, old);
            ReleaseDC(hwnd, dc);
            let (w, h) = (r.right + scale(56), r.bottom + scale(28));

            // Bottom centre of the work area of the monitor under the mouse.
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(monitor, &mut info);
            let work = info.rcWork;
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.bottom - h - scale(64);

            CONTENT.with(|c| {
                let mut c = c.borrow_mut();
                if !c.1.is_null() {
                    DeleteObject(c.1);
                }
                *c = (text16, font);
            });
            let radius = scale(16);
            // The window owns the region after this call.
            SetWindowRgn(
                hwnd,
                CreateRoundRectRgn(0, 0, w + 1, h + 1, radius, radius),
                1,
            );
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(hwnd, std::ptr::null(), 1);
            SetTimer(hwnd, TIMER_ID, VISIBLE_MS, None);
        }
    }
}

unsafe extern "system" fn osd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r);
            let brush = CreateSolidBrush(BACKGROUND);
            FillRect(dc, &r, brush);
            DeleteObject(brush);
            CONTENT.with(|c| {
                let c = c.borrow();
                let old = SelectObject(dc, c.1);
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, TEXT);
                DrawTextW(
                    dc,
                    c.0.as_ptr(),
                    c.0.len() as i32,
                    &mut r,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
                SelectObject(dc, old);
            });
            EndPaint(hwnd, &ps);
            0
        }
        WM_TIMER => {
            KillTimer(hwnd, TIMER_ID);
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        // Clicks go to whatever is underneath.
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
