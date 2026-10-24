//! The notification-area ("tray") icon and its menu.

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    GetSystemMetrics, PostMessageW, SetForegroundWindow, SetMenuDefaultItem, TrackPopupMenu, HICON,
    ICONINFO, MF_CHECKED, MF_DISABLED, MF_POPUP, MF_SEPARATOR, MF_STRING, SM_CXSMICON,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

use super::util::{copy_into, wide};
use crate::ui::view::{Command, MenuEntry};

/// Identifier of our (single) icon.
const ICON_ID: u32 = 1;

/// The tray icon of one window.
pub struct Tray {
    hwnd: HWND,
    callback: u32,
    /// The icon shown now.
    icon: HICON,
    /// The plain icon (also used by windows); `icon` may equal it.
    plain: HICON,
    size: i32,
    /// Battery shown on the icon (rounded), `None` = plain icon.
    battery: Option<(u8, bool)>,
}

impl Tray {
    /// Creates the icon; mouse events arrive at `hwnd` as message `callback`.
    pub fn new(hwnd: HWND, callback: u32) -> Self {
        // SAFETY: plain metric query.
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64);
        let plain = make_icon(&crate::icon::draw_rgba(size as u32), size);
        let tray = Self {
            hwnd,
            callback,
            icon: plain,
            plain,
            size,
            battery: None,
        };
        tray.add();
        tray
    }

    /// The application icon (also used for windows).
    #[cfg(not(feature = "gui"))]
    pub fn app_icon(&self) -> HICON {
        self.plain
    }

    fn data(&self) -> NOTIFYICONDATAW {
        // SAFETY: NOTIFYICONDATAW is plain data; all-zero is a valid start.
        let mut d: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        d.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        d.hWnd = self.hwnd;
        d.uID = ICON_ID;
        d
    }

    /// (Re-)adds the icon, e.g. after Explorer restarted.
    pub fn add(&self) -> bool {
        let mut d = self.data();
        d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        d.uCallbackMessage = self.callback;
        d.hIcon = self.icon;
        copy_into(&mut d.szTip, "logirev");
        // SAFETY: `d` is fully initialised.
        unsafe { Shell_NotifyIconW(NIM_ADD, &d) != 0 }
    }

    /// Changes the text shown when hovering the icon.
    pub fn set_tip(&self, tip: &str) {
        let mut d = self.data();
        d.uFlags = NIF_TIP;
        copy_into(&mut d.szTip, tip);
        // SAFETY: `d` is fully initialised.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
    }

    /// Shows `battery` (percent, charging) on the icon, or the plain icon.
    pub fn set_battery(&mut self, battery: Option<(u8, bool)>) {
        // Steps of 10 %: no need to redraw for every percent.
        let battery = battery.map(|(p, c)| (p.min(100).div_ceil(10) * 10, c));
        if battery == self.battery {
            return;
        }
        let icon = match battery {
            Some((p, c)) => make_icon(
                &crate::icon::draw_with_battery(self.size as u32, p, c),
                self.size,
            ),
            None => self.plain,
        };
        if icon.is_null() {
            return;
        }
        let mut d = self.data();
        d.uFlags = NIF_ICON;
        d.hIcon = icon;
        // SAFETY: `d` is fully initialised; the old icon is destroyed only
        // after the shell has the new one.
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &d);
            if self.icon != self.plain {
                DestroyIcon(self.icon);
            }
        }
        self.icon = icon;
        self.battery = battery;
    }

    /// Shows a notification next to the icon (a toast on Windows 10/11).
    pub fn notify(&self, title: &str, text: &str, warning: bool) {
        let mut d = self.data();
        d.uFlags = NIF_INFO;
        copy_into(&mut d.szInfoTitle, title);
        copy_into(&mut d.szInfo, text);
        d.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
        // SAFETY: `d` is fully initialised.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
    }

    pub fn remove(&self) {
        let d = self.data();
        // SAFETY: `d` identifies our icon.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &d) };
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
        // SAFETY: the icons were created by `make_icon` and are owned by us.
        unsafe {
            if self.icon != self.plain {
                DestroyIcon(self.icon);
            }
            DestroyIcon(self.plain);
        }
    }
}

/// Shows the menu at the mouse position and returns the command picked.
/// Returns the raw id of the picked item (see [`crate::ui::view::decode`]).
pub fn show_menu(hwnd: HWND, entries: &[MenuEntry]) -> Option<usize> {
    // SAFETY: standard popup-menu sequence; every string outlives its
    // AppendMenuW call (the menu copies it).
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        for entry in entries {
            match entry {
                MenuEntry::Header(text) => {
                    let t = wide(text);
                    AppendMenuW(menu, MF_STRING | MF_DISABLED, 0, t.as_ptr());
                }
                MenuEntry::Device { label, online } => {
                    let t = wide(label);
                    let flags = MF_STRING | if *online { MF_CHECKED } else { 0 };
                    AppendMenuW(menu, flags, Command::ShowDevices.id(), t.as_ptr());
                }
                MenuEntry::Separator => {
                    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                }
                MenuEntry::Command {
                    command,
                    label,
                    checked,
                } => {
                    let t = wide(label);
                    let flags = MF_STRING | if *checked { MF_CHECKED } else { 0 };
                    AppendMenuW(menu, flags, command.id(), t.as_ptr());
                }
                MenuEntry::Submenu { label, items } => {
                    let sub = CreatePopupMenu();
                    if sub.is_null() {
                        continue;
                    }
                    for item in items {
                        let t = wide(&item.label);
                        let flags = MF_STRING | if item.checked { MF_CHECKED } else { 0 };
                        AppendMenuW(sub, flags, item.id, t.as_ptr());
                    }
                    let t = wide(label);
                    // The submenu is destroyed together with its parent.
                    AppendMenuW(menu, MF_STRING | MF_POPUP, sub as usize, t.as_ptr());
                }
            }
        }
        SetMenuDefaultItem(menu, Command::ShowDevices.id() as u32, 0);

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        (picked > 0).then_some(picked as usize)
    }
}

/// Builds an icon handle from `size`×`size` RGBA pixels (see
/// [`crate::icon`]).
fn make_icon(rgba: &[u8], size: i32) -> HICON {
    // Windows wants BGRA.
    let pixels: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();

    // SAFETY: standard 32-bit ARGB icon creation; the DIB's memory is filled
    // with exactly size*size*4 bytes; bitmaps are freed after the icon copies
    // them.
    unsafe {
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(
            std::ptr::null_mut(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if color.is_null() || bits.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
        // Monochrome mask, all zero: transparency comes from the alpha channel.
        let stride = (((size + 15) / 16) * 2) as usize;
        let mask_bits = vec![0u8; stride * size as usize];
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr().cast());
        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}
