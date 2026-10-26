//! The settings window (`logirev --settings`).
//!
//! A separate process from the service: it starts when the user opens it
//! (tray icon, Start menu shortcut...) and exits when closed, so the always-
//! running service stays small. It talks to the service through
//! [`crate::ipc`] and edits the configuration file, which the service
//! reloads by itself.
//!
//! * [`app`] — pages and state;
//! * [`form`] — the "what does this button do" form;
//! * [`style`] — colours, fonts and icons;
//! * [`i18n`] — English and Italian texts.

mod app;
mod form;
mod i18n;
mod style;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use eframe::egui;

use crate::app::{default_config_path, ServiceOptions};

/// Window title; also used to find an already open window.
const TITLE: &str = "logirev";

/// Opens the settings window and returns when it is closed.
pub fn run(opts: &ServiceOptions) -> ExitCode {
    // After an update the old window may still be closing: wait for it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let _guard = loop {
        match crate::instance::acquire_named("logirev-settings") {
            Ok(g) => break g,
            Err(_) if opts.after_update && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(_) => {
                focus_existing_window();
                return ExitCode::SUCCESS;
            }
        }
    };
    crate::update::cleanup_old_files();
    let custom = opts.config.is_some();
    let config_path: PathBuf = opts.config.clone().unwrap_or_else(default_config_path);

    let size = 128;
    let icon = egui::IconData {
        rgba: crate::icon::draw_rgba(size),
        width: size,
        height: size,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_app_id("logirev")
            .with_inner_size([1040.0, 700.0])
            .with_min_inner_size([1040.0, 700.0])
            .with_max_inner_size([1040.0, 700.0])
            .with_resizable(false)
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    let result = eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::SettingsApp::new(cc, config_path, custom)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            #[cfg(windows)]
            crate::ui::message_box(
                "logirev",
                &format!("Cannot open the settings window:\n\n{e}"),
                true,
            );
            #[cfg(not(windows))]
            eprintln!("cannot open the settings window: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Brings an already open settings window to the front.
fn focus_existing_window() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        let title: Vec<u16> = TITLE.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: NUL-terminated title; the handle is only used if found.
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
    }
}
