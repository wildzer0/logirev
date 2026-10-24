//! The "Devices" window: a table of receivers and their paired devices.
//!
//! It is a standard Windows list control, created once and then shown or
//! hidden; closing it only hides it. Its content is rebuilt from each new
//! [`Snapshot`].

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVCFMT_LEFT, LVCF_FMT,
    LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS, LVM_INSERTCOLUMNW,
    LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMTEXTW, LVS_EX_DOUBLEBUFFER,
    LVS_EX_FULLROWSELECT, LVS_NOSORTHEADER, LVS_REPORT, LVS_SINGLESEL, WC_LISTVIEWW,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetWindow, LoadCursorW, MoveWindow,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, CW_USEDEFAULT,
    GW_CHILD, HICON, IDC_ARROW, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CLOSE, WM_SIZE,
    WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use super::util::wide;
use crate::status::Snapshot;
use crate::ui::view::{rows, COLUMNS, COLUMN_WIDTHS};

const CLASS_NAME: &str = "logirev.devices";
/// Approximate width of one character at 96 DPI, in pixels.
const CHAR_WIDTH: i32 = 7;

pub struct DevicesWindow {
    hwnd: HWND,
    list: HWND,
}

impl DevicesWindow {
    /// Creates the (hidden) window.
    pub fn create(instance: HINSTANCE, icon: HICON) -> Option<Self> {
        let class = wide(CLASS_NAME);
        let title = wide("logirev - devices");
        // SAFETY: standard window creation; strings outlive the calls.
        unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_LISTVIEW_CLASSES,
            };
            InitCommonControlsEx(&icc);

            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                hIcon: icon,
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                800,
                260,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return None;
            }
            let scale = |px: i32| px * GetDpiForWindow(hwnd).max(96) as i32 / 96;
            let total: i32 = COLUMN_WIDTHS.iter().map(|w| scale(w * CHAR_WIDTH)).sum();
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                total + scale(40),
                scale(240),
                SWP_NOMOVE | SWP_NOZORDER,
            );

            let empty = wide("");
            let list = CreateWindowExW(
                0,
                WC_LISTVIEWW,
                empty.as_ptr(),
                WS_CHILD | WS_VISIBLE | LVS_REPORT | LVS_SINGLESEL | LVS_NOSORTHEADER,
                0,
                0,
                0,
                0,
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if list.is_null() {
                return None;
            }
            let ex = (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize;
            SendMessageW(list, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as usize, ex);
            for (i, (name, width)) in COLUMNS.iter().zip(COLUMN_WIDTHS).enumerate() {
                let mut text = wide(name);
                let col = LVCOLUMNW {
                    mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
                    fmt: LVCFMT_LEFT,
                    cx: scale(width * CHAR_WIDTH),
                    pszText: text.as_mut_ptr(),
                    ..std::mem::zeroed()
                };
                SendMessageW(list, LVM_INSERTCOLUMNW, i, &col as *const _ as LPARAM);
            }
            fit_list(hwnd, list);
            Some(Self { hwnd, list })
        }
    }

    /// Shows the window and brings it to the front.
    pub fn show(&self) {
        // SAFETY: valid window handle.
        unsafe {
            ShowWindow(self.hwnd, SW_SHOW);
            SetForegroundWindow(self.hwnd);
        }
    }

    /// Replaces the table content.
    pub fn update(&self, snapshot: &Snapshot) {
        // SAFETY: valid list handle; every text buffer outlives its message.
        unsafe {
            SendMessageW(self.list, LVM_DELETEALLITEMS, 0, 0);
            for (r, row) in rows(snapshot).iter().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    let mut text = wide(cell);
                    let mut item = LVITEMW {
                        mask: LVIF_TEXT,
                        iItem: r as i32,
                        iSubItem: c as i32,
                        pszText: text.as_mut_ptr(),
                        ..std::mem::zeroed()
                    };
                    if c == 0 {
                        SendMessageW(self.list, LVM_INSERTITEMW, 0, &mut item as *mut _ as LPARAM);
                    } else {
                        SendMessageW(
                            self.list,
                            LVM_SETITEMTEXTW,
                            r,
                            &mut item as *mut _ as LPARAM,
                        );
                    }
                }
            }
        }
    }
}

/// Makes the list fill the window's client area.
unsafe fn fit_list(hwnd: HWND, list: HWND) {
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut rc);
    MoveWindow(list, 0, 0, rc.right - rc.left, rc.bottom - rc.top, 1);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let list = GetWindow(hwnd, GW_CHILD);
            if !list.is_null() {
                fit_list(hwnd, list);
            }
            0
        }
        WM_CLOSE => {
            // Closing only hides: the window is reused next time.
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
