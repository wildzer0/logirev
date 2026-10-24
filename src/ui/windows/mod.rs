//! Windows tray application.
//!
//! # Threads
//!
//! * The **UI thread** (the program's main thread) owns every window and
//!   runs the Win32 message loop. It owns a hidden window that receives the
//!   tray icon's mouse events, "device arrived" notifications and the
//!   daemon's updates.
//! * The **daemon thread** runs [`crate::daemon::run`]. It never touches
//!   windows: it pushes [`UiUpdate`]s into a queue and posts
//!   `WM_APP_UPDATE` to wake the UI thread, which then drains the queue.
//!
//! Commands go the other way through the daemon's event channel
//! ([`Event::Rescan`], [`Event::Reload`], [`Event::Quit`]).
//!
//! # Re-entrancy
//!
//! Menus and message boxes run nested message loops that call the window
//! procedure again. The UI state lives in a `RefCell`, so it is never kept
//! borrowed across such calls: handlers copy what they need, release the
//! borrow, and only then open a menu or a message box.

// The native devices window is only used when the settings window (egui)
// is not built in.
#[cfg(not(feature = "gui"))]
mod devices;
mod osd;
mod scroll;
mod tray;
mod util;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use log::{error, info, warn};
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterDeviceNotificationW, RegisterWindowMessageW,
    TranslateMessage, DBT_DEVICEARRIVAL, DBT_DEVTYP_DEVICEINTERFACE, DEVICE_NOTIFY_WINDOW_HANDLE,
    DEV_BROADCAST_DEVICEINTERFACE_W, MSG, WM_APP, WM_CONTEXTMENU, WM_DESTROY, WM_DEVICECHANGE,
    WM_ENDSESSION, WM_LBUTTONUP, WM_QUERYENDSESSION, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

pub use util::message_box;

use crate::app::{self, Prepared, ServiceOptions};
use crate::autostart;
use crate::daemon::{self, Event};
use crate::instance::InstanceGuard;
use crate::status::{Snapshot, UiHooks, UiUpdate};
use crate::ui::view::{self, Command, Picked};
#[cfg(not(feature = "gui"))]
use devices::DevicesWindow;
use tray::Tray;

/// Tray icon mouse events.
const WM_APP_TRAY: u32 = WM_APP + 1;
/// "Updates are waiting in the queue".
const WM_APP_UPDATE: u32 = WM_APP + 2;

/// Device interface class of HID devices.
const GUID_DEVINTERFACE_HID: GUID = GUID::from_u128(0x4d1e55b2_f16f_11cf_88cb_001111000030);

type Queue = Arc<Mutex<VecDeque<UiUpdate>>>;

/// Everything the window procedure needs.
struct State {
    hwnd: HWND,
    tray: Tray,
    osd: Option<osd::Osd>,
    /// Show the battery on the tray icon.
    tray_battery: bool,
    #[cfg(not(feature = "gui"))]
    devices: Option<DevicesWindow>,
    snapshot: Snapshot,
    queue: Queue,
    tx: Sender<Event>,
    daemon: Option<JoinHandle<()>>,
    config_path: PathBuf,
    log_file: Option<PathBuf>,
    autostart_command: String,
    /// Message Explorer broadcasts when the taskbar is (re)created.
    taskbar_created: u32,
    /// Error that stopped the daemon, reported as the exit status.
    failure: Option<String>,
    /// The configuration file was given with `--config`.
    #[cfg(feature = "gui")]
    custom_config: bool,
    _instance: InstanceGuard,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Runs the tray application until the user picks "Quit" or logs off.
pub fn run(opts: &ServiceOptions) -> ExitCode {
    let prepared = match app::prepare(opts, false) {
        Ok(p) => p,
        Err(e) if e.contains("already running") && cfg!(feature = "gui") => {
            // Started again while running: the user wants the window.
            open_settings(opts.config.as_deref(), false);
            return ExitCode::SUCCESS;
        }
        Err(e) if e.contains("already running") => {
            message_box(
                "logirev",
                "logirev is already running: look for its icon next to the clock.",
                false,
            );
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            message_box("logirev", &format!("logirev could not start:\n\n{e}"), true);
            return ExitCode::FAILURE;
        }
    };
    match start(prepared, opts) {
        // Started by the user (not at log-in): show the window too.
        Ok(()) if !opts.background && cfg!(feature = "gui") => {
            open_settings(opts.config.as_deref(), opts.after_update)
        }
        Ok(()) => {}
        Err(e) => {
            error!("{e}");
            message_box("logirev", &e, true);
            return ExitCode::FAILURE;
        }
    }

    // The message loop: runs until PostQuitMessage.
    // SAFETY: standard Win32 message loop on the thread owning the windows.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let state = STATE.with(|s| s.borrow_mut().take());
    let failure = state.and_then(|mut s| {
        stop_daemon(&mut s);
        s.failure.take()
    });
    info!("stopped");
    log::logger().flush();
    if failure.is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Creates the windows, the tray icon and the daemon thread.
fn start(p: Prepared, opts: &ServiceOptions) -> Result<(), String> {
    let class = util::wide("logirev.main");
    let title = util::wide("logirev-service");
    // SAFETY: window creation with valid, NUL-terminated strings.
    let (instance, hwnd) = unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        // A normal top-level window that is never shown: it receives the
        // tray callbacks and the device notifications.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        (instance, hwnd)
    };
    if hwnd.is_null() {
        return Err("cannot create the main window".into());
    }

    let tray = Tray::new(hwnd, WM_APP_TRAY);
    #[cfg(not(feature = "gui"))]
    let devices = DevicesWindow::create(instance, tray.app_icon());
    #[cfg(not(feature = "gui"))]
    if devices.is_none() {
        warn!("cannot create the devices window");
    }
    let osd = osd::Osd::create(instance);
    if osd.is_none() {
        warn!("cannot create the on-screen message window");
    }
    register_device_notifications(hwnd);
    // SAFETY: NUL-terminated string.
    let taskbar_created = unsafe { RegisterWindowMessageW(util::wide("TaskbarCreated").as_ptr()) };

    // Keep "Start with Windows" pointing at this copy of the program.
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let autostart_command = autostart::command_line(&exe, opts.config.as_deref());
    autostart::refresh(&autostart_command);

    // The daemon thread.
    let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
    let (tx, rx) = mpsc::channel();
    let post = {
        let queue = Arc::clone(&queue);
        let target = hwnd as isize; // HWND is not Send; the value is.
        move |update: UiUpdate| {
            if let Ok(mut q) = queue.lock() {
                q.push_back(update);
            }
            // SAFETY: posting to a window owned by another thread is allowed;
            // if the window is gone the call simply fails.
            unsafe { PostMessageW(target as HWND, WM_APP_UPDATE, 0, 0) };
        }
    };
    crate::foreground::watch(tx.clone());
    if p.cfg.general.check_updates {
        crate::update::spawn_checker(tx.clone());
    }
    let ipc_for_daemon = app::start_ipc(&tx);
    let (cfg, config_path, pcap) = (p.cfg, p.config_path.clone(), p.pcap);
    let daemon_tx = tx.clone();
    let daemon = std::thread::Builder::new()
        .name("daemon".into())
        .spawn(move || {
            let (to_ui, to_ipc) = (post.clone(), ipc_for_daemon.clone());
            let hooks = UiHooks {
                observer: Box::new(move |update| {
                    if let Some(server) = &to_ipc {
                        server.publish(&update);
                    }
                    to_ui(update);
                }),
                hotplug_events: true,
            };
            let result = daemon::run(cfg, &config_path, pcap, daemon_tx, rx, Some(hooks));
            if let Some(server) = &ipc_for_daemon {
                server.close();
            }
            post(UiUpdate::Stopped(result.err()));
        })
        .map_err(|e| format!("cannot start the service thread: {e}"))?;

    info!("tray icon ready");
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            hwnd,
            tray,
            osd,
            tray_battery: true,
            #[cfg(not(feature = "gui"))]
            devices,
            snapshot: Snapshot::default(),
            queue,
            tx,
            daemon: Some(daemon),
            config_path: p.config_path,
            log_file: p.log_file,
            #[cfg(feature = "gui")]
            custom_config: opts.config.is_some(),
            autostart_command,
            taskbar_created,
            failure: None,
            _instance: p.instance,
        })
    });
    Ok(())
}

/// Starts the settings window (a separate `logirev --settings` process).
fn open_settings(config: Option<&std::path::Path>, after_update: bool) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--settings");
    if after_update {
        // The window of the old version may still be closing.
        cmd.arg("--after-update");
    }
    if let Some(c) = config {
        cmd.arg("--config").arg(c);
    }
    if let Err(e) = cmd.spawn() {
        warn!("cannot open the settings window: {e}");
    }
}

/// Asks Windows to tell us when a HID device (e.g. a receiver) is plugged in.
fn register_device_notifications(hwnd: HWND) {
    let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
        dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
        dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE,
        dbcc_reserved: 0,
        dbcc_classguid: GUID_DEVINTERFACE_HID,
        dbcc_name: [0],
    };
    // SAFETY: `filter` is a valid DEV_BROADCAST_DEVICEINTERFACE_W.
    let handle = unsafe {
        RegisterDeviceNotificationW(
            hwnd as _,
            &filter as *const _ as *const core::ffi::c_void,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        )
    };
    if handle.is_null() {
        warn!("cannot register for device notifications; plug-in detection disabled");
    }
}

/// Stops the daemon and waits for it to give the buttons back.
fn stop_daemon(s: &mut State) {
    let _ = s.tx.send(Event::Quit);
    if let Some(handle) = s.daemon.take() {
        let _ = handle.join();
    }
}

/// Runs `f` on the state if it exists and is not already borrowed.
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_TRAY => {
            match lparam as u32 {
                WM_LBUTTONUP => run_command(Command::ShowDevices),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_APP_UPDATE => {
            drain_updates();
            0
        }
        WM_DEVICECHANGE => {
            if wparam as u32 == DBT_DEVICEARRIVAL {
                with_state(|s| s.tx.send(Event::Rescan));
            }
            1
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wparam != 0 {
                // Windows is logging off or shutting down: restore devices now.
                with_state(stop_daemon);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        m if m != 0 && with_state(|s| s.taskbar_created == m).unwrap_or(false) => {
            // Explorer restarted: the icon must be added again.
            with_state(|s| s.tray.add());
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Applies every queued update from the daemon.
fn drain_updates() {
    let stopped = with_state(|s| {
        let updates: Vec<UiUpdate> = s
            .queue
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        let mut stopped = None;
        for update in updates {
            match update {
                UiUpdate::Snapshot(snap) => {
                    s.tray.set_tip(&snap.tooltip());
                    let battery = s.tray_battery.then(|| lowest_battery(&snap)).flatten();
                    s.tray.set_battery(battery);
                    #[cfg(not(feature = "gui"))]
                    if let Some(w) = &s.devices {
                        w.update(&snap);
                    }
                    s.snapshot = snap;
                }
                UiUpdate::Notice { title, text } => s.tray.notify(&title, &text, true),
                UiUpdate::Stopped(err) => stopped = Some(err),
                UiUpdate::Osd(text) => {
                    if let Some(o) = &s.osd {
                        o.show(&text);
                    }
                }
                UiUpdate::Preferences(p) => {
                    s.tray_battery = p.tray_battery;
                    let battery = p
                        .tray_battery
                        .then(|| lowest_battery(&s.snapshot))
                        .flatten();
                    s.tray.set_battery(battery);
                    scroll::configure(p.scroll_percent, p.smooth_scrolling);
                }
            }
        }
        stopped.map(|err| {
            s.daemon.take().map(JoinHandle::join);
            s.failure.clone_from(&err);
            (s.hwnd, err)
        })
    })
    .flatten();

    // The daemon stopped by itself (fatal error): tell the user and exit.
    if let Some((hwnd, err)) = stopped {
        if let Some(e) = err {
            message_box("logirev", &format!("logirev stopped:\n\n{e}"), true);
        }
        // SAFETY: our own window.
        unsafe { DestroyWindow(hwnd) };
    }
}

/// The lowest battery among connected devices: `(percent, charging)`.
fn lowest_battery(snap: &Snapshot) -> Option<(u8, bool)> {
    snap.links
        .iter()
        .flat_map(|l| &l.devices)
        .filter(|d| d.online)
        .filter_map(|d| d.battery_level)
        .min_by_key(|(p, _)| *p)
}

fn show_menu(hwnd: HWND) {
    let built = with_state(|s| {
        let entries = view::menu(&s.snapshot, autostart::is_enabled(), s.log_file.is_some());
        (entries, s.snapshot.clone())
    });
    let Some((entries, snapshot)) = built else {
        return;
    };
    // The state is not borrowed while the menu is open (nested message loop).
    match tray::show_menu(hwnd, &entries).and_then(|id| view::decode(&snapshot, id)) {
        Some(Picked::Command(cmd)) => run_command(cmd),
        Some(Picked::SendTo { link, slot, host }) => {
            with_state(|s| s.tx.send(Event::ChangeHost { link, slot, host }));
        }
        None => {}
    }
}

fn run_command(cmd: Command) {
    match cmd {
        #[cfg(feature = "gui")]
        Command::ShowDevices => {
            if let Some(custom) = with_state(|s| s.custom_config.then(|| s.config_path.clone())) {
                open_settings(custom.as_deref(), false);
            }
        }
        #[cfg(not(feature = "gui"))]
        Command::ShowDevices => {
            with_state(|s| {
                if let Some(w) = &s.devices {
                    w.update(&s.snapshot);
                    w.show();
                }
            });
        }
        Command::EditSettings => {
            if let Some((hwnd, path)) = with_state(|s| (s.hwnd, s.config_path.clone())) {
                if !util::open_file(hwnd, &path) {
                    message_box("logirev", &format!("Cannot open {}", path.display()), true);
                }
            }
        }
        Command::OpenLog => {
            if let Some((hwnd, Some(path))) = with_state(|s| (s.hwnd, s.log_file.clone())) {
                util::open_file(hwnd, &path);
            }
        }
        Command::OpenFolder => {
            if let Some((hwnd, path)) = with_state(|s| (s.hwnd, s.config_path.clone())) {
                if let Some(dir) = path.parent() {
                    util::open_folder(hwnd, dir);
                }
            }
        }
        Command::Reload => {
            with_state(|s| s.tx.send(Event::Reload));
        }
        Command::Autostart => {
            with_state(|s| {
                let ok = if autostart::is_enabled() {
                    autostart::disable()
                } else {
                    autostart::enable(&s.autostart_command)
                };
                if ok {
                    info!(
                        "start with Windows: {}",
                        if autostart::is_enabled() { "on" } else { "off" }
                    );
                } else {
                    s.tray.notify(
                        "logirev",
                        "Could not change the start-with-Windows setting.",
                        true,
                    );
                }
            });
        }
        Command::Quit => {
            let hwnd = with_state(|s| {
                stop_daemon(s);
                s.hwnd
            });
            if let Some(hwnd) = hwnd {
                // SAFETY: our own window; WM_DESTROY then ends the loop.
                unsafe { DestroyWindow(hwnd) };
            }
        }
    }
}
