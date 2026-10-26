//! The radial menu window (Windows).
//!
//! A layered, topmost popup centred on the mouse pointer. It never takes the
//! focus ([`WS_EX_NOACTIVATE`]) and captures the mouse while it is open, so a
//! click selects a sector and a click anywhere else (or the X in the middle)
//! just closes it.
//!
//! The picture is painted by [`crate::radial`] (sectors, shadow, hover) and
//! the labels and icons are drawn with GDI into small masks, then blended in.
//! The window is created once, hidden, and only moved and shown when a menu
//! is requested, so it appears in a few milliseconds.
//!
//! `logirev --radial-demo` shows a sample menu without a receiver: it is the
//! quickest way to look at the design (see `docs/TESTING.md`).

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use log::{debug, warn};
use windows_sys::Win32::Foundation::{
    COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC,
    DeleteObject, DrawTextW, GetMonitorInfoW, MonitorFromPoint, SelectObject, SetBkMode,
    SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_SEMIBOLD, HDC, HFONT,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForSystem, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer,
    LoadCursorW, PostQuitMessage, RegisterClassW, SetTimer, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WM_CAPTURECHANGED, WM_DESTROY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_RBUTTONDOWN, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use super::util::wide;
use crate::daemon::Event;
use crate::radial::{self, Layout, Metrics};
use crate::status::RadialMenu;
/// Window class of the menu.
const CLASS: &str = "logirev.radial";
/// Fade-in timer and safety timeout.
const TIMER_FADE: usize = 1;
const TIMER_TIMEOUT: usize = 2;
const TIMER_OUTSIDE: usize = 3;
/// How long the menu waits before closing itself (the service has the same
/// limit; whichever comes first wins).
const VISIBLE_MS: u32 = 20_000;
/// A release sooner than this after opening is a click, not a "hold and
/// release": the menu stays open for the click that follows.
const HOLD_GRACE: Duration = Duration::from_millis(180);
/// Supersampling used while drawing the text masks with GDI.
const TEXT_SCALE: usize = 2;
/// Label and title colours.
const TEXT_COLOR: [u8; 3] = [235, 239, 247];
const MUTED_COLOR: [u8; 3] = [148, 163, 184];

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

/// The menu window, created once by the tray application.
pub struct RadialWindow {
    hwnd: HWND,
}

impl RadialWindow {
    /// Registers the class and creates the hidden window.
    pub fn create(instance: HINSTANCE) -> Option<Self> {
        let class = wide(CLASS);
        let title = wide("logirev");
        // SAFETY: standard window class and window creation; the strings are
        // NUL-terminated and outlive the calls.
        let hwnd = unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class.as_ptr(),
                title.as_ptr(),
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
        (!hwnd.is_null()).then_some(Self { hwnd })
    }

    /// Shows `menu` under the pointer.
    pub fn show(&self, menu: &RadialMenu, tx: &Sender<Event>) {
        show_at(self.hwnd, menu, Some(tx), false);
    }

    /// The button that opened the menu was released: choose the sector under
    /// the pointer, keep the menu open (quick click) or cancel.
    pub fn release(&self, id: u64) {
        release_at(id);
    }
}

/// Shows a sample menu (for `logirev --radial-demo`), then ends the process
/// when it is closed.
pub fn demo() -> ExitCode {
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let Some(window) = RadialWindow::create(instance) else {
        eprintln!("cannot create the radial menu window");
        return ExitCode::FAILURE;
    };
    let item = |label: &str, icon: &str| crate::status::RadialItem {
        label: label.into(),
        icon: Some(icon.into()),
    };
    let menu = RadialMenu {
        id: 0,
        title: Some("logirev".into()),
        items: vec![
            item("Browser", "browser"),
            item("Terminal", "terminal"),
            item("Files", "folder"),
            item("Music", "play"),
            item("Desktop →", "desktop"),
            item("Settings", "settings"),
        ],
    };
    show_at(window.hwnd, &menu, None, true);
    // SAFETY: the standard Win32 message loop of this thread.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    ExitCode::SUCCESS
}

impl Drop for RadialWindow {
    fn drop(&mut self) {
        // SAFETY: the window is ours and not used any more.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd) };
    }
}

/// State of the menu while it is open.
struct Open {
    hwnd: HWND,
    id: u64,
    metrics: Metrics,
    layout: Layout,
    origin: POINT,
    /// Where the pointer was when the menu opened.
    opened_cursor: POINT,
    /// The left button went down over a sector (the choice is made on release).
    press_inside: bool,
    /// State of the mouse buttons at the last outside-click check.
    buttons_down: bool,
    center: POINT,
    size: (u32, u32),
    hover: Option<usize>,
    alpha: u8,
    opened_at: Instant,
    tx: Option<Sender<Event>>,
    demo: bool,
    surface: Surface,
    masks: Vec<Mask>,
}

/// A text or icon mask at 1x, to blend on top of the painted ring.
struct Mask {
    at: (i32, i32),
    width: usize,
    height: usize,
    coverage: Vec<u8>,
    color: [u8; 3],
}

impl Open {
    /// The complete picture (ring + labels) for the current hover.
    fn frame(&self) -> radial::Frame {
        let mut frame = radial::paint(self.layout.count(), self.hover, &self.metrics);
        for mask in &self.masks {
            radial::composite_mask(
                &mut frame,
                mask.at,
                &mask.coverage,
                mask.width,
                mask.height,
                mask.color,
            );
        }
        frame
    }
}

/// The 32-bit premultiplied BGRA bitmap the layered window shows.
struct Surface {
    dc: HDC,
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: usize,
    height: usize,
}

impl Surface {
    fn new(width: usize, height: usize) -> Option<Self> {
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..unsafe { std::mem::zeroed() }
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        // SAFETY: standard DIB creation; `info` is initialised and the bitmap
        // is freed in `Drop`.
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null() {
                return None;
            }
            let bitmap = CreateDIBSection(
                dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bitmap);
            Some(Self {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                width,
                height,
            })
        }
    }

    fn copy(&mut self, frame: &radial::Frame) {
        let pixels = radial::to_premultiplied_bgra(frame);
        debug_assert_eq!(pixels.len(), self.width * self.height * 4);
        // SAFETY: the DIB has room for width * height * 4 bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits, pixels.len());
        }
    }

    fn update(&self, hwnd: HWND, origin: &POINT, size: (u32, u32), alpha: u8) {
        let size = SIZE {
            cx: size.0 as i32,
            cy: size.1 as i32,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: the bitmap is ours; the structures live for the call.
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                std::ptr::null_mut(),
                origin,
                &size,
                self.dc,
                &source,
                0 as COLORREF,
                &blend,
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: the objects were created by `Surface::new` and are no
        // longer used.
        unsafe {
            if !self.old.is_null() {
                SelectObject(self.dc, self.old);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
        }
    }
}

/// Shows `menu` in `hwnd`, centred on the pointer.
fn show_at(hwnd: HWND, menu: &RadialMenu, tx: Option<&Sender<Event>>, demo: bool) {
    if menu.items.is_empty() {
        warn!("the radial menu has no items");
        return;
    }
    // A menu already open is replaced without telling the service: it has
    // replaced its own request too.
    drop_open(false);

    let mut cursor = POINT { x: 0, y: 0 };
    // SAFETY: plain query of the pointer position.
    unsafe { GetCursorPos(&mut cursor) };
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let metrics = Metrics::new(monitor_dpi(monitor));
    let (width, height) = metrics.frame_size();
    let origin = clamp_to_work_area(cursor, monitor, width, height);
    let Some(surface) = Surface::new(width as usize, height as usize) else {
        warn!("cannot create the bitmap of the radial menu");
        return;
    };
    let masks = build_masks(menu, &metrics);
    let mut open = Open {
        hwnd,
        id: menu.id,
        metrics,
        layout: Layout::new(menu.items.len()),
        origin,
        opened_cursor: cursor,
        press_inside: false,
        buttons_down: false,
        center: POINT {
            x: origin.x + width as i32 / 2,
            y: origin.y + height as i32 / 2,
        },
        size: (width, height),
        hover: None,
        alpha: 0,
        opened_at: Instant::now(),
        tx: tx.cloned(),
        demo,
        surface,
        masks,
    };
    let frame = open.frame();
    open.surface.copy(&frame);
    OPEN.with(|state| *state.borrow_mut() = Some(open));
    // The pointer may not be exactly in the middle if the menu was moved to
    // stay on screen: highlight the sector under it right away.
    let hover = hit();
    OPEN.with(|state| {
        if let Some(open) = state.borrow_mut().as_mut() {
            open.hover = hover;
        }
    });
    repaint();
    // SAFETY: our window; no focus is taken and the pointer is captured so
    // clicks outside the menu reach us too.
    unsafe {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetCapture(hwnd);
        SetTimer(hwnd, TIMER_FADE, 15, None);
        SetTimer(hwnd, TIMER_TIMEOUT, VISIBLE_MS, None);
        // Fallback for clicks outside the window: Windows does not always
        // deliver them to a no-activate window, even with the mouse captured.
        SetTimer(hwnd, TIMER_OUTSIDE, 15, None);
    }
    debug!("radial menu shown at ({}, {})", origin.x, origin.y);
}

/// DPI of the monitor the menu is on.
fn monitor_dpi(monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR) -> u32 {
    let (mut x, mut y) = (96u32, 96u32);
    // SAFETY: the monitor handle comes from `MonitorFromPoint`.
    let result = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
    if result >= 0 && x >= 96 {
        x
    } else {
        // SAFETY: plain query, no arguments.
        unsafe { GetDpiForSystem() }.max(96)
    }
}

/// Top-left corner of the menu: centred on `cursor`, kept inside the work
/// area of its monitor.
fn clamp_to_work_area(
    cursor: POINT,
    monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR,
    w: u32,
    h: u32,
) -> POINT {
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    // SAFETY: `info` is a valid MONITORINFO.
    let ok = unsafe { GetMonitorInfoW(monitor, &mut info) } != 0;
    let (left, top, right, bottom) = if ok {
        (
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        )
    } else {
        (0, 0, i32::MAX / 2, i32::MAX / 2)
    };
    let (w, h) = (w as i32, h as i32);
    let x = if right - left > w {
        (cursor.x - w / 2).clamp(left, right - w)
    } else {
        left
    };
    let y = if bottom - top > h {
        (cursor.y - h / 2).clamp(top, bottom - h)
    } else {
        top
    };
    POINT { x, y }
}

/// Rebuilds the picture and shows it with the current fade level.
fn repaint() {
    OPEN.with(|state| {
        let mut state = state.borrow_mut();
        let Some(open) = state.as_mut() else {
            return;
        };
        let frame = open.frame();
        open.surface.copy(&frame);
        open.surface
            .update(open.hwnd, &open.origin, open.size, open.alpha);
    });
}

/// Changes only the fade level, without repainting.
fn update_alpha() {
    OPEN.with(|state| {
        let state = state.borrow();
        if let Some(open) = state.as_ref() {
            open.surface
                .update(open.hwnd, &open.origin, open.size, open.alpha);
        }
    });
}

/// Which sector (if any) is under the pointer.
fn hit() -> Option<usize> {
    OPEN.with(|state| {
        let state = state.borrow();
        let open = state.as_ref()?;
        let mut cursor = POINT { x: 0, y: 0 };
        // SAFETY: plain query.
        unsafe { GetCursorPos(&mut cursor) };
        let dx = (cursor.x - open.center.x) as f32;
        let dy = (cursor.y - open.center.y) as f32;
        match open.layout.hit(&open.metrics, dx, dy) {
            radial::Hit::Item(index) => Some(index),
            radial::Hit::Center | radial::Hit::Outside => None,
        }
    })
}

/// Handles the release of the button that opened the menu.
///
/// A quick click (within [`HOLD_GRACE`], or released in the middle) leaves
/// the menu open for the click that follows; a held release over a sector
/// runs it; a release outside the ring cancels.
fn release_at(id: u64) {
    let decision = OPEN.with(|state| {
        let state = state.borrow();
        let open = state.as_ref()?;
        if open.id != id || open.opened_at.elapsed() < HOLD_GRACE {
            return None;
        }
        let mut cursor = POINT { x: 0, y: 0 };
        // SAFETY: plain query.
        unsafe { GetCursorPos(&mut cursor) };
        // A release without movement is a click, even when the menu had to be
        // moved away from a screen edge and the pointer is over a sector.
        let (mx, my) = (
            (cursor.x - open.opened_cursor.x) as f32,
            (cursor.y - open.opened_cursor.y) as f32,
        );
        let moved = 18.0 * open.metrics.scale;
        if mx * mx + my * my < moved * moved {
            return None;
        }
        let dx = (cursor.x - open.center.x) as f32;
        let dy = (cursor.y - open.center.y) as f32;
        match radial::release_action(open.layout, &open.metrics, dx, dy) {
            radial::Release::Select(index) => Some(Some(index)),
            radial::Release::KeepOpen => None,
            radial::Release::Cancel => Some(None),
        }
    });
    if let Some(selected) = decision {
        close(selected);
    }
}

/// Closes the menu. `Some(index)` sends the choice, `None` cancels it.
fn close(selected: Option<usize>) {
    let Some(open) = OPEN.with(|state| state.borrow_mut().take()) else {
        return;
    };
    // SAFETY: our window; release the capture only once.
    unsafe {
        ReleaseCapture();
        KillTimer(open.hwnd, TIMER_FADE);
        KillTimer(open.hwnd, TIMER_TIMEOUT);
        KillTimer(open.hwnd, TIMER_OUTSIDE);
        ShowWindow(open.hwnd, SW_HIDE);
    }
    if let Some(tx) = &open.tx {
        let event = match selected {
            Some(index) => {
                debug!("radial menu: item {} selected", index + 1);
                // Let the window disappear before the injected click/key
                // arrives, so the action never lands on the menu itself.
                std::thread::sleep(Duration::from_millis(12));
                Event::RadialSelect { id: open.id, index }
            }
            None => Event::RadialCancel { id: open.id },
        };
        let _ = tx.send(event);
    }
    if open.demo {
        // SAFETY: ends the demo's message loop.
        unsafe { PostQuitMessage(0) };
    }
    debug!("radial menu closed");
}

/// Forgets an open menu without telling anyone (it is being replaced).
fn drop_open(tell: bool) {
    if tell {
        close(None);
    } else if let Some(open) = OPEN.with(|state| state.borrow_mut().take()) {
        // SAFETY: our window; cleanup only.
        unsafe {
            ReleaseCapture();
            KillTimer(open.hwnd, TIMER_FADE);
            KillTimer(open.hwnd, TIMER_TIMEOUT);
            KillTimer(open.hwnd, TIMER_OUTSIDE);
            ShowWindow(open.hwnd, SW_HIDE);
        }
    }
}

/// Draws the labels and icons of a menu into 1x coverage masks.
fn build_masks(menu: &RadialMenu, metrics: &Metrics) -> Vec<Mask> {
    load_icon_font();
    let icon_font = font("Phosphor", 400, metrics.icon_size * TEXT_SCALE as f32);
    let label_font = font(
        "Segoe UI",
        FW_SEMIBOLD,
        metrics.label_size * TEXT_SCALE as f32,
    );
    let title_font = font("Segoe UI", 400, metrics.title_size * TEXT_SCALE as f32);
    let mut masks = Vec::new();
    for (i, placement) in radial::item_placements(menu.items.len(), metrics)
        .into_iter()
        .enumerate()
    {
        let item = &menu.items[i];
        let (x, y, w, h) = placement.bounds.bounds();
        if w == 0 || h == 0 {
            continue;
        }
        let Some(coverage) = text_mask(w as usize, h as usize, |dc| {
            if let Some(glyph) = item.icon.as_deref().and_then(radial::icon_glyph) {
                if let Some((cx, cy)) = placement.icon {
                    let size = metrics.icon_size * TEXT_SCALE as f32;
                    let mut rect = RECT {
                        left: ((cx - x as f32) * TEXT_SCALE as f32 - size / 2.0).round() as i32,
                        top: ((cy - y as f32) * TEXT_SCALE as f32 - size / 2.0).round() as i32,
                        right: 0,
                        bottom: 0,
                    };
                    rect.right = rect.left + size.round() as i32;
                    rect.bottom = rect.top + size.round() as i32;
                    draw(dc, icon_font, glyph, &mut rect, false);
                }
            }
            let mut rect = scaled(&placement.label, (x, y));
            draw(dc, label_font, &item.label, &mut rect, true);
        }) else {
            continue;
        };
        masks.push(Mask {
            at: (x, y),
            width: w as usize,
            height: h as usize,
            coverage,
            color: TEXT_COLOR,
        });
    }
    if let Some(placement) = radial::title_placement(menu.title.as_deref(), metrics) {
        let (x, y, w, h) = placement.bounds.bounds();
        if w > 0 && h > 0 {
            if let Some(title) = menu
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                if let Some(coverage) = text_mask(w as usize, h as usize, |dc| {
                    let mut rect = scaled(&placement.label, (x, y));
                    draw(dc, title_font, title, &mut rect, true);
                }) {
                    masks.push(Mask {
                        at: (x, y),
                        width: w as usize,
                        height: h as usize,
                        coverage,
                        color: MUTED_COLOR,
                    });
                }
            }
        }
    }
    // SAFETY: the fonts were created above and are no longer selected.
    unsafe {
        DeleteObject(icon_font);
        DeleteObject(label_font);
        DeleteObject(title_font);
    }
    masks
}

/// A label rectangle in the coordinates of its mask, scaled for supersampling.
fn scaled(rect: &radial::Rect, origin: (i32, i32)) -> RECT {
    let f = TEXT_SCALE as f32;
    RECT {
        left: ((rect.left - origin.0 as f32) * f).round() as i32,
        top: ((rect.top - origin.1 as f32) * f).round() as i32,
        right: ((rect.right - origin.0 as f32) * f).round() as i32,
        bottom: ((rect.bottom - origin.1 as f32) * f).round() as i32,
    }
}

/// Draws white text on black in a `w * TEXT_SCALE` by `h * TEXT_SCALE` DIB
/// and returns the downsampled coverage.
fn text_mask(w: usize, h: usize, paint: impl FnOnce(HDC)) -> Option<Vec<u8>> {
    let (bw, bh) = (w * TEXT_SCALE, h * TEXT_SCALE);
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bw as i32,
        biHeight: -(bh as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..unsafe { std::mem::zeroed() }
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: standard DIB drawing; everything created here is freed below.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let bitmap = CreateDIBSection(
            dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        std::ptr::write_bytes(bits.cast::<u8>(), 0, bw * bh * 4);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00FF_FFFF);
        paint(dc);
        let bytes = std::slice::from_raw_parts(bits.cast::<u8>(), bw * bh * 4);
        let gray = radial::coverage_from_bgra(bytes);
        let (coverage, _, _) = radial::downsample(&gray, bw, bh, TEXT_SCALE);
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        Some(coverage)
    }
}

/// Draws one line of text in `rect` (already in DIB pixels).
fn draw(dc: HDC, font: HFONT, text: &str, rect: &mut RECT, ellipsis: bool) {
    if font.is_null() {
        return;
    }
    let mut text: Vec<u16> = text.encode_utf16().collect();
    if text.is_empty() {
        return;
    }
    let mut flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
    if ellipsis {
        flags |= DT_END_ELLIPSIS;
    }
    // SAFETY: `dc` is ours, the font is selected below, and both buffers
    // live for the call.
    unsafe {
        let old = SelectObject(dc, font);
        DrawTextW(dc, text.as_mut_ptr(), text.len() as i32, rect, flags);
        SelectObject(dc, old);
    }
}

/// Creates a font at the given pixel height.
fn font(face: &str, weight: u32, height: f32) -> HFONT {
    let name = wide(face);
    // SAFETY: the face name is NUL-terminated and outlives the call.
    unsafe {
        CreateFontW(
            -(height.round().max(1.0) as i32),
            0,
            0,
            0,
            weight as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            ANTIALIASED_QUALITY as u32,
            DEFAULT_PITCH as u32,
            name.as_ptr(),
        )
    }
}

/// Makes the Phosphor icons available to GDI (once per process).
fn load_icon_font() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut count = 0u32;
        // SAFETY: the font bytes are embedded in the binary and live for the
        // whole process; Windows copies the face when it is used.
        let handle: HANDLE = unsafe {
            AddFontMemResourceEx(
                crate::icons::PHOSPHOR_TTF.as_ptr().cast(),
                crate::icons::PHOSPHOR_TTF.len() as u32,
                std::ptr::null(),
                &mut count as *mut u32 as *const u32,
            )
        };
        if handle.is_null() {
            warn!("cannot load the icon font of the radial menu");
        }
    });
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_MOUSEMOVE => {
            let hovered = hit();
            let changed = OPEN.with(|state| {
                let mut state = state.borrow_mut();
                let Some(open) = state.as_mut() else {
                    return false;
                };
                if open.hover == hovered {
                    false
                } else {
                    open.hover = hovered;
                    true
                }
            });
            if changed {
                repaint();
            }
            0
        }
        WM_LBUTTONDOWN => {
            // Choose on release, after the physical left button is up: an
            // action that injects a click must not start while the user is
            // still holding it.
            let inside = hit().is_some();
            OPEN.with(|state| {
                if let Some(open) = state.borrow_mut().as_mut() {
                    open.press_inside = inside;
                }
            });
            0
        }
        WM_LBUTTONUP => {
            let inside = OPEN.with(|state| {
                state
                    .borrow()
                    .as_ref()
                    .is_some_and(|open| open.press_inside)
            });
            close(if inside { hit() } else { None });
            0
        }
        WM_RBUTTONDOWN => {
            close(None);
            0
        }
        WM_CAPTURECHANGED => {
            if lparam as HWND != hwnd {
                close(None);
            }
            0
        }
        WM_TIMER => {
            match wparam {
                TIMER_FADE => {
                    let done = OPEN.with(|state| {
                        let mut state = state.borrow_mut();
                        let Some(open) = state.as_mut() else {
                            return true;
                        };
                        open.alpha = open.alpha.saturating_add(28);
                        open.alpha == 255
                    });
                    update_alpha();
                    if done {
                        unsafe { KillTimer(hwnd, TIMER_FADE) };
                    }
                }
                TIMER_TIMEOUT => close(None),
                TIMER_OUTSIDE => {
                    let close_now = OPEN.with(|state| {
                        let mut state = state.borrow_mut();
                        let Some(open) = state.as_mut() else {
                            return false;
                        };
                        let mut cursor = POINT { x: 0, y: 0 };
                        // SAFETY: plain query.
                        unsafe { GetCursorPos(&mut cursor) };
                        let dx = (cursor.x - open.center.x) as f32;
                        let dy = (cursor.y - open.center.y) as f32;
                        let visible = open.metrics.panel_radius();
                        let outside = dx * dx + dy * dy > visible * visible;
                        // SAFETY: plain query of the physical mouse buttons.
                        let down = unsafe {
                            (GetAsyncKeyState(i32::from(VK_LBUTTON)) as u16 & 0x8000 != 0)
                                || (GetAsyncKeyState(i32::from(VK_RBUTTON)) as u16 & 0x8000 != 0)
                                || (GetAsyncKeyState(i32::from(VK_MBUTTON)) as u16 & 0x8000 != 0)
                        };
                        let changed = down != open.buttons_down;
                        open.buttons_down = down;
                        outside && changed
                    });
                    if close_now {
                        close(None);
                    }
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            drop_open(true);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
