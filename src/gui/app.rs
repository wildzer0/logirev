//! The settings window: state, pages and saving.
//!
//! The window shows what the service reports (through [`crate::ipc`]) and
//! edits the configuration file. Every change is saved automatically a
//! moment after the last edit; the service notices the new file and applies
//! it, so the user never has to press "save" or restart anything.
//!
//! Device settings go into a profile matching that device's exact name,
//! created on the first change (a copy of whatever profile applied before),
//! so customising one device never changes another.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ComboBox, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea,
    Sense, Stroke, TextureHandle, ThemePreference, Ui,
};

use super::form::{dpi_slider, ActionForm, FormContext};
use super::i18n::{control_label, fill, Lang, T};
use super::style::{self, icon, palette, ACCENT, BAD, GOOD, WARN};
use crate::config::{
    BacklightMode, Match, RawAction, RawApp, RawBacklight, RawConfig, RawMacro, RawMenu,
    RawMenuItem, RawProfile, RawSmartShift, RawStep, SmartShiftMode,
};
use crate::hidpp::features::{feature_name, parse_control, DeviceKind, DISABLE_KEY_NAMES};
use crate::hidpp::frame::RECEIVER_INDEX;
use crate::hidpp::receiver::MAX_SLOTS;
use crate::hidpp::traffic::TrafficEntry;
use crate::input::keys::MouseButton;
use crate::ipc::{Client, ToService};
use crate::status::{DeviceStatus, LinkStatus, Snapshot};
use crate::update::{self, Release};

/// How long the "settings applied" message stays on screen.
const TOAST_TIME: Duration = Duration::from_millis(2500);
/// Wait after the last edit before saving.
const SAVE_DELAY: Duration = Duration::from_millis(600);
/// How often to retry connecting to the service.
const RECONNECT_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Page {
    Devices,
    Device { link: String, slot: u8 },
    Receiver,
    Menus,
    Macros,
    Advanced,
    Settings,
    About,
}

/// Sections of the advanced page (only after the user unlocks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdvancedTab {
    Features,
    Diagnostics,
    Onboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Buttons,
    Pointer,
    Keyboard,
    Programs,
    Computers,
    Technical,
}

/// Progress of the update check / installation (shared with the worker
/// thread).
#[derive(Clone)]
enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available(Box<Release>),
    Installing,
    Installed,
    Failed(String),
}

enum SaveState {
    Idle,
    Saved(Instant),
    Failed(String),
}

/// The "change this button" dialog.
struct Editor {
    device: String,
    kind: DeviceKind,
    /// `Some(i)`: inside `[[profile.app]]` number `i`.
    app: Option<usize>,
    button_key: String,
    button_label: String,
    gestures: bool,
    dpi_values: Vec<u16>,
    form: ActionForm,
}

/// The "change this menu item" dialog.
struct ItemEditor {
    menu: String,
    /// `Some(i)` edits an existing item, `None` adds one.
    index: Option<usize>,
    label: String,
    icon: Option<String>,
    form: ActionForm,
}

pub struct SettingsApp {
    lang: Lang,
    t: &'static T,
    client: Option<Client>,
    last_attempt: Option<Instant>,
    snapshot: Snapshot,
    /// Version of `snapshot` received from the service.
    snapshot_version: u64,
    config_path: PathBuf,
    raw: RawConfig,
    load_error: Option<String>,
    dirty_at: Option<Instant>,
    save: SaveState,
    page: Page,
    tab: Tab,
    editor: Option<Editor>,
    item_editor: Option<ItemEditor>,
    /// The button row under the mouse (drawn highlighted next frame).
    hovered_row: Option<u16>,
    /// Developer mode: messages frozen by "Pause".
    traffic_paused: Option<Vec<TrafficEntry>>,
    /// Developer mode: show only the messages of the open device.
    traffic_only_device: bool,
    /// Developer mode: when the report was copied.
    report_copied: Option<Instant>,
    /// Outcome of the last export / import.
    backup_result: Option<Result<String, String>>,
    /// Rendered previews of the radial menus, by menu name: `(items, texture)`.
    previews: BTreeMap<String, (String, egui::TextureHandle)>,
    /// Drag in progress on a menu wheel: menu name and source sector.
    wheel_drag: Option<(String, usize)>,
    /// The menu whose details are open in the Radial menus page.
    open_menu: Option<String>,
    /// The user accepted the warning about the advanced HID++ tools.
    advanced_unlocked: bool,
    /// Warning dialog before unlocking the advanced tools.
    advanced_confirm: bool,
    advanced_tab: AdvancedTab,
    advanced_device: Option<(String, u8)>,
    advanced_scanning: bool,
    advanced_scan: Arc<Mutex<Option<Result<String, String>>>>,
    advanced_output: Option<Result<String, String>>,
    /// Command palette (Ctrl+K).
    palette_open: bool,
    palette_query: String,
    palette_focus: bool,
    new_program: String,
    running_programs: Vec<String>,
    logo: Option<TextureHandle>,
    exe: PathBuf,
    custom_config: bool,
    /// Demo mode (`LOGIREV_DEMO=1`): sample devices, nothing is saved.
    /// Used for screenshots and to try the interface without hardware.
    demo: bool,
    update: Arc<Mutex<UpdateStatus>>,
    /// The update banner was dismissed for this session.
    update_dismissed: bool,
    /// An update was installed and the restart has been started.
    restarting: bool,
    /// "Remove pairing?" confirmation: link, slot, device name.
    confirm_unpair: Option<(String, u8, String)>,
}

impl SettingsApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config_path: PathBuf,
        custom_config: bool,
    ) -> Self {
        style::install(&cc.egui_ctx);
        let (raw, load_error) = match RawConfig::load(&config_path) {
            Ok(r) => (r, None),
            Err(e) if !config_path.exists() => {
                let _ = e;
                (
                    RawConfig::parse(crate::config::TEMPLATE).unwrap_or_default(),
                    None,
                )
            }
            Err(e) => (RawConfig::default(), Some(e)),
        };
        let lang = Lang::from_setting(&raw.general.language);
        let app = Self {
            lang,
            t: lang.texts(),
            client: None,
            last_attempt: None,
            snapshot: Snapshot::default(),
            snapshot_version: 0,
            config_path,
            raw,
            load_error,
            dirty_at: None,
            save: SaveState::Idle,
            page: Page::Devices,
            tab: Tab::Buttons,
            editor: None,
            item_editor: None,
            hovered_row: None,
            traffic_paused: None,
            traffic_only_device: true,
            report_copied: None,
            backup_result: None,
            previews: BTreeMap::new(),
            wheel_drag: None,
            open_menu: None,
            advanced_unlocked: false,
            advanced_confirm: false,
            advanced_tab: AdvancedTab::Features,
            advanced_device: None,
            advanced_scanning: false,
            advanced_scan: Arc::new(Mutex::new(None)),
            advanced_output: None,
            palette_open: false,
            palette_query: String::new(),
            palette_focus: false,
            new_program: String::new(),
            running_programs: Vec::new(),
            logo: None,
            exe: std::env::current_exe().unwrap_or_default(),
            custom_config,
            demo: std::env::var_os("LOGIREV_DEMO").is_some(),
            update: Arc::new(Mutex::new(UpdateStatus::Idle)),
            update_dismissed: false,
            restarting: false,
            confirm_unpair: None,
        };
        let mut app = app;
        if app.demo {
            app.snapshot = demo_snapshot();
            app.page = match std::env::var("LOGIREV_PAGE").as_deref() {
                Ok("device") | Ok("pointer") | Ok("computers") | Ok("editor") | Ok("technical") => {
                    Page::Device {
                        link: "demo".into(),
                        slot: 2,
                    }
                }
                Ok("receiver") => Page::Receiver,
                Ok("menus") => Page::Menus,
                Ok("macros") => Page::Macros,
                Ok("advanced") => Page::Advanced,
                Ok("settings") => Page::Settings,
                Ok("about") => Page::About,
                _ => Page::Devices,
            };
            app.tab = match std::env::var("LOGIREV_PAGE").as_deref() {
                Ok("pointer") => Tab::Pointer,
                Ok("computers") => Tab::Computers,
                Ok("technical") => Tab::Technical,
                _ => Tab::Buttons,
            };
            if std::env::var("LOGIREV_PAGE").as_deref() == Ok("editor") {
                app.editor = Some(Editor {
                    device: "MX Ergo S".into(),
                    kind: DeviceKind::Trackball,
                    app: None,
                    button_key: "back".into(),
                    button_label: control_label(app.t, 0x53),
                    gestures: false,
                    dpi_values: (400..=4000).step_by(50).collect(),
                    form: ActionForm::from_raw(Some(&RawAction::Keys {
                        keys: "Alt+Left".into(),
                    })),
                });
            }
        }
        app.apply_theme(&cc.egui_ctx);
        if app.raw.general.check_updates && !app.demo {
            app.check_for_update(&cc.egui_ctx);
        }
        if app.demo {
            use crate::hidpp::pairing::{Family, Found, Stage};
            use crate::status::PairingStatus;
            let found = vec![Found {
                counter: 1,
                address: [1, 2, 3, 4, 5, 6],
                kind: DeviceKind::Mouse,
                authentication: 0,
                name: Some("MX Anywhere 3S".into()),
            }];
            let stage = match std::env::var("LOGIREV_PAIRING").as_deref() {
                Ok("found") => Some(Stage::Waiting),
                Ok("mouse") => Some(Stage::Passkey {
                    passkey: "000613".into(),
                    keyboard: false,
                    entered: 3,
                }),
                Ok("keyboard") => Some(Stage::Passkey {
                    passkey: "482916".into(),
                    keyboard: true,
                    entered: 2,
                }),
                Ok("done") => Some(Stage::Paired { slot: 4 }),
                _ => None,
            };
            if let (Some(stage), Some(link)) = (stage, app.snapshot.links.first_mut()) {
                link.pairing = Some(PairingStatus {
                    family: Family::Bolt,
                    stage,
                    found,
                });
            }
            let release = Release {
                tag_name: "v9.9.0".into(),
                html_url: env!("CARGO_PKG_REPOSITORY").into(),
                body: "- Example release notes\n- Shown in demo mode only".into(),
                assets: Vec::new(),
            };
            Self::set_update(&app.update, UpdateStatus::Available(Box::new(release)));
        }
        app
    }

    fn update_status(&self) -> UpdateStatus {
        self.update
            .lock()
            .map(|s| s.clone())
            .unwrap_or(UpdateStatus::Idle)
    }

    fn set_update(status: &Arc<Mutex<UpdateStatus>>, value: UpdateStatus) {
        if let Ok(mut s) = status.lock() {
            *s = value;
        }
    }

    /// Asks GitHub for a newer version, in the background.
    fn check_for_update(&self, ctx: &egui::Context) {
        let (status, repaint) = (Arc::clone(&self.update), ctx.clone());
        Self::set_update(&status, UpdateStatus::Checking);
        std::thread::spawn(move || {
            let result = match update::check() {
                Ok(Some(release)) => UpdateStatus::Available(Box::new(release)),
                Ok(None) => UpdateStatus::UpToDate,
                Err(e) => UpdateStatus::Failed(e),
            };
            Self::set_update(&status, result);
            repaint.request_repaint();
        });
    }

    /// Downloads and installs `release` in the background.
    fn install_update(&self, ctx: &egui::Context, release: Release) {
        let (status, repaint) = (Arc::clone(&self.update), ctx.clone());
        Self::set_update(&status, UpdateStatus::Installing);
        std::thread::spawn(move || {
            let result = match update::install(&release) {
                Ok(()) => UpdateStatus::Installed,
                Err(e) => UpdateStatus::Failed(e),
            };
            Self::set_update(&status, result);
            repaint.request_repaint();
        });
    }

    /// After installing: stop the old service, start the new version (which
    /// waits for the old one to exit and reopens this window), and close.
    fn restart_after_update(&mut self, ctx: &egui::Context) {
        self.restarting = true;
        self.send(ToService::Quit);
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.arg("--after-update");
        if self.custom_config {
            cmd.arg("--config").arg(&self.config_path);
        }
        if let Err(e) = cmd.spawn() {
            log::warn!("cannot restart logirev: {e}");
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// The "updates" card of the settings page.
    fn updates_ui(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let mut auto = self.raw.general.check_updates;
        if ui.checkbox(&mut auto, t.check_updates_auto).changed() {
            self.raw.general.check_updates = auto;
            self.changed();
        }
        ui.label(
            RichText::new(fill(t.current_version, &[env!("CARGO_PKG_VERSION")])).color(pal.muted),
        );
        let status = self.update_status();
        match &status {
            UpdateStatus::Idle => {}
            UpdateStatus::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t.checking);
                });
            }
            UpdateStatus::UpToDate => {
                ui.label(RichText::new(format!("{} {}", icon::CHECK, t.up_to_date)).color(GOOD));
            }
            UpdateStatus::Available(r) => self.release_ui(ui, r),
            UpdateStatus::Installing => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t.updating);
                });
            }
            UpdateStatus::Installed => {
                ui.label(RichText::new(t.restarting).color(GOOD));
            }
            UpdateStatus::Failed(e) => {
                ui.label(RichText::new(fill(t.update_failed, &[e])).color(BAD));
            }
        }
        let busy = matches!(
            status,
            UpdateStatus::Checking | UpdateStatus::Installing | UpdateStatus::Installed
        );
        if ui
            .add_enabled(
                !busy,
                egui::Button::new(format!("{} {}", icon::ARROWS_CLOCKWISE, t.check_now)),
            )
            .clicked()
        {
            self.check_for_update(ui.ctx());
        }
    }

    /// "Version X is available" with notes and the install button.
    fn release_ui(&mut self, ui: &mut Ui, release: &Release) {
        let t = self.t;
        ui.label(
            RichText::new(fill(t.update_available, &[release.version()]))
                .strong()
                .color(ACCENT),
        );
        ui.horizontal(|ui| {
            if update::can_install() {
                let button = egui::Button::new(RichText::new(t.update_now).color(Color32::WHITE))
                    .fill(ACCENT);
                if ui.add(button).clicked() {
                    self.install_update(ui.ctx(), release.clone());
                }
            }
            ui.hyperlink_to(t.download_page, &release.html_url);
        });
        if !release.body.trim().is_empty() {
            egui::CollapsingHeader::new(t.whats_new)
                .id_salt("notes")
                .show(ui, |ui| {
                    ui.add(egui::Label::new(release.body.trim()).wrap());
                });
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.raw.general.theme.as_str() {
            "light" => ThemePreference::Light,
            "dark" => ThemePreference::Dark,
            _ => ThemePreference::System,
        });
    }

    fn changed(&mut self) {
        self.dirty_at = Some(Instant::now());
    }

    fn connected(&self) -> bool {
        self.demo || self.client.as_ref().is_some_and(Client::is_connected)
    }

    fn send(&self, msg: ToService) {
        if let Some(c) = &self.client {
            let _ = c.send(&msg);
        }
    }

    // ------------------------------------------------------------------
    // Background work (every frame)
    // ------------------------------------------------------------------

    fn tick(&mut self, ctx: &egui::Context) {
        if !self.restarting && matches!(self.update_status(), UpdateStatus::Installed) {
            self.restart_after_update(ctx);
        }
        if self.demo {
            // Nothing is written in the demo; just show the confirmation.
            if self.dirty_at.take().is_some() || std::env::var_os("LOGIREV_TOAST").is_some() {
                self.save = SaveState::Saved(Instant::now());
            }
            return;
        }
        if !self.connected()
            && self
                .last_attempt
                .is_none_or(|t| t.elapsed() >= RECONNECT_EVERY)
        {
            self.last_attempt = Some(Instant::now());
            let repaint = ctx.clone();
            self.client = Client::connect(move || repaint.request_repaint()).ok();
        }
        if let Some((version, snapshot)) = self
            .client
            .as_ref()
            .and_then(|c| c.snapshot_since(self.snapshot_version))
        {
            self.snapshot = snapshot;
            self.snapshot_version = version;
        }
        if let Some(at) = self.dirty_at {
            if at.elapsed() >= SAVE_DELAY {
                self.dirty_at = None;
                self.save = match self.raw.save(&self.config_path) {
                    Ok(()) => {
                        self.send(ToService::Reload);
                        SaveState::Saved(Instant::now())
                    }
                    Err(e) => SaveState::Failed(e),
                };
            } else {
                ctx.request_repaint_after(SAVE_DELAY);
            }
        }
        // Keep trying to connect while the service is down.
        if !self.connected() {
            ctx.request_repaint_after(RECONNECT_EVERY);
        }
    }

    // ------------------------------------------------------------------
    // Profiles
    // ------------------------------------------------------------------

    /// Index of the profile that applies to a device now.
    fn effective_profile(&self, name: &str, kind: DeviceKind) -> Option<usize> {
        self.raw
            .profiles
            .iter()
            .position(|p| p.matcher.fits(name, kind))
    }

    /// The device's own profile, created (as a copy of the one that applied
    /// so far) the first time something is changed.
    fn own_profile(&mut self, name: &str, kind: DeviceKind) -> &mut RawProfile {
        let own = self.raw.profiles.iter().position(|p| {
            p.matcher
                .name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
                && p.matcher.kind.is_none_or(|k| k == kind)
        });
        let index = match own {
            Some(i) => i,
            None => {
                let mut p = self
                    .effective_profile(name, kind)
                    .map(|i| self.raw.profiles[i].clone())
                    .unwrap_or_default();
                p.name = name.to_owned();
                p.matcher = Match {
                    name: Some(name.to_owned()),
                    kind: None,
                };
                self.raw.profiles.insert(0, p);
                0
            }
        };
        &mut self.raw.profiles[index]
    }

    fn profile_view(&self, name: &str, kind: DeviceKind) -> RawProfile {
        self.effective_profile(name, kind)
            .map(|i| self.raw.profiles[i].clone())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Layout
    // ------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(26.0, 26.0)));
            }
            ui.label(RichText::new("logirev").size(17.0).strong());
            ui.label(
                RichText::new(env!("CARGO_PKG_VERSION"))
                    .small()
                    .color(pal.muted),
            );
            ui.add_space(16.0);

            let device_page = matches!(self.page, Page::Device { .. });
            for (page, ic, label) in [
                (Page::Devices, icon::MOUSE, t.nav_devices),
                (Page::Receiver, icon::BROADCAST, t.nav_receiver),
                (Page::Menus, icon::HAND_POINTING, t.nav_menus),
                (Page::Macros, icon::MAGIC_WAND, t.nav_macros),
                (Page::Advanced, icon::CODE, t.nav_advanced),
            ] {
                let selected = self.page == page || (device_page && page == Page::Devices);
                if ui
                    .selectable_label(selected, RichText::new(format!("{ic}  {label}")).size(14.0))
                    .clicked()
                {
                    self.page = page;
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(icon::INFO).on_hover_text(t.nav_about).clicked() {
                    self.page = Page::About;
                }
                if ui
                    .button(icon::GEAR)
                    .on_hover_text(t.nav_settings)
                    .clicked()
                {
                    self.page = Page::Settings;
                }
                if ui
                    .add(egui::Button::new(format!("{}  {}", icon::ZOOM_IN, t.search)).frame(false))
                    .clicked()
                {
                    self.palette_open = true;
                    self.palette_focus = true;
                }
                if self.connected() {
                    status_dot(ui, true, t.service_running);
                } else {
                    if ui
                        .button(format!("{} {}", icon::POWER, t.service_start))
                        .clicked()
                    {
                        start_service(
                            &self.exe,
                            self.custom_config.then_some(self.config_path.as_path()),
                            false,
                        );
                        self.last_attempt = None;
                    }
                    status_dot(ui, false, t.service_stopped);
                }
            });
        });
    }

    /// Ctrl+K: jump to a page or a connected device.
    fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }
        let t = self.t;
        let mut entries: Vec<(Page, &'static str, String)> = vec![
            (Page::Devices, icon::MOUSE, t.nav_devices.to_owned()),
            (Page::Receiver, icon::BROADCAST, t.nav_receiver.to_owned()),
            (Page::Menus, icon::HAND_POINTING, t.nav_menus.to_owned()),
            (Page::Macros, icon::MAGIC_WAND, t.nav_macros.to_owned()),
            (Page::Advanced, icon::CODE, t.nav_advanced.to_owned()),
            (Page::Settings, icon::GEAR, t.nav_settings.to_owned()),
            (Page::About, icon::INFO, t.nav_about.to_owned()),
        ];
        for link in &self.snapshot.links {
            for d in &link.devices {
                entries.push((
                    Page::Device {
                        link: link.key.clone(),
                        slot: d.slot,
                    },
                    style::kind_icon(&d.kind),
                    format!("{}  ·  {}", d.name, link.name),
                ));
            }
        }
        let query = self.palette_query.trim().to_lowercase();
        let filtered: Vec<(Page, &'static str, String)> = entries
            .into_iter()
            .filter(|(_, _, label)| query.is_empty() || label.to_lowercase().contains(&query))
            .collect();
        let mut chosen: Option<Page> = None;
        let mut close = false;
        egui::Modal::new(egui::Id::NULL.with("logirev-palette")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(t.search).strong());
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.palette_query)
                    .hint_text(t.search_hint)
                    .desired_width(f32::INFINITY),
            );
            if self.palette_focus {
                response.request_focus();
                self.palette_focus = false;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (page, ic, label) in &filtered {
                    if ui
                        .selectable_label(false, format!("{ic}  {label}"))
                        .clicked()
                    {
                        chosen = Some(page.clone());
                    }
                }
                if filtered.is_empty() {
                    ui.label(
                        RichText::new(t.search_no_results)
                            .color(palette(ui.visuals().dark_mode).muted),
                    );
                }
            });
            if chosen.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some((page, _, _)) = filtered.first() {
                    chosen = Some(page.clone());
                }
            }
        });
        if let Some(page) = chosen {
            self.page = page;
            close = true;
        }
        if close {
            self.palette_open = false;
            self.palette_query.clear();
        }
    }

    /// A floating message at the bottom of the window while changes are
    /// being saved and applied, then a confirmation for a moment.
    fn save_toast(&self, ctx: &egui::Context) {
        let t = self.t;
        let applying = self.dirty_at.is_some();
        let done = matches!(self.save, SaveState::Saved(at) if at.elapsed() < TOAST_TIME);
        if !applying && !done {
            return;
        }
        egui::Area::new(egui::Id::NULL.with("logirev-save-toast"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
            .interactable(false)
            .show(ctx, |ui| {
                let color = if applying { ACCENT } else { GOOD };
                Frame::new()
                    .fill(color)
                    .corner_radius(CornerRadius::same(20))
                    .inner_margin(Margin::symmetric(18, 10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: Color32::from_black_alpha(60),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if applying {
                                ui.add(egui::Spinner::new().color(Color32::WHITE).size(16.0));
                                ui.label(RichText::new(t.applying).color(Color32::WHITE).strong());
                            } else {
                                ui.label(
                                    RichText::new(format!("{} {}", icon::CHECK_CIRCLE, t.saved))
                                        .color(Color32::WHITE)
                                        .strong(),
                                );
                            }
                        });
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn content(&mut self, ui: &mut Ui) {
        let t = self.t;
        if let Some(e) = &self.load_error {
            banner(ui, BAD, &fill(t.save_error, &[e]));
        }
        if let SaveState::Failed(e) = &self.save {
            banner(ui, BAD, &fill(t.save_error, &[e]));
        }
        match self.page.clone() {
            Page::Devices => self.devices_page(ui),
            Page::Device { link, slot } => self.device_page(ui, &link, slot),
            Page::Receiver => self.receiver_page(ui),
            Page::Menus => self.menus_page(ui),
            Page::Macros => self.macros_page(ui),
            Page::Advanced => self.advanced_page(ui),
            Page::Settings => self.settings_page(ui),
            Page::About => self.about_page(ui),
        }
    }

    // ------------------------------------------------------------------
    // Devices
    // ------------------------------------------------------------------

    fn devices_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.devices_title).size(24.0).strong());
        ui.label(RichText::new(t.devices_subtitle).color(pal.muted));
        ui.add_space(12.0);

        if let UpdateStatus::Available(release) = self.update_status() {
            if !self.update_dismissed {
                Frame::new()
                    .fill(ACCENT.gamma_multiply(0.15))
                    .stroke(Stroke::new(1.0, ACCENT))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(icon::ARROWS_CLOCKWISE)
                                    .size(22.0)
                                    .color(ACCENT),
                            );
                            ui.label(
                                RichText::new(fill(t.update_available, &[release.version()]))
                                    .strong(),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button(t.update_later).clicked() {
                                    self.update_dismissed = true;
                                }
                                if update::can_install() {
                                    let b = egui::Button::new(
                                        RichText::new(t.update_now).color(Color32::WHITE),
                                    )
                                    .fill(ACCENT);
                                    if ui.add(b).clicked() {
                                        self.install_update(ui.ctx(), *release.clone());
                                        self.page = Page::Settings;
                                    }
                                } else {
                                    ui.hyperlink_to(t.download_page, &release.html_url);
                                }
                            });
                        });
                    });
                ui.add_space(12.0);
            }
        }

        if !self.connected() {
            empty_state(ui, icon::POWER, t.service_stopped, t.service_stopped_hint);
            ui.vertical_centered(|ui| {
                if ui
                    .add(egui::Button::new(RichText::new(t.service_start).size(16.0)).fill(ACCENT))
                    .clicked()
                {
                    start_service(
                        &self.exe,
                        self.custom_config.then_some(self.config_path.as_path()),
                        false,
                    );
                    self.last_attempt = None;
                }
            });
            return;
        }
        if self.snapshot.links.is_empty() {
            empty_state(ui, icon::USB, t.no_receiver_title, t.no_receiver_hint);
            return;
        }
        let links = self.snapshot.links.clone();
        for link in &links {
            let link_icon = if !link.is_receiver {
                icon::BLUETOOTH
            } else {
                icon::BROADCAST
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{link_icon}  {}", link.name))
                        .strong()
                        .color(pal.muted),
                );
                if link.can_pair && link.pairing.is_none() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(format!("{} {}", icon::PLUS, t.pair_new))
                            .clicked()
                        {
                            self.send(ToService::StartPairing {
                                link: link.key.clone(),
                            });
                        }
                    });
                }
            });
            if link.devices.is_empty() {
                ui.label(t.no_devices);
            }
            for d in &link.devices {
                self.receiver_slot_row(ui, link, d.slot, Some(d));
            }
            ui.add_space(12.0);
        }
    }

    /// One device card: a click opens the device (if connected); the
    /// right-click menu can remove the pairing.
    fn find_device(&self, link: &str, slot: u8) -> Option<(LinkStatus, DeviceStatus)> {
        let l = self.snapshot.links.iter().find(|l| l.key == link)?;
        let d = l.devices.iter().find(|d| d.slot == slot)?;
        Some((l.clone(), d.clone()))
    }

    fn receiver_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.receiver_title).size(24.0).strong());
        ui.label(RichText::new(t.receiver_hint).color(pal.muted));
        ui.add_space(8.0);

        if !self.connected() {
            empty_state(ui, icon::POWER, t.service_stopped, t.service_stopped_hint);
            return;
        }
        if self.snapshot.links.is_empty() {
            empty_state(ui, icon::USB, t.no_receiver_title, t.no_receiver_hint);
            return;
        }
        let links = self.snapshot.links.clone();
        for link in &links {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{}  {}",
                        if link.is_receiver {
                            icon::BROADCAST
                        } else {
                            icon::BLUETOOTH
                        },
                        link.name
                    ))
                    .size(17.0)
                    .strong(),
                );
                if link.is_receiver {
                    ui.label(
                        RichText::new(format!(
                            "{}/{} {}",
                            link.devices
                                .iter()
                                .filter(|d| d.slot != RECEIVER_INDEX)
                                .count(),
                            MAX_SLOTS,
                            t.receiver_slots
                        ))
                        .small()
                        .color(pal.muted),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if link.can_pair
                        && link.pairing.is_none()
                        && ui
                            .button(format!("{} {}", icon::PLUS, t.pair_new))
                            .clicked()
                    {
                        self.send(ToService::StartPairing {
                            link: link.key.clone(),
                        });
                    }
                    if ui
                        .add(
                            egui::Button::new(format!("{} {}", icon::ARROWS_CLOCKWISE, t.rescan))
                                .frame(false),
                        )
                        .clicked()
                    {
                        self.send(ToService::Rescan);
                    }
                });
            });
            if let Some(pairing) = &link.pairing {
                ui.label(
                    RichText::new(format!("{}  {}", icon::BROADCAST, t.pairing_in_progress))
                        .color(ACCENT),
                );
                let _ = pairing;
            }
            ui.add_space(2.0);
            ui.separator();
            if link.is_receiver {
                for slot in 1..=MAX_SLOTS {
                    let d = link.devices.iter().find(|d| d.slot == slot);
                    self.receiver_slot_row(ui, link, slot, d);
                }
            }
            for d in link.devices.iter().filter(|d| d.slot == RECEIVER_INDEX) {
                self.receiver_slot_row(ui, link, RECEIVER_INDEX, Some(d));
            }
            ui.add_space(8.0);
        }
    }

    fn receiver_slot_row(
        &mut self,
        ui: &mut Ui,
        link: &LinkStatus,
        slot: u8,
        d: Option<&DeviceStatus>,
    ) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.horizontal(|ui| {
            let slot_text = if slot == RECEIVER_INDEX {
                t.direct.to_owned()
            } else {
                fill(t.slot, &[&slot.to_string()])
            };
            ui.label(
                RichText::new(slot_text)
                    .monospace()
                    .small()
                    .color(pal.muted),
            );
            match d {
                Some(d) => {
                    ui.label(
                        RichText::new(style::kind_icon(&d.kind))
                            .size(18.0)
                            .color(if d.online { ACCENT } else { pal.muted }),
                    );
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&d.name).strong());
                            if !d.online {
                                ui.label(RichText::new(t.asleep).small().color(pal.muted))
                                    .on_hover_text(t.asleep_hint);
                            }
                        });
                        let mut details = Vec::new();
                        if let Some(b) = &d.battery {
                            details.push(b.clone());
                        }
                        if let Some(p) = &d.profile {
                            details.push(p.clone());
                        }
                        if d.remapped > 0 {
                            details.push(fill(t.buttons_remapped, &[&d.remapped.to_string()]));
                        }
                        if !details.is_empty() {
                            ui.label(
                                RichText::new(details.join("  ·  "))
                                    .small()
                                    .color(pal.muted),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if link.can_pair
                            && slot != RECEIVER_INDEX
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(icon::TRASH).size(14.0).color(pal.muted),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text(t.unpair)
                                .clicked()
                        {
                            self.confirm_unpair = Some((link.key.clone(), d.slot, d.name.clone()));
                        }
                        if d.online
                            && d.battery.is_some()
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(icon::ARROWS_CLOCKWISE).size(14.0),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text(t.refresh_battery)
                                .clicked()
                        {
                            self.send(ToService::RefreshBattery {
                                link: link.key.clone(),
                                slot: d.slot,
                            });
                        }
                        if d.online && ui.button(t.open).clicked() {
                            self.page = Page::Device {
                                link: link.key.clone(),
                                slot: d.slot,
                            };
                            self.tab = Tab::Buttons;
                        }
                    });
                }
                None => {
                    ui.label(RichText::new(t.receiver_free).color(pal.muted));
                    if link.can_pair && link.pairing.is_none() {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(egui::Button::new(icon::PLUS).frame(false)).clicked() {
                                self.send(ToService::StartPairing {
                                    link: link.key.clone(),
                                });
                            }
                        });
                    }
                }
            }
        });
        ui.separator();
    }

    fn advanced_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.advanced_title).size(24.0).strong());
        ui.label(RichText::new(t.advanced_hint).color(pal.muted));
        ui.add_space(8.0);

        if !self.advanced_unlocked {
            Frame::new()
                .fill(WARN.gamma_multiply(0.08))
                .stroke(Stroke::new(1.0, WARN))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon::WARNING).size(20.0).color(WARN));
                        ui.label(RichText::new(t.advanced_warning_title).size(16.0).strong());
                    });
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(t.advanced_warning).wrap());
                    ui.add_space(8.0);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(t.advanced_enable).color(Color32::WHITE),
                            )
                            .fill(WARN),
                        )
                        .clicked()
                    {
                        self.advanced_confirm = true;
                    }
                });
            return;
        }

        if self.advanced_scanning {
            if let Some(result) = self
                .advanced_scan
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
            {
                self.advanced_scanning = false;
                self.advanced_output = Some(result);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::LOCK).color(GOOD));
            ui.label(RichText::new(t.advanced_readonly).small().color(pal.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(t.advanced_lock).clicked() {
                    self.advanced_unlocked = false;
                }
            });
        });
        ui.add_space(6.0);

        let devices: Vec<(String, u8, String)> = self
            .snapshot
            .links
            .iter()
            .flat_map(|link| {
                link.devices.iter().filter(|d| d.online).map(|d| {
                    (
                        link.key.clone(),
                        d.slot,
                        format!("{}  ·  {}", d.name, link.name),
                    )
                })
            })
            .collect();
        if devices.is_empty() {
            empty_state(ui, icon::MOUSE, t.advanced_no_device, t.no_receiver_hint);
            return;
        }
        let valid = self
            .advanced_device
            .as_ref()
            .is_some_and(|(link, slot)| devices.iter().any(|(l, s, _)| l == link && s == slot));
        if !valid {
            self.advanced_device = devices.first().map(|(l, s, _)| (l.clone(), *s));
        }
        let selected = self.advanced_device.clone().and_then(|(link, slot)| {
            devices
                .iter()
                .find(|(l, s, _)| *l == link && *s == slot)
                .map(|(l, s, label)| (l.clone(), *s, label.clone()))
        });
        let selected_label = selected
            .as_ref()
            .map(|(_, _, label)| label.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new(t.advanced_device).small().color(pal.muted));
            egui::ComboBox::from_id_salt("advanced-device")
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    for (link, slot, label) in &devices {
                        let is_selected = self
                            .advanced_device
                            .as_ref()
                            .is_some_and(|(l, s)| l == link && s == slot);
                        if ui.selectable_label(is_selected, label).clicked() {
                            self.advanced_device = Some((link.clone(), *slot));
                        }
                    }
                });
        });
        let Some((link, slot, _)) = selected else {
            return;
        };
        let Some((_, d)) = self.find_device(&link, slot) else {
            empty_state(ui, icon::WARNING, t.asleep, t.device_gone);
            return;
        };

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (tab, label) in [
                (AdvancedTab::Features, t.advanced_features),
                (AdvancedTab::Diagnostics, t.advanced_diagnostics),
                (AdvancedTab::Onboard, t.advanced_onboard),
            ] {
                if ui
                    .selectable_label(self.advanced_tab == tab, label)
                    .clicked()
                {
                    self.advanced_tab = tab;
                }
            }
        });
        ui.separator();
        ui.add_space(6.0);

        match self.advanced_tab {
            AdvancedTab::Features => self.advanced_features_tab(ui, &d),
            AdvancedTab::Diagnostics => self.advanced_diagnostics_tab(ui, &link, &d),
            AdvancedTab::Onboard => self.advanced_onboard_tab(ui, &d),
        }
    }

    fn advanced_features_tab(&mut self, ui: &mut Ui, d: &DeviceStatus) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let tech = d.details.as_ref().and_then(|x| x.tech.clone());
        if let Some(tech) = &tech {
            egui::Grid::new("advanced-features-grid")
                .num_columns(5)
                .striped(true)
                .spacing([18.0, 5.0])
                .show(ui, |ui| {
                    for header in ["idx", "id", "feature", "ver", "flags"] {
                        ui.label(RichText::new(header).small().color(pal.muted));
                    }
                    ui.end_row();
                    for f in &tech.features {
                        ui.label(RichText::new(format!("{:>3}", f.index)).monospace().small());
                        ui.label(RichText::new(format!("{:#06x}", f.id)).monospace());
                        ui.label(feature_name(f.id));
                        ui.label(RichText::new(format!("v{}", f.version)).monospace().small());
                        ui.label(
                            RichText::new(f.flag_names().join(" "))
                                .small()
                                .color(pal.muted),
                        );
                        ui.end_row();
                    }
                });
        } else {
            ui.label(RichText::new(t.advanced_no_tech).color(pal.muted));
        }
        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new(t.advanced_scan_hint).small().color(pal.muted));
        let mut start = false;
        let mut copy = false;
        ui.horizontal(|ui| {
            if !self.advanced_scanning && ui.button(t.advanced_scan).clicked() {
                start = true;
            }
            if self.advanced_scanning {
                ui.spinner();
                ui.label(RichText::new(t.advanced_scanning).color(pal.muted));
            }
            if let Some(Ok(text)) = &self.advanced_output {
                if ui.button(t.advanced_copy).clicked() {
                    ui.ctx().copy_text(text.clone());
                    copy = true;
                }
            }
            if copy {
                ui.label(RichText::new(t.advanced_copied).small().color(GOOD));
            }
        });
        if start {
            self.start_advanced_scan(ui.ctx());
        }
        match &self.advanced_output {
            Some(Ok(text)) => {
                ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text.clone())
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
            }
            Some(Err(e)) => {
                ui.label(RichText::new(fill(t.advanced_scan_failed, &[e])).color(BAD));
            }
            None if !self.advanced_scanning => {
                ui.label(RichText::new(t.advanced_no_scan).small().color(pal.muted));
            }
            None => {}
        }
    }

    fn advanced_diagnostics_tab(&mut self, ui: &mut Ui, link: &str, d: &DeviceStatus) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let details = d.details.clone().unwrap_or_default();
        egui::Grid::new("advanced-diagnostics-grid")
            .num_columns(2)
            .spacing([18.0, 6.0])
            .show(ui, |ui| {
                let mut row = |label: &str, value: String| {
                    ui.label(RichText::new(label).small().color(pal.muted));
                    ui.label(value);
                    ui.end_row();
                };
                row(t.menu_name, d.name.clone());
                row(
                    "slot",
                    if d.slot == RECEIVER_INDEX {
                        t.direct.to_owned()
                    } else {
                        d.slot.to_string()
                    },
                );
                row("kind", d.kind.clone());
                row(
                    "online",
                    if d.online {
                        t.connected.into()
                    } else {
                        t.asleep.into()
                    },
                );
                if let Some(b) = &d.battery {
                    row("battery", b.clone());
                }
                if !details.firmware.is_empty() {
                    row(t.firmware, details.firmware.clone());
                }
                if let Some(dpi) = d.dpi {
                    row("dpi", dpi.to_string());
                }
                if let Some(profile) = &d.profile {
                    row("profile", profile.clone());
                }
                row(
                    "features",
                    details
                        .tech
                        .as_ref()
                        .map_or("—".into(), |x| x.features.len().to_string()),
                );
                if let Some(hosts) = &d.hosts {
                    row("hosts", format!("{} / {}", hosts.current + 1, hosts.count));
                }
                if !details.report_rates.is_empty() {
                    row("report rates", format!("{:?} Hz", details.report_rates));
                }
            });
        ui.add_space(10.0);
        ui.separator();
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "logirev": env!("CARGO_PKG_VERSION"),
            "link": link,
            "device": d,
            "snapshot": &self.snapshot,
        }))
        .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new(t.advanced_snapshot).strong());
            if ui.button(t.advanced_copy).clicked() {
                ui.ctx().copy_text(json.clone());
            }
        });
        ui.label(
            RichText::new(t.advanced_snapshot_hint)
                .small()
                .color(pal.muted),
        );
        ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut json.clone())
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        });
    }

    fn advanced_onboard_tab(&mut self, ui: &mut Ui, d: &DeviceStatus) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let tech = d.details.as_ref().and_then(|x| x.tech.as_ref());
        let onboard = tech.and_then(|x| x.features.iter().find(|f| f.id == 0x8100));
        if let Some(f) = onboard {
            ui.label(
                RichText::new(format!(
                    "{}  0x8100  v{}  index {}",
                    icon::CPU,
                    f.version,
                    f.index
                ))
                .strong(),
            );
            ui.add_space(4.0);
            ui.add(egui::Label::new(t.advanced_onboard_present).wrap());
        } else {
            ui.add(egui::Label::new(t.advanced_onboard_missing).wrap());
        }
        ui.add_space(8.0);
        ui.label(RichText::new(t.advanced_readonly).small().color(pal.muted));
    }

    fn start_advanced_scan(&mut self, ctx: &egui::Context) {
        if self.advanced_scanning {
            return;
        }
        let exe = self.exe.with_file_name(if cfg!(windows) {
            "logirevctl.exe"
        } else {
            "logirevctl"
        });
        if !exe.exists() {
            self.advanced_output = Some(Err(format!("{} not found", exe.display())));
            return;
        }
        self.advanced_scanning = true;
        self.advanced_output = None;
        let config = self.config_path.clone();
        let result_slot = Arc::clone(&self.advanced_scan);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--config").arg(&config).arg("scan").arg("--hidden");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let result = match cmd.output() {
                Ok(out) => {
                    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                    let err = String::from_utf8_lossy(&out.stderr);
                    if !err.trim().is_empty() {
                        text.push('\n');
                        text.push_str(&err);
                    }
                    if out.status.success() {
                        Ok(text)
                    } else {
                        Err(text)
                    }
                }
                Err(e) => Err(format!("cannot run {}: {e}", exe.display())),
            };
            if let Ok(mut slot) = result_slot.lock() {
                *slot = Some(result);
            }
            ctx.request_repaint();
        });
    }

    fn advanced_warning_dialog(&mut self, ctx: &egui::Context) {
        if !self.advanced_confirm {
            return;
        }
        let t = self.t;
        let mut answer: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::NULL.with("logirev-advanced")).show(ctx, |ui| {
            ui.set_width(540.0);
            ui.label(RichText::new(t.advanced_warning_title).size(18.0).strong());
            ui.add_space(6.0);
            ui.add(egui::Label::new(t.advanced_warning).wrap());
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new(t.advanced_enable).color(Color32::WHITE))
                            .fill(WARN),
                    )
                    .clicked()
                {
                    answer = Some(true);
                }
                if ui.button(t.cancel).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        if let Some(yes) = answer {
            self.advanced_confirm = false;
            if yes {
                self.advanced_unlocked = true;
            }
        }
    }

    fn device_page(&mut self, ui: &mut Ui, link: &str, slot: u8) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        if ui
            .button(format!("{}  {}", icon::CARET_LEFT, t.back))
            .clicked()
        {
            self.page = Page::Devices;
            return;
        }
        let Some((_, d)) = self.find_device(link, slot).filter(|(_, d)| d.online) else {
            empty_state(ui, icon::WARNING, t.asleep, t.device_gone);
            return;
        };
        let kind = kind_of(&d.kind);
        let details = d.details.clone().unwrap_or_default();

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(style::kind_icon(&d.kind))
                    .size(46.0)
                    .color(ACCENT),
            );
            ui.vertical(|ui| {
                ui.label(RichText::new(&d.name).size(22.0).strong());
                ui.horizontal(|ui| {
                    if let Some(b) = &d.battery {
                        let (ic, color) = style::battery_icon(b);
                        ui.label(RichText::new(format!("{ic} {b}")).color(color));
                        if ui
                            .add(
                                egui::Button::new(RichText::new(icon::ARROWS_CLOCKWISE).size(14.0))
                                    .frame(false),
                            )
                            .on_hover_text(t.refresh_battery)
                            .clicked()
                        {
                            self.send(ToService::RefreshBattery {
                                link: link.to_owned(),
                                slot,
                            });
                        }
                        if let Some(age) = d.battery_age {
                            let age = if age < 5 {
                                t.just_now.to_owned()
                            } else {
                                short_age(age)
                            };
                            ui.label(
                                RichText::new(fill(t.battery_updated, &[&age]))
                                    .small()
                                    .color(pal.muted),
                            );
                        }
                    }
                    if !details.firmware.is_empty() {
                        ui.label(
                            RichText::new(fill(t.firmware, &[&details.firmware]))
                                .small()
                                .color(pal.muted),
                        );
                    }
                });
            });
        });
        ui.add_space(10.0);

        // Tabs that make sense for this device.
        let pointer = !details.dpi_values.is_empty()
            || details.pointer_speed
            || details.hires_wheel
            || details.smartshift
            || details.thumb_wheel
            || !details.report_rates.is_empty();
        let keyboard =
            details.fn_keys || details.disable_keys != 0 || details.backlight_levels.is_some();
        let buttons = details.controls.iter().any(|c| c.remappable);
        let computers = d.hosts.as_ref().is_some_and(|h| h.count > 1);
        let mut tabs = Vec::new();
        if buttons {
            tabs.push((Tab::Buttons, icon::CURSOR_CLICK, t.tab_buttons));
        }
        if pointer {
            tabs.push((Tab::Pointer, icon::SLIDERS, t.tab_pointer));
        }
        if keyboard {
            tabs.push((Tab::Keyboard, icon::KEYBOARD, t.tab_keyboard));
        }
        if buttons {
            tabs.push((Tab::Programs, icon::APP_WINDOW, t.tab_programs));
        }
        if computers {
            tabs.push((Tab::Computers, icon::DESKTOP, t.tab_computers));
        }
        if self.raw.general.developer {
            tabs.push((Tab::Technical, icon::CODE, t.tab_tech));
        }
        if !tabs.iter().any(|(tab, ..)| *tab == self.tab) {
            if let Some((first, ..)) = tabs.first() {
                self.tab = *first;
            }
        }
        ui.horizontal(|ui| {
            for (tab, ic, label) in &tabs {
                if ui
                    .selectable_label(
                        self.tab == *tab,
                        RichText::new(format!("{ic}  {label}")).size(15.0),
                    )
                    .clicked()
                {
                    self.tab = *tab;
                    if *tab == Tab::Programs {
                        self.running_programs = running_programs();
                    }
                }
            }
        });
        ui.separator();
        ui.add_space(6.0);

        let link_can_pair = self
            .snapshot
            .links
            .iter()
            .any(|l| l.key == link && l.can_pair);
        if link_can_pair && d.slot != RECEIVER_INDEX {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if ui.button(format!("{} {}", icon::TRASH, t.unpair)).clicked() {
                    self.confirm_unpair = Some((link.to_owned(), d.slot, d.name.clone()));
                }
            });
        }
        match self.tab {
            Tab::Buttons => self.buttons_tab(ui, &d, kind, None),
            Tab::Pointer => self.pointer_tab(ui, &d, kind),
            Tab::Keyboard => self.keyboard_tab(ui, &d, kind),
            Tab::Programs => self.programs_tab(ui, &d, kind),
            Tab::Computers => self.computers_tab(ui, link, &d),
            Tab::Technical => self.technical_tab(ui, link, &d),
        }
    }

    /// The list of buttons, for the device (`app = None`) or for one
    /// `[[profile.app]]` block.
    fn buttons_tab(&mut self, ui: &mut Ui, d: &DeviceStatus, kind: DeviceKind, app: Option<usize>) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let details = d.details.clone().unwrap_or_default();
        let profile = self.profile_view(&d.name, kind);
        if app.is_none() {
            ui.label(RichText::new(t.buttons_hint).color(pal.muted));
            ui.add_space(6.0);
        }
        let mut hovered = None;
        for c in &details.controls {
            // Find the action by control id, not by the key as written: a
            // hand-written file may use "0x00C3" where the window shows
            // "gesture".
            let found = match app {
                None => action_for_control(&profile.buttons, c.cid),
                Some(i) => profile
                    .apps
                    .get(i)
                    .and_then(|a| action_for_control(&a.buttons, c.cid)),
            };
            let current = found.as_ref().map(|(_, action)| action.clone());
            let button_key = found.map(|(key, _)| key).unwrap_or_else(|| c.key.clone());
            let fill_color = if self.hovered_row == Some(c.cid) && c.remappable {
                pal.card_hover
            } else {
                pal.card
            };
            let row = card_row(ui, fill_color, pal.border, |ui| {
                ui.label(
                    RichText::new(icon::CURSOR_CLICK)
                        .size(20.0)
                        .color(if c.remappable { ACCENT } else { pal.muted }),
                );
                ui.vertical(|ui| {
                    ui.label(RichText::new(control_label(t, c.cid)).strong());
                    let summary = if !c.remappable {
                        format!("{} {}", icon::LOCK, t.not_remappable)
                    } else if app.is_some() && current.is_none() {
                        t.same_as_usual.to_owned()
                    } else {
                        let mut s = ActionForm::from_raw(current.as_ref()).summary(t);
                        if c.gestures {
                            s.push_str(&format!("  ·  {}", t.gestures_supported));
                        }
                        s
                    };
                    ui.label(RichText::new(summary).small().color(pal.muted));
                });
                if c.remappable {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} {}", icon::PENCIL, t.change)).color(ACCENT),
                        );
                    });
                }
            });
            if !c.remappable {
                continue;
            }
            // The whole row opens the editor.
            let resp = ui
                .interact(
                    row.rect,
                    egui::Id::NULL.with(("button-row", c.cid)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                hovered = Some(c.cid);
            }
            if resp.clicked() {
                self.editor = Some(Editor {
                    device: d.name.clone(),
                    kind,
                    app,
                    button_key: button_key.clone(),
                    button_label: control_label(t, c.cid),
                    gestures: c.gestures,
                    dpi_values: details.dpi_values.clone(),
                    form: ActionForm::from_raw(current.as_ref()),
                });
            }
        }
        self.hovered_row = hovered;
    }

    /// Developer mode: everything the device says about itself, and the
    /// latest HID++ messages.
    fn technical_tab(&mut self, ui: &mut Ui, link: &str, d: &DeviceStatus) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let mono = |text: String| RichText::new(text).monospace();
        ui.label(RichText::new(t.tech_hint).color(pal.muted));
        ui.add_space(6.0);
        let Some(tech) = d.details.as_ref().and_then(|x| x.tech.clone()) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t.tech_loading);
            });
            return;
        };
        let link_status = self.snapshot.links.iter().find(|l| l.key == link).cloned();

        // Report: copy / save.
        let report = || {
            serde_json::to_string_pretty(&serde_json::json!({
                "logirev": env!("CARGO_PKG_VERSION"),
                "receiver": link_status.as_ref().map(|l| l.name.clone()),
                "device": d,
                "traffic": link_status.as_ref().map(|l| {
                    l.traffic.iter().map(|e| format!(
                        "{} {}", if e.outgoing { "->" } else { "<-" }, hex_bytes(&e.bytes)
                    )).collect::<Vec<_>>()
                }),
            }))
            .unwrap_or_default()
        };
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", icon::CLIPBOARD_TEXT, t.tech_copy))
                .clicked()
            {
                ui.ctx().copy_text(report());
                self.report_copied = Some(Instant::now());
            }
            #[cfg(windows)]
            if ui
                .button(format!("{} {}", icon::FLOPPY_DISK, t.tech_save))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(format!("logirev-{}.json", d.name.replace(' ', "-")))
                    .add_filter("JSON", &["json"])
                    .save_file()
                {
                    let _ = std::fs::write(path, report());
                }
            }
            if self
                .report_copied
                .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
            {
                ui.label(RichText::new(format!("{} {}", icon::CHECK, t.tech_copied)).color(GOOD));
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
        });
        ui.add_space(8.0);

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::INFO, t.tech_identity),
            |ui| {
                egui::Grid::new("tech-identity")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        let mut row = |k: &str, v: String| {
                            ui.label(RichText::new(k).color(pal.muted));
                            ui.label(mono(v));
                            ui.end_row();
                        };
                        row(
                            t.tech_serial,
                            tech.serial.clone().unwrap_or_else(|| "—".into()),
                        );
                        row(t.tech_unit_id, or_dash(&tech.unit_id));
                        row(t.tech_model_ids, or_dash(&tech.model_ids.join(", ")));
                        row(t.tech_transports, or_dash(&tech.transports.join(", ")));
                        if let Some(l) = &link_status {
                            row(t.tech_receiver, l.name.clone());
                        }
                        row(t.tech_slot, d.slot.to_string());
                    });
            },
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::CPU, t.tech_firmware),
            |ui| {
                egui::Grid::new("tech-firmware")
                    .num_columns(3)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        for f in &tech.firmware {
                            ui.label(RichText::new(&f.kind).color(pal.muted));
                            ui.label(mono(or_dash(&f.version)));
                            let mut extra =
                                f.pid.map(|p| format!("PID {p:04X}")).unwrap_or_default();
                            if f.active {
                                extra =
                                    format!("{extra}  {} {}", icon::CHECK_CIRCLE, t.tech_running);
                            }
                            ui.label(RichText::new(extra.trim()).small().color(pal.muted));
                            ui.end_row();
                        }
                    });
            },
        );

        let title = fill(t.tech_features, &[&tech.features.len().to_string()]);
        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {title}", icon::SLIDERS),
            |ui| {
                ui.label(RichText::new(t.tech_features_hint).small().color(pal.muted));
                ui.add_space(4.0);
                egui::Grid::new("tech-features")
                    .num_columns(4)
                    .striped(true)
                    .spacing([18.0, 4.0])
                    .show(ui, |ui| {
                        for f in &tech.features {
                            ui.label(mono(format!("{:>2}", f.index)).color(pal.muted));
                            ui.label(mono(format!("{:#06X}", f.id).replace("0X", "0x")));
                            ui.label(format!("{}  v{}", feature_name(f.id), f.version));
                            ui.horizontal(|ui| {
                                for flag in f.flag_names() {
                                    let color = if flag == "obsolete" { pal.muted } else { WARN };
                                    ui.label(RichText::new(flag).small().color(color));
                                }
                            });
                            ui.end_row();
                        }
                    });
            },
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::CODE, t.tech_traffic),
            |ui| {
                ui.label(
                    RichText::new(fill(
                        t.tech_traffic_hint,
                        &[icon::ARROW_RIGHT, icon::ARROW_LEFT],
                    ))
                    .small()
                    .color(pal.muted),
                );
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.traffic_only_device, t.tech_only_device);
                    let paused = self.traffic_paused.is_some();
                    let label = if paused {
                        format!("{} {}", icon::PLAY, t.tech_resume)
                    } else {
                        format!("{} {}", icon::PAUSE, t.tech_pause)
                    };
                    if ui.button(label).clicked() {
                        self.traffic_paused = if paused {
                            None
                        } else {
                            Some(
                                link_status
                                    .as_ref()
                                    .map(|l| l.traffic.clone())
                                    .unwrap_or_default(),
                            )
                        };
                    }
                });
                let live = link_status
                    .as_ref()
                    .map(|l| l.traffic.clone())
                    .unwrap_or_default();
                let entries = self.traffic_paused.clone().unwrap_or(live);
                let shown: Vec<&TrafficEntry> = entries
                    .iter()
                    .filter(|e| !self.traffic_only_device || e.bytes.get(1) == Some(&d.slot))
                    .collect();
                if shown.is_empty() {
                    ui.label(RichText::new(t.tech_no_traffic).color(pal.muted));
                    return;
                }
                ScrollArea::vertical()
                    .id_salt("tech-traffic")
                    .max_height(320.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let mut previous: Option<u64> = None;
                        for e in shown {
                            let gap = previous.map_or(0, |p| e.time_ms.saturating_sub(p));
                            previous = Some(e.time_ms);
                            let (arrow, color) = if e.outgoing {
                                ("→", ACCENT)
                            } else {
                                ("←", GOOD)
                            };
                            ui.horizontal(|ui| {
                                ui.label(mono(format!("+{:>5} ms", gap)).small().color(pal.muted));
                                ui.label(RichText::new(arrow).color(color));
                                ui.label(mono(hex_bytes(&e.bytes)).small());
                                ui.label(
                                    RichText::new(describe_frame(&e.bytes, &tech.features))
                                        .small()
                                        .color(pal.muted),
                                );
                            });
                        }
                    });
            },
        );
    }

    fn pointer_tab(&mut self, ui: &mut Ui, d: &DeviceStatus, kind: DeviceKind) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let details = d.details.clone().unwrap_or_default();
        let view = self.profile_view(&d.name, kind);
        let mut p = view.clone();

        section(ui, pal.card, pal.border, t.speed_title, |ui| {
            if !details.dpi_values.is_empty() {
                ui.label(RichText::new(t.dpi).strong());
                ui.label(RichText::new(t.dpi_hint).small().color(pal.muted));
                let mut choose = p.dpi.is_some();
                if segmented(
                    ui,
                    &mut choose,
                    &[(false, t.keep_device_value), (true, t.choose_value)],
                ) {
                    p.dpi = choose.then(|| d.dpi.unwrap_or(details.dpi_values[0]));
                }
                match p.dpi.as_mut() {
                    Some(dpi) => {
                        dpi_slider(ui, dpi, &details.dpi_values);
                        // Common values, one click each.
                        ui.horizontal_wrapped(|ui| {
                            for v in [400u16, 800, 1000, 1600, 2400, 3200, 4000] {
                                if !details.dpi_values.contains(&v) {
                                    continue;
                                }
                                if ui.selectable_label(*dpi == v, v.to_string()).clicked() {
                                    *dpi = v;
                                }
                            }
                        });
                    }
                    None => {
                        let now = d
                            .dpi
                            .map(|v| fill(t.device_value_now, &[&format!("{v} DPI")]))
                            .unwrap_or_else(|| t.device_value_unknown.to_owned());
                        ui.label(RichText::new(now).color(pal.muted));
                    }
                }
                ui.add_space(6.0);
            }
            if details.pointer_speed {
                ui.label(RichText::new(t.pointer_speed).strong());
                let mut choose = p.pointer_speed.is_some();
                if segmented(
                    ui,
                    &mut choose,
                    &[(false, t.keep_device_value), (true, t.choose_value)],
                ) {
                    p.pointer_speed = choose.then_some(1.0);
                }
                if let Some(speed) = p.pointer_speed.as_mut() {
                    ui.add(
                        egui::Slider::new(
                            speed,
                            crate::config::POINTER_SPEED_MIN..=crate::config::POINTER_SPEED_MAX,
                        )
                        .step_by(0.05)
                        .suffix("×"),
                    );
                }
                ui.add_space(6.0);
            }
            if !details.report_rates.is_empty() {
                ui.label(RichText::new(t.report_rate).strong());
                ComboBox::from_id_salt("rate")
                    .selected_text(
                        p.report_rate
                            .map(|r| format!("{r} Hz"))
                            .unwrap_or_else(|| t.not_set.into()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.report_rate, None, t.not_set);
                        for r in &details.report_rates {
                            ui.selectable_value(&mut p.report_rate, Some(*r), format!("{r} Hz"));
                        }
                    });
            }
        });

        if details.hires_wheel || details.smartshift || details.thumb_wheel {
            section(ui, pal.card, pal.border, t.wheel_title, |ui| {
                if details.hires_wheel {
                    tri_state(ui, &mut p.hires_wheel, t.hires_wheel, t);
                    ui.label(RichText::new(t.hires_hint).small().color(pal.muted));
                    tri_state(ui, &mut p.invert_wheel, t.invert_wheel, t);
                }
                if details.smartshift {
                    ui.label(RichText::new(t.smartshift).strong());
                    let mode = p.smartshift.as_ref().map(|s| s.mode);
                    let label = |m: Option<SmartShiftMode>| match m {
                        None => t.not_set,
                        Some(SmartShiftMode::Smart) => t.smartshift_smart,
                        Some(SmartShiftMode::Ratchet) => t.smartshift_ratchet,
                        Some(SmartShiftMode::Free) => t.smartshift_free,
                    };
                    let mut chosen = mode;
                    ComboBox::from_id_salt("smartshift")
                        .selected_text(label(mode))
                        .show_ui(ui, |ui| {
                            for m in [
                                None,
                                Some(SmartShiftMode::Smart),
                                Some(SmartShiftMode::Ratchet),
                                Some(SmartShiftMode::Free),
                            ] {
                                ui.selectable_value(&mut chosen, m, label(m));
                            }
                        });
                    if chosen != mode {
                        p.smartshift = chosen.map(|mode| RawSmartShift {
                            mode,
                            threshold: Some(10),
                        });
                    }
                    if let Some(s) = p
                        .smartshift
                        .as_mut()
                        .filter(|s| s.mode == SmartShiftMode::Smart)
                    {
                        let mut th = s.threshold.unwrap_or(10);
                        ui.add(egui::Slider::new(&mut th, 1..=50).text(t.smartshift_threshold));
                        s.threshold = Some(th);
                    }
                }
                if details.thumb_wheel {
                    tri_state(ui, &mut p.thumb_wheel_invert, t.thumb_wheel_invert, t);
                }
            });
        }

        if !pointer_differs(&view, &p) {
            return;
        }
        let own = self.own_profile(&d.name, kind);
        own.dpi = p.dpi;
        own.pointer_speed = p.pointer_speed;
        own.report_rate = p.report_rate;
        own.hires_wheel = p.hires_wheel;
        own.invert_wheel = p.invert_wheel;
        own.smartshift = p.smartshift;
        own.thumb_wheel_invert = p.thumb_wheel_invert;
        self.changed();
    }

    fn keyboard_tab(&mut self, ui: &mut Ui, d: &DeviceStatus, kind: DeviceKind) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let details = d.details.clone().unwrap_or_default();
        let view = self.profile_view(&d.name, kind);
        let mut fn_keys = view.function_keys;
        let mut disabled: Vec<String> = view.disable_keys.clone().unwrap_or_default();
        let mut backlight = view.backlight.as_ref().map(|b| (b.mode, b.level));

        if details.fn_keys {
            section(ui, pal.card, pal.border, t.fn_keys, |ui| {
                use crate::config::FunctionKeys as F;
                ui.radio_value(&mut fn_keys, None, t.not_set);
                ui.radio_value(&mut fn_keys, Some(F::Standard), t.fn_standard);
                ui.radio_value(&mut fn_keys, Some(F::Special), t.fn_special);
            });
        }
        if details.disable_keys != 0 {
            section(ui, pal.card, pal.border, t.disable_keys, |ui| {
                for (name, bit) in DISABLE_KEY_NAMES {
                    if details.disable_keys & bit == 0 {
                        continue;
                    }
                    let mut on = disabled.iter().any(|k| k.eq_ignore_ascii_case(name));
                    if ui.checkbox(&mut on, key_display(name)).changed() {
                        disabled.retain(|k| !k.eq_ignore_ascii_case(name));
                        if on {
                            disabled.push((*name).to_owned());
                        }
                    }
                }
            });
        }
        if let Some(levels) = details.backlight_levels {
            section(ui, pal.card, pal.border, t.backlight, |ui| {
                let mode = backlight.map(|b| b.0);
                let mut chosen = mode;
                ui.radio_value(&mut chosen, None, t.not_set);
                ui.radio_value(&mut chosen, Some(BacklightMode::Off), t.backlight_off);
                ui.radio_value(&mut chosen, Some(BacklightMode::On), t.backlight_on);
                if details.backlight_auto {
                    ui.radio_value(&mut chosen, Some(BacklightMode::Auto), t.backlight_auto);
                }
                ui.radio_value(&mut chosen, Some(BacklightMode::Manual), t.backlight_manual);
                if chosen != mode {
                    backlight =
                        chosen.map(|m| (m, (m == BacklightMode::Manual).then_some(levels / 2)));
                }
                if let Some((BacklightMode::Manual, level)) = backlight.as_mut() {
                    let mut l = level.unwrap_or(0);
                    ui.add(egui::Slider::new(&mut l, 0..=levels.saturating_sub(1)).text(t.level));
                    *level = Some(l);
                }
            });
        }

        let new_disabled = (!disabled.is_empty()).then_some(disabled);
        let new_backlight = backlight.map(|(mode, level)| RawBacklight { mode, level });
        let old_backlight = view.backlight.as_ref().map(|b| (b.mode, b.level));
        if fn_keys == view.function_keys
            && new_disabled == view.disable_keys
            && new_backlight.as_ref().map(|b| (b.mode, b.level)) == old_backlight
        {
            return;
        }
        let own = self.own_profile(&d.name, kind);
        own.function_keys = fn_keys;
        own.disable_keys = new_disabled;
        own.backlight = new_backlight;
        self.changed();
    }

    fn programs_tab(&mut self, ui: &mut Ui, d: &DeviceStatus, kind: DeviceKind) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        if !cfg!(windows) {
            ui.label(t.programs_windows_only);
        }
        ui.label(RichText::new(t.programs_hint).color(pal.muted));
        ui.add_space(6.0);

        // Add a program.
        let mut add: Option<String> = None;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_program)
                    .hint_text(t.program_name)
                    .desired_width(240.0),
            );
            if ui
                .button(format!("{} {}", icon::PLUS, t.add_program))
                .clicked()
                && !self.new_program.trim().is_empty()
            {
                add = Some(self.new_program.trim().to_owned());
            }
            if !self.running_programs.is_empty() {
                ComboBox::from_id_salt("running")
                    .selected_text(t.running_programs)
                    .height(360.0)
                    .show_ui(ui, |ui| {
                        for p in &self.running_programs {
                            if ui.selectable_label(false, p).clicked() {
                                add = Some(p.clone());
                            }
                        }
                    });
            }
        });
        if let Some(program) = add {
            self.new_program.clear();
            let own = self.own_profile(&d.name, kind);
            if !own
                .apps
                .iter()
                .any(|a| a.programs.iter().any(|p| p.eq_ignore_ascii_case(&program)))
            {
                own.apps.push(RawApp {
                    programs: vec![program],
                    buttons: Default::default(),
                });
                self.changed();
            }
        }

        let apps = self.profile_view(&d.name, kind).apps;
        let mut remove = None;
        for (i, a) in apps.iter().enumerate() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{}  {}",
                        icon::APP_WINDOW,
                        fill(t.when_in, &[&a.programs.join(", ")])
                    ))
                    .size(17.0)
                    .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(format!("{} {}", icon::TRASH, t.remove)).clicked() {
                        remove = Some(i);
                    }
                });
            });
            self.buttons_tab(ui, d, kind, Some(i));
        }
        if let Some(i) = remove {
            let own = self.own_profile(&d.name, kind);
            if i < own.apps.len() {
                own.apps.remove(i);
                self.changed();
            }
        }
    }

    fn computers_tab(&mut self, ui: &mut Ui, link: &str, d: &DeviceStatus) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        let Some(h) = &d.hosts else { return };
        ui.label(RichText::new(t.computers_hint).color(pal.muted));
        ui.add_space(6.0);
        for host in 0..h.count {
            card_row(ui, pal.card, pal.border, |ui| {
                ui.label(RichText::new(icon::DESKTOP).size(22.0).color(ACCENT));
                ui.label(RichText::new(h.label(host)).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if host == h.current {
                        ui.label(
                            RichText::new(format!("{} {}", icon::CHECK, t.computer_current))
                                .color(GOOD),
                        );
                    } else if ui.button(t.send_here).clicked() {
                        self.send(ToService::ChangeHost {
                            link: link.to_owned(),
                            slot: d.slot,
                            host,
                        });
                    }
                });
            });
        }
    }

    // ------------------------------------------------------------------
    // Button editor (modal)
    // ------------------------------------------------------------------

    fn editor_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.editor.take() else {
            return;
        };
        let t = self.t;
        let mut result: Option<bool> = None; // Some(true) = save
        let modal = egui::Modal::new(egui::Id::NULL.with("logirev-editor")).show(ctx, |ui| {
            ui.set_width(600.0);
            ui.label(
                RichText::new(fill(t.edit_button, &[&ed.button_label]))
                    .size(20.0)
                    .strong(),
            );
            ui.label(
                RichText::new(fill(t.current_action, &[&ed.form.summary(t)]))
                    .color(palette(ui.visuals().dark_mode).muted),
            );
            ui.add_space(8.0);
            let fc = FormContext {
                t,
                dpi_values: &ed.dpi_values,
                gestures: ed.gestures,
                macros: self.raw.macros.keys().cloned().collect(),
                pointing: ed.kind.is_pointing(),
                menus: self.raw.menus.keys().cloned().collect(),
                menu_item: false,
                dark: ui.visuals().dark_mode,
            };
            ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 180.0).max(240.0))
                .show(ui, |ui| {
                    ed.form.ui(ui, &fc, "editor", false);
                });
            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new(t.save).color(Color32::WHITE)).fill(ACCENT),
                    )
                    .clicked()
                {
                    result = Some(true);
                }
                if ui.button(t.cancel).clicked() {
                    result = Some(false);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(format!("{} {}", icon::ARROW_U_UP_LEFT, t.reset_default))
                        .clicked()
                    {
                        ed.form = ActionForm::default();
                        result = Some(true);
                    }
                });
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }
        match result {
            Some(true) => {
                let action = ed.form.to_raw();
                let own = self.own_profile(&ed.device, ed.kind);
                match ed.app {
                    None => match action {
                        Some(a) => {
                            own.buttons.insert(ed.button_key.clone(), a);
                        }
                        None => {
                            own.buttons.remove(&ed.button_key);
                        }
                    },
                    Some(i) => {
                        if let Some(app) = own.apps.get_mut(i) {
                            // Inside a program, "default" must be written
                            // explicitly to override the device's mapping.
                            app.buttons.insert(
                                ed.button_key.clone(),
                                action.unwrap_or(RawAction::Default),
                            );
                        }
                    }
                }
                self.changed();
            }
            Some(false) => {}
            None => self.editor = Some(ed),
        }
    }

    // ------------------------------------------------------------------
    // Pairing (modal)
    // ------------------------------------------------------------------

    /// The pairing assistant, shown while a receiver is in pairing mode.
    fn pairing_dialog(&mut self, ctx: &egui::Context) {
        use crate::hidpp::pairing::{mouse_clicks, Family, Stage};
        let Some(link) = self
            .snapshot
            .links
            .iter()
            .find(|l| l.pairing.is_some())
            .cloned()
        else {
            return;
        };
        let Some(p) = link.pairing.clone() else {
            return;
        };
        let t = self.t;
        let key = link.key.clone();
        let mut send: Option<ToService> = None;
        egui::Modal::new(egui::Id::NULL.with("logirev-pairing")).show(ctx, |ui| {
            ui.set_width(500.0);
            let pal = palette(ui.visuals().dark_mode);
            ui.label(
                RichText::new(format!("{}  {}", icon::BROADCAST, t.pairing_title))
                    .size(20.0)
                    .strong(),
            );
            ui.label(RichText::new(&link.name).small().color(pal.muted));
            ui.add_space(10.0);
            match &p.stage {
                Stage::Waiting => {
                    let hint = if p.family == Family::Bolt {
                        t.pairing_bolt_hint
                    } else {
                        t.pairing_unifying_hint
                    };
                    ui.add(egui::Label::new(RichText::new(hint).size(16.0)).wrap());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t.searching);
                    });
                    // Bounded height: the buttons below must stay reachable.
                    ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for (i, f) in p.found.iter().enumerate() {
                            let ready = f.address != [0; 6];
                            card_row(ui, pal.card, pal.border, |ui| {
                                ui.label(
                                    RichText::new(style::kind_icon(&f.kind.to_string()))
                                        .size(22.0)
                                        .color(ACCENT),
                                );
                                ui.label(
                                    RichText::new(
                                        f.name.clone().unwrap_or_else(|| f.kind.to_string()),
                                    )
                                    .strong(),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let b = egui::Button::new(
                                        RichText::new(t.pair_this).color(Color32::WHITE),
                                    )
                                    .fill(ACCENT);
                                    if ui.add_enabled(ready, b).clicked() {
                                        send = Some(ToService::ChoosePairing {
                                            link: key.clone(),
                                            index: i,
                                        });
                                    }
                                });
                            });
                        }
                    });
                }
                Stage::Pairing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(t.pairing_in_progress).size(16.0));
                    });
                }
                Stage::Passkey {
                    passkey,
                    keyboard: true,
                    entered,
                } => {
                    ui.label(RichText::new(t.passkey_keyboard).size(16.0));
                    ui.add_space(6.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(passkey)
                                .size(44.0)
                                .strong()
                                .monospace()
                                .color(ACCENT),
                        );
                        ui.label(
                            RichText::new(format!("{} / {}", entered, passkey.len()))
                                .color(pal.muted),
                        );
                    });
                }
                Stage::Passkey {
                    passkey,
                    keyboard: false,
                    entered,
                } => {
                    ui.label(RichText::new(t.passkey_mouse).size(16.0));
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                        for (i, right) in mouse_clicks(passkey).iter().enumerate() {
                            let done = i < usize::from(*entered);
                            let (ic, label) = if *right {
                                (icon::CARET_RIGHT, t.right_short)
                            } else {
                                (icon::CARET_LEFT, t.left_short)
                            };
                            let color = if done { GOOD } else { ACCENT };
                            Frame::new()
                                .stroke(Stroke::new(1.5, color))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(Margin::symmetric(2, 4))
                                .show(ui, |ui| {
                                    // Fixed-size box: a wrapping row must not
                                    // stretch it.
                                    ui.set_width(40.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(format!("{}", i + 1))
                                                .small()
                                                .color(pal.muted),
                                        );
                                        ui.label(RichText::new(ic).size(20.0).color(color));
                                        ui.label(RichText::new(label).size(10.0).color(color));
                                    });
                                });
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(t.passkey_mouse_finish).strong());
                }
                Stage::Paired { slot } => {
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            icon::CHECK_CIRCLE,
                            fill(t.paired_ok, &[&slot.to_string()])
                        ))
                        .size(17.0)
                        .color(GOOD),
                    );
                }
                Stage::Failed { reason } => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(fill(t.pairing_failed, &[reason])).color(BAD),
                        )
                        .wrap(),
                    );
                }
                Stage::Cancelled => {
                    ui.label(t.pairing_cancelled);
                }
            }
            ui.add_space(12.0);
            ui.separator();
            ui.horizontal(|ui| {
                if p.stage.is_finished() {
                    if !matches!(p.stage, Stage::Paired { .. })
                        && ui
                            .add(
                                egui::Button::new(RichText::new(t.try_again).color(Color32::WHITE))
                                    .fill(ACCENT),
                            )
                            .clicked()
                    {
                        send = Some(ToService::StartPairing { link: key.clone() });
                    }
                    if ui.button(t.close).clicked() {
                        send = Some(ToService::StopPairing { link: key.clone() });
                    }
                } else if ui.button(t.cancel).clicked() {
                    send = Some(ToService::StopPairing { link: key.clone() });
                }
            });
        });
        if let Some(msg) = send {
            self.send(msg);
        }
        // Keep the spinner and progress moving while pairing.
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    /// "Remove pairing?" confirmation.
    fn unpair_dialog(&mut self, ctx: &egui::Context) {
        let Some((link, slot, name)) = self.confirm_unpair.clone() else {
            return;
        };
        let t = self.t;
        let mut answer: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::NULL.with("logirev-unpair")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                RichText::new(fill(t.unpair_confirm, &[&name]))
                    .size(18.0)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.add(egui::Label::new(t.unpair_hint).wrap());
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new(t.unpair).color(Color32::WHITE)).fill(BAD);
                if ui.add(b).clicked() {
                    answer = Some(true);
                }
                if ui.button(t.cancel).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        if let Some(yes) = answer {
            self.confirm_unpair = None;
            if yes {
                self.send(ToService::Unpair { link, slot });
                if matches!(&self.page, Page::Device { slot: s, .. } if *s == slot) {
                    self.page = Page::Devices;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Macros
    // ------------------------------------------------------------------

    fn macros_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.macros_title).size(24.0).strong());
        ui.label(RichText::new(t.macros_hint).color(pal.muted));
        ui.add_space(8.0);
        if ui
            .button(format!("{} {}", icon::PLUS, t.new_macro))
            .clicked()
        {
            let mut n = self.raw.macros.len() + 1;
            while self.raw.macros.contains_key(&format!("macro{n}")) {
                n += 1;
            }
            self.raw.macros.insert(
                format!("macro{n}"),
                RawMacro {
                    steps: vec![RawStep {
                        keys: Some("Ctrl+C".into()),
                        ..Default::default()
                    }],
                },
            );
            self.changed();
        }

        let names: Vec<String> = self.raw.macros.keys().cloned().collect();
        let mut changed = false;
        let mut rename: Option<(String, String)> = None;
        let mut delete: Option<String> = None;
        for name in names {
            let Some(m) = self.raw.macros.get_mut(&name) else {
                continue;
            };
            ui.add_space(8.0);
            section(
                ui,
                pal.card,
                pal.border,
                &format!("{}  {name}", icon::MAGIC_WAND),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(t.macro_name);
                        // The draft lives in egui memory until the field loses
                        // focus: rebuilding it from `name` every frame would
                        // discard what the user types.
                        let draft_id = ui.scope_id().with(("macro-name", &name));
                        let mut new_name = ui
                            .ctx()
                            .data(|d| d.get_temp::<String>(draft_id))
                            .unwrap_or_else(|| name.clone());
                        let resp =
                            ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(200.0));
                        if resp.changed() {
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(draft_id, new_name.clone()));
                        }
                        if resp.lost_focus() {
                            ui.ctx().data_mut(|d| d.remove::<String>(draft_id));
                            let clean: String = new_name
                                .trim()
                                .chars()
                                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                                .collect();
                            if !clean.is_empty() && clean != name {
                                rename = Some((name.clone(), clean));
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(format!("{} {}", icon::TRASH, t.delete)).clicked() {
                                delete = Some(name.clone());
                            }
                        });
                    });
                    changed |= steps_editor(ui, &mut m.steps, t, &name);
                },
            );
        }
        if let Some((old, new)) = rename {
            if !self.raw.macros.contains_key(&new) {
                if let Some(m) = self.raw.macros.remove(&old) {
                    self.raw.macros.insert(new.clone(), m);
                    rename_macro_uses(&mut self.raw, &old, &new);
                    changed = true;
                }
            }
        }
        if let Some(name) = delete {
            self.raw.macros.remove(&name);
            remove_macro_uses(&mut self.raw, &name);
            changed = true;
        }
        if changed {
            self.changed();
        }
    }

    // ------------------------------------------------------------------
    // Radial menus
    // ------------------------------------------------------------------

    fn menus_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.menus_title).size(24.0).strong());
        ui.label(RichText::new(t.menus_hint).color(pal.muted));
        ui.add_space(8.0);
        if ui
            .button(format!("{}  {}", icon::PLUS, t.new_menu))
            .clicked()
        {
            let mut n = self.raw.menus.len() + 1;
            while self.raw.menus.contains_key(&format!("menu{n}")) {
                n += 1;
            }
            let name = format!("menu{n}");
            self.raw.menus.insert(
                name.clone(),
                RawMenu {
                    title: None,
                    items: Vec::new(),
                },
            );
            self.open_menu = Some(name);
            self.changed();
        }
        if self.raw.menus.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new(t.no_menus).color(pal.muted));
            return;
        }
        // The selected menu may have disappeared after an external edit.
        if self
            .open_menu
            .as_deref()
            .is_some_and(|name| !self.raw.menus.contains_key(name))
        {
            self.open_menu = None;
        }

        let names: Vec<String> = self.raw.menus.keys().cloned().collect();
        let mut open_menu = self.open_menu.clone();
        let mut changed = false;
        let mut rename: Option<(String, String)> = None;
        let mut delete: Option<String> = None;
        let mut edit: Option<(String, Option<usize>)> = None;
        let mut remove_item: Option<(String, usize)> = None;

        for name in &names {
            let Some(menu) = self.raw.menus.get_mut(name) else {
                continue;
            };
            let open = open_menu.as_deref() == Some(name.as_str());
            let title = menu
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty());
            let header = Frame::new()
                .fill(if open { pal.card_hover } else { pal.card })
                .stroke(Stroke::new(1.0, if open { ACCENT } else { pal.border }))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if open {
                                icon::CARET_DOWN
                            } else {
                                icon::CARET_RIGHT
                            })
                            .size(14.0)
                            .color(pal.muted),
                        );
                        ui.label(RichText::new(name).strong().size(15.0));
                        if let Some(title) = title {
                            ui.label(
                                RichText::new(format!("· {}", shorten_text(title, 34)))
                                    .small()
                                    .color(pal.muted),
                            );
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{}/{}",
                                    menu.items.len(),
                                    crate::radial::MAX_ITEMS
                                ))
                                .small()
                                .color(pal.muted),
                            );
                        });
                    });
                });
            let response = ui
                .interact(
                    header.response.rect,
                    ui.scope_id().with(("menu-header", name.as_str())),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                open_menu = if open { None } else { Some(name.clone()) };
            }
            if !open {
                ui.add_space(6.0);
                continue;
            }

            ui.add_space(4.0);
            Frame::new()
                .fill(pal.card)
                .stroke(Stroke::new(1.0, pal.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t.menu_name).small().color(pal.muted));
                        // The draft lives in egui memory until the field loses
                        // focus: rebuilding it from `name` every frame would
                        // discard what the user types.
                        let draft_id = ui.scope_id().with(("menu-name", name));
                        let mut new_name = ui
                            .ctx()
                            .data(|d| d.get_temp::<String>(draft_id))
                            .unwrap_or_else(|| name.clone());
                        let resp =
                            ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(180.0));
                        if resp.changed() {
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(draft_id, new_name.clone()));
                        }
                        if resp.lost_focus() {
                            ui.ctx().data_mut(|d| d.remove::<String>(draft_id));
                            let clean: String = new_name
                                .trim()
                                .chars()
                                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                                .collect();
                            if !clean.is_empty() && clean != *name {
                                rename = Some((name.clone(), clean));
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(icon::TRASH).size(14.0))
                                        .frame(false),
                                )
                                .on_hover_text(t.delete)
                                .clicked()
                            {
                                delete = Some(name.clone());
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t.menu_title).small().color(pal.muted));
                        let mut title = menu.title.clone().unwrap_or_default();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut title)
                                    .desired_width(260.0)
                                    .hint_text(t.menu_title),
                            )
                            .changed()
                        {
                            menu.title = (!title.is_empty()).then_some(title);
                            changed = true;
                        }
                    });
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!(
                            "{}: {}/{}",
                            t.menu_items,
                            menu.items.len(),
                            crate::radial::MAX_ITEMS
                        ))
                        .small()
                        .color(pal.muted),
                    );
                    let len = menu.items.len();
                    if len > 0 {
                        ui.add_space(4.0);
                        ui.vertical_centered(|ui| {
                            menu_editor(
                                ui,
                                name,
                                menu,
                                &mut self.previews,
                                &mut self.wheel_drag,
                                &mut edit,
                                &mut changed,
                            );
                            ui.label(RichText::new(t.wheel_hint).small().color(pal.muted));
                        });
                        ui.add_space(4.0);
                    } else {
                        ui.label(RichText::new(t.menu_empty).small().color(pal.muted));
                    }
                    for (i, item) in menu.items.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(
                                    item.icon
                                        .as_deref()
                                        .and_then(crate::radial::icon_glyph)
                                        .unwrap_or("•"),
                                )
                                .size(18.0)
                                .color(ACCENT),
                            );
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&item.label).strong());
                                let summary = ActionForm::from_raw(Some(&item.action)).summary(t);
                                ui.label(RichText::new(summary).small().color(pal.muted));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(icon::TRASH).size(14.0).color(pal.muted),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(t.delete)
                                    .clicked()
                                {
                                    remove_item = Some((name.clone(), i));
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(icon::PENCIL).size(14.0).color(pal.muted),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(t.change)
                                    .clicked()
                                {
                                    edit = Some((name.clone(), Some(i)));
                                }
                            });
                        });
                        if i + 1 < len {
                            ui.separator();
                        }
                    }
                    ui.add_space(4.0);
                    if len < crate::radial::MAX_ITEMS {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{} {}", icon::PLUS, t.add_item))
                                        .color(ACCENT),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            edit = Some((name.clone(), None));
                        }
                    } else {
                        ui.label(RichText::new(t.menu_limit).small().color(pal.muted));
                    }
                });
            ui.add_space(6.0);
        }

        if let Some((old, new)) = rename {
            if !self.raw.menus.contains_key(&new) {
                if let Some(menu) = self.raw.menus.remove(&old) {
                    self.raw.menus.insert(new.clone(), menu);
                    self.previews.remove(&old);
                    if open_menu.as_deref() == Some(old.as_str()) {
                        open_menu = Some(new.clone());
                    }
                    rename_menu_uses(&mut self.raw, &old, &new);
                    changed = true;
                }
            }
        }
        if let Some(name) = delete {
            self.raw.menus.remove(&name);
            self.previews.remove(&name);
            if open_menu.as_deref() == Some(name.as_str()) {
                open_menu = None;
            }
            remove_menu_uses(&mut self.raw, &name);
            changed = true;
        }
        if let Some((menu, index)) = remove_item {
            if let Some(m) = self.raw.menus.get_mut(&menu) {
                if index < m.items.len() {
                    m.items.remove(index);
                    changed = true;
                }
            }
        }
        if let Some((menu, index)) = edit {
            if let Some(m) = self.raw.menus.get(&menu) {
                let (label, icon, form) = match index.and_then(|i| m.items.get(i)) {
                    Some(item) => (
                        item.label.clone(),
                        item.icon.clone(),
                        ActionForm::from_raw(Some(&item.action)),
                    ),
                    None => (String::new(), None, ActionForm::default()),
                };
                self.item_editor = Some(ItemEditor {
                    menu,
                    index,
                    label,
                    icon,
                    form,
                });
            }
        }
        self.open_menu = open_menu;
        if changed {
            self.changed();
        }
    }

    /// The dialog that edits one item of a radial menu.
    fn item_editor_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.item_editor.take() else {
            return;
        };
        let t = self.t;
        let mut result: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::NULL.with("logirev-item-editor")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(
                RichText::new(if ed.index.is_some() {
                    t.edit_item
                } else {
                    t.new_item
                })
                .size(20.0)
                .strong(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t.item_label);
                ui.add(egui::TextEdit::singleline(&mut ed.label).desired_width(300.0));
            });
            ui.horizontal(|ui| {
                ui.label(t.item_icon);
                let (glyph, name) = match ed.icon.as_deref().and_then(crate::radial::icon_glyph) {
                    Some(glyph) => (glyph, ed.icon.clone().unwrap_or_default()),
                    None => ("", t.no_icon.to_owned()),
                };
                ComboBox::from_id_salt("menu-item-icon")
                    .selected_text(format!("{glyph} {name}"))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(ed.icon.is_none(), t.no_icon).clicked() {
                            ed.icon = None;
                        }
                        for (name, glyph) in crate::radial::ICONS {
                            if ui
                                .selectable_label(
                                    ed.icon.as_deref() == Some(*name),
                                    format!("{glyph}  {name}"),
                                )
                                .clicked()
                            {
                                ed.icon = Some((*name).to_owned());
                            }
                        }
                    });
            });
            ui.add_space(8.0);
            let fc = FormContext {
                t,
                dpi_values: &[],
                gestures: false,
                macros: self.raw.macros.keys().cloned().collect(),
                pointing: false,
                menus: Vec::new(),
                menu_item: true,
                dark: ui.visuals().dark_mode,
            };
            ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 230.0).max(220.0))
                .show(ui, |ui| {
                    ed.form.ui(ui, &fc, "menu-item", false);
                });
            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                let valid = !ed.label.trim().is_empty() && ed.form.to_raw().is_some();
                let button =
                    egui::Button::new(RichText::new(t.save).color(Color32::WHITE)).fill(ACCENT);
                if ui.add_enabled(valid, button).clicked() {
                    result = Some(true);
                }
                if ui.button(t.cancel).clicked() {
                    result = Some(false);
                }
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }
        match result {
            Some(true) => {
                let action = ed.form.to_raw();
                if let (Some(action), Some(menu)) = (action, self.raw.menus.get_mut(&ed.menu)) {
                    let item = RawMenuItem {
                        label: ed.label.trim().to_owned(),
                        icon: ed.icon.clone().filter(|i| !i.is_empty()),
                        action,
                    };
                    match ed.index {
                        Some(i) if i < menu.items.len() => menu.items[i] = item,
                        _ => menu.items.push(item),
                    }
                    self.changed();
                }
            }
            Some(false) => {}
            None => self.item_editor = Some(ed),
        }
    }

    // ------------------------------------------------------------------
    // Settings and about
    // ------------------------------------------------------------------

    fn settings_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.settings_title).size(24.0).strong());
        ui.add_space(8.0);
        let before = self.raw.general.clone();
        let level_before = self.raw.logging.level.clone();

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::SUN, t.theme),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.language);
                    ComboBox::from_id_salt("lang")
                        .selected_text(match self.raw.general.language.as_str() {
                            "en" => "English",
                            "it" => "Italiano",
                            _ => t.lang_auto,
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.raw.general.language,
                                "auto".into(),
                                t.lang_auto,
                            );
                            ui.selectable_value(
                                &mut self.raw.general.language,
                                "en".into(),
                                "English",
                            );
                            ui.selectable_value(
                                &mut self.raw.general.language,
                                "it".into(),
                                "Italiano",
                            );
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(t.theme);
                    for (v, label) in [
                        ("system", t.theme_system),
                        ("light", t.theme_light),
                        ("dark", t.theme_dark),
                    ] {
                        ui.selectable_value(&mut self.raw.general.theme, v.to_owned(), label);
                    }
                });
            },
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::BATTERY_LOW, t.battery_and_messages),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.battery_warning);
                    let mut v = self.raw.general.battery_warning;
                    ui.add(
                        egui::Slider::new(&mut v, 0..=50)
                            .suffix("%")
                            .custom_formatter(|n, _| {
                                if n == 0.0 {
                                    t.off.to_owned()
                                } else {
                                    format!("{n:.0}")
                                }
                            }),
                    );
                    self.raw.general.battery_warning = v;
                });
                #[cfg(windows)]
                ui.checkbox(&mut self.raw.general.tray_battery, t.tray_battery);
                ui.checkbox(&mut self.raw.general.osd, t.osd_on);
                ui.label(RichText::new(t.osd_hint).small().color(pal.muted));
            },
        );

        #[cfg(windows)]
        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::MOUSE_SCROLL, t.wheel_all),
            |ui| {
                ui.label(RichText::new(t.wheel_all_hint).small().color(pal.muted));
                ui.horizontal(|ui| {
                    ui.label(t.scroll_speed);
                    ui.add(
                        egui::Slider::new(&mut self.raw.general.scroll_speed, 0.25..=5.0)
                            .step_by(0.05)
                            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                    );
                    if self.raw.general.scroll_speed != 1.0 && ui.button(t.normal).clicked() {
                        self.raw.general.scroll_speed = 1.0;
                    }
                });
                ui.checkbox(&mut self.raw.general.smooth_scrolling, t.smooth_scrolling);
            },
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::FLOPPY_DISK, t.backup),
            |ui| self.backup_ui(ui),
        );

        #[cfg(windows)]
        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::POWER, t.start_with_windows),
            |ui| {
                let mut on = crate::autostart::is_enabled();
                if ui.checkbox(&mut on, t.start_with_windows).changed() {
                    if on {
                        let svc = self.exe.with_file_name("logirev.exe");
                        let cfg = self.custom_config.then_some(self.config_path.as_path());
                        crate::autostart::enable(&crate::autostart::command_line(&svc, cfg));
                    } else {
                        crate::autostart::disable();
                    }
                }
            },
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::ARROWS_CLOCKWISE, t.updates),
            |ui| self.updates_ui(ui),
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::CODE, t.developer),
            |ui| {
                ui.checkbox(&mut self.raw.general.developer, t.developer_on);
                ui.label(RichText::new(t.developer_hint).small().color(pal.muted));
            },
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::FILE_TEXT, t.files),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.log_level);
                    ComboBox::from_id_salt("level")
                        .selected_text(self.raw.logging.level.clone())
                        .show_ui(ui, |ui| {
                            for l in ["error", "warn", "info", "debug", "trace"] {
                                ui.selectable_value(&mut self.raw.logging.level, l.to_owned(), l);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("{} {}", icon::FILE_TEXT, t.open_log))
                        .clicked()
                    {
                        if let Some(f) = &self.raw.logging.file {
                            let dir = self.config_path.parent().unwrap_or(Path::new("."));
                            open_text_file(&if f.is_absolute() {
                                f.clone()
                            } else {
                                dir.join(f)
                            });
                        }
                    }
                    if ui
                        .button(format!("{} {}", icon::SLIDERS, t.open_settings_file))
                        .clicked()
                    {
                        open_text_file(&self.config_path);
                    }
                    if ui
                        .button(format!("{} {}", icon::FOLDER_OPEN, t.open_folder))
                        .clicked()
                    {
                        if let Some(dir) = self.config_path.parent() {
                            open_folder(dir);
                        }
                    }
                });
            },
        );

        section(
            ui,
            pal.card,
            pal.border,
            &format!("{}  {}", icon::ARROWS_CLOCKWISE, t.service),
            |ui| {
                ui.horizontal(|ui| {
                    if self.connected() {
                        if ui
                            .button(format!("{} {}", icon::ARROWS_CLOCKWISE, t.restart_service))
                            .clicked()
                        {
                            self.send(ToService::Quit);
                            // The new process waits for the old one to release
                            // the lock (--after-update), so the UI never sleeps.
                            start_service(
                                &self.exe,
                                self.custom_config.then_some(self.config_path.as_path()),
                                true,
                            );
                            self.last_attempt = None;
                        }
                        if ui
                            .button(format!("{} {}", icon::POWER, t.stop_service))
                            .clicked()
                        {
                            self.send(ToService::Quit);
                        }
                    } else if ui
                        .button(format!("{} {}", icon::POWER, t.service_start))
                        .clicked()
                    {
                        start_service(
                            &self.exe,
                            self.custom_config.then_some(self.config_path.as_path()),
                            false,
                        );
                        self.last_attempt = None;
                    }
                });
            },
        );

        if self.raw.general != before || self.raw.logging.level != level_before {
            self.lang = Lang::from_setting(&self.raw.general.language);
            self.t = self.lang.texts();
            self.apply_theme(ui.ctx());
            self.changed();
        }
    }

    /// Export / import of the whole configuration.
    fn backup_ui(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.backup_hint).small().color(pal.muted));
        #[cfg(windows)]
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", icon::FLOPPY_DISK, t.export_settings))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("logirev-settings.toml")
                    .add_filter("logirev", &["toml"])
                    .save_file()
                {
                    self.backup_result = Some(match self.raw.save(&path) {
                        Ok(()) => Ok(t.exported.to_owned()),
                        Err(e) => Err(e),
                    });
                }
            }
            if ui
                .button(format!("{} {}", icon::FOLDER_OPEN, t.import_settings))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("logirev", &["toml"])
                    .pick_file()
                {
                    self.backup_result = Some(self.import_settings(&path));
                }
            }
        });
        ui.label(
            RichText::new(fill(
                t.settings_file_is,
                &[&self.config_path.display().to_string()],
            ))
            .small(),
        );
        match &self.backup_result {
            Some(Ok(msg)) => {
                ui.label(RichText::new(format!("{} {msg}", icon::CHECK)).color(GOOD));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("{} {e}", icon::WARNING)).color(BAD));
            }
            None => {}
        }
    }

    /// Replaces the settings with those of `path`, if they are valid.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn import_settings(&mut self, path: &Path) -> Result<String, String> {
        let raw = RawConfig::load(path)?;
        let base = path.parent().unwrap_or(Path::new("."));
        crate::config::Config::from_raw(raw.clone(), base)?;
        self.raw = raw;
        self.lang = Lang::from_setting(&self.raw.general.language);
        self.t = self.lang.texts();
        self.changed();
        Ok(self.t.imported.to_owned())
    }

    fn about_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(110.0, 110.0)));
            }
            ui.add_space(10.0);
            ui.label(RichText::new("logirev").size(28.0).strong());
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(pal.muted));
            ui.add_space(14.0);
            ui.label(RichText::new(t.about_text).size(16.0));
            ui.add_space(14.0);
            ui.hyperlink_to(
                format!("{}  {}", icon::BROADCAST, t.website),
                env!("CARGO_PKG_REPOSITORY"),
            );
            ui.add_space(20.0);
            ui.label(RichText::new(t.license_text).small().color(pal.muted));
            ui.label(RichText::new(t.not_affiliated).small().color(pal.muted));
        });
    }
}

impl eframe::App for SettingsApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.logo.is_none() {
            let rgba = crate::icon::draw_rgba(128);
            let image = egui::ColorImage::from_rgba_unmultiplied([128, 128], &rgba);
            self.logo = Some(ctx.load_texture("logo", image, egui::TextureOptions::LINEAR));
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            self.palette_open = !self.palette_open;
            self.palette_focus = self.palette_open;
        }
        let pal = palette(ui.visuals().dark_mode);
        egui::Panel::top("nav")
            .exact_size(56.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(pal.sidebar)
                    .inner_margin(Margin::symmetric(18, 8)),
            )
            .show(ui, |ui| self.top_bar(ui));
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(Margin::symmetric(28, 22)),
            )
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(1100.0));
                        self.content(ui);
                    });
            });
        self.command_palette(&ctx);
        self.advanced_warning_dialog(&ctx);
        self.editor_dialog(&ctx);
        self.item_editor_dialog(&ctx);
        self.pairing_dialog(&ctx);
        self.save_toast(&ctx);
        self.unpair_dialog(&ctx);
    }
}

// ----------------------------------------------------------------------
// Small widgets and helpers
// ----------------------------------------------------------------------

/// Draws the editable wheel with the same painter used by the Windows
/// overlay. Click a sector to edit it; drag a sector onto another to move it.
fn menu_editor(
    ui: &mut Ui,
    name: &str,
    menu: &mut RawMenu,
    previews: &mut BTreeMap<String, (String, egui::TextureHandle)>,
    wheel_drag: &mut Option<(String, usize)>,
    edit: &mut Option<(String, Option<usize>)>,
    changed: &mut bool,
) {
    let metrics = crate::radial::Metrics::scaled(0.8);
    let (w, h) = metrics.frame_size();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(w as f32, h as f32),
        egui::Sense::click_and_drag(),
    );
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| ui.ctx().pointer_latest_pos());
    let hover_pos = if response.dragged() {
        pointer
    } else {
        response.hover_pos()
    };
    let hover = hover_pos.and_then(|p| wheel_sector(p, rect, &metrics, menu.items.len()));
    let key = format!("{name}|{menu:?}|{hover:?}");
    let texture = match previews.get(name) {
        Some((cached, texture)) if *cached == key => texture.clone(),
        _ => {
            let frame = crate::radial::paint_preview(menu.items.len(), hover, &metrics);
            let image =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &frame.rgba);
            let texture = ui.ctx().load_texture(
                format!("radial-preview-{name}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            previews.insert(name.to_owned(), (key, texture.clone()));
            texture
        }
    };
    let painter = ui.painter();
    painter.image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let white = Color32::from_rgb(235, 239, 247);
    for (item, place) in menu
        .items
        .iter()
        .zip(crate::radial::item_placements(menu.items.len(), &metrics))
    {
        if let Some(glyph) = item.icon.as_deref().and_then(crate::radial::icon_glyph) {
            if let Some((x, y)) = place.icon {
                painter.text(
                    rect.min + egui::vec2(x, y),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    egui::FontId::proportional(metrics.icon_size),
                    white,
                );
            }
        }
        let (x, y) = place.label.center();
        let max_chars =
            ((place.label.width() / (metrics.label_size * 0.55)).floor() as usize).max(3);
        painter.text(
            rect.min + egui::vec2(x, y),
            egui::Align2::CENTER_CENTER,
            shorten_text(&item.label, max_chars),
            egui::FontId::proportional(metrics.label_size),
            white,
        );
    }
    if let Some(place) = crate::radial::title_placement(menu.title.as_deref(), &metrics) {
        if let Some(title) = menu
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
        {
            let (x, y) = place.label.center();
            painter.text(
                rect.min + egui::vec2(x, y),
                egui::Align2::CENTER_CENTER,
                shorten_text(title, 20),
                egui::FontId::proportional(metrics.title_size),
                Color32::from_rgb(148, 163, 184),
            );
        }
    }

    // A drag can outlive the item it refers to if the list changes.
    let stale = wheel_drag
        .as_ref()
        .is_some_and(|(drag_name, from)| drag_name == name && *from >= menu.items.len());
    if stale {
        *wheel_drag = None;
    }
    let target = pointer.and_then(|p| wheel_sector(p, rect, &metrics, menu.items.len()));
    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(pointer)
            .and_then(|p| wheel_sector(p, rect, &metrics, menu.items.len()));
        if let Some(from) = origin {
            *wheel_drag = Some((name.to_owned(), from));
        }
    }
    if response.clicked() {
        if let Some(index) = target {
            *edit = Some((name.to_owned(), Some(index)));
        }
    }
    if response.drag_stopped() {
        if let Some((from_name, from)) = wheel_drag.take() {
            if from_name == name {
                if let Some(to) = target {
                    if from != to {
                        move_menu_item(&mut menu.items, from, to);
                        *changed = true;
                    }
                }
            }
        }
    }
    if let Some((drag_name, from)) = wheel_drag.as_ref() {
        if drag_name == name {
            if let Some(item) = menu.items.get(*from) {
                let pos = pointer.unwrap_or_else(|| rect.center());
                let painter = ui.painter();
                painter.circle_filled(pos, 17.0, Color32::from_black_alpha(220));
                painter.circle_stroke(pos, 17.0, egui::Stroke::new(1.5, ACCENT));
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    shorten_text(&item.label, 12),
                    egui::FontId::proportional(10.0),
                    white,
                );
            }
        }
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(if response.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
}

/// The sector under `pointer`, if it is inside the ring.
fn wheel_sector(
    pointer: egui::Pos2,
    rect: egui::Rect,
    metrics: &crate::radial::Metrics,
    count: usize,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let dx = pointer.x - rect.center().x;
    let dy = pointer.y - rect.center().y;
    match crate::radial::Layout::new(count).hit(metrics, dx, dy) {
        crate::radial::Hit::Item(index) => Some(index),
        _ => None,
    }
}

/// Moves the item at `from` to the `to` sector.
fn move_menu_item(items: &mut Vec<RawMenuItem>, from: usize, to: usize) {
    if from == to || from >= items.len() || to >= items.len() {
        return;
    }
    let item = items.remove(from);
    items.insert(to, item);
}

/// Shortens `text` to `max` characters, adding an ellipsis.
fn shorten_text(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// "now", "42s", "3m", "2h" or "4d".
fn short_age(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

/// A small painted service-status dot (not a text glyph).
fn status_dot(ui: &mut Ui, ok: bool, tooltip: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 4.0, if ok { GOOD } else { BAD });
    response.on_hover_text(tooltip);
}

/// A titled card.
fn section(
    ui: &mut Ui,
    fill_color: Color32,
    border: Color32,
    title: &str,
    add: impl FnOnce(&mut Ui),
) {
    Frame::new()
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(16.0).strong());
            ui.add_space(4.0);
            add(ui);
        });
    ui.add_space(10.0);
}

/// A one-line card.
fn card_row(
    ui: &mut Ui,
    fill_color: Color32,
    border: Color32,
    add: impl FnOnce(&mut Ui),
) -> egui::Response {
    let resp = Frame::new()
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add);
        })
        .response;
    ui.add_space(4.0);
    resp
}

fn banner(ui: &mut Ui, color: Color32, text: &str) {
    Frame::new()
        .fill(color.gamma_multiply(0.15))
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("{} {text}", icon::WARNING)).color(color));
        });
    ui.add_space(8.0);
}

fn empty_state(ui: &mut Ui, ic: &str, title: &str, hint: &str) {
    let pal = palette(ui.visuals().dark_mode);
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(ic).size(64.0).color(pal.muted));
        ui.add_space(8.0);
        ui.label(RichText::new(title).size(20.0).strong());
        ui.add(egui::Label::new(RichText::new(hint).color(pal.muted)).wrap());
    });
    ui.add_space(16.0);
}

/// A setting that can be on, off or "leave it as it is", as three clearly
/// labelled buttons side by side.
fn tri_state(ui: &mut Ui, value: &mut Option<bool>, label: &str, t: &T) {
    ui.label(RichText::new(label).strong());
    segmented(
        ui,
        value,
        &[
            (None, t.not_set),
            (Some(true), t.turn_on),
            (Some(false), t.turn_off),
        ],
    );
    ui.add_space(4.0);
}

/// Buttons joined in a row, one of which is selected (like a radio group).
/// Returns true when the choice changed.
fn segmented<V: PartialEq + Clone>(ui: &mut Ui, value: &mut V, options: &[(V, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let last = options.len().saturating_sub(1);
        for (i, (v, text)) in options.iter().enumerate() {
            let on = *value == *v;
            let r = |first: bool, end: bool| if first || end { 8 } else { 0 };
            let corner = CornerRadius {
                nw: r(i == 0, false),
                sw: r(i == 0, false),
                ne: r(false, i == last),
                se: r(false, i == last),
            };
            let text = if on {
                RichText::new(format!("{} {text}", icon::CHECK)).color(Color32::WHITE)
            } else {
                RichText::new(*text)
            };
            let mut button = egui::Button::new(text)
                .corner_radius(corner)
                .min_size(egui::vec2(0.0, 28.0));
            if on {
                button = button.fill(ACCENT);
            }
            if ui.add(button).clicked() && !on {
                *value = v.clone();
                changed = true;
            }
        }
    });
    changed
}

fn pointer_differs(a: &RawProfile, b: &RawProfile) -> bool {
    a.dpi != b.dpi
        || a.pointer_speed != b.pointer_speed
        || a.report_rate != b.report_rate
        || a.hires_wheel != b.hires_wheel
        || a.invert_wheel != b.invert_wheel
        || a.thumb_wheel_invert != b.thumb_wheel_invert
        || a.smartshift.as_ref().map(|s| (s.mode, s.threshold))
            != b.smartshift.as_ref().map(|s| (s.mode, s.threshold))
}

/// Edits the steps of a macro; returns true if something changed.
fn steps_editor(ui: &mut Ui, steps: &mut Vec<RawStep>, t: &T, id: &str) -> bool {
    #[derive(PartialEq, Clone, Copy)]
    enum S {
        Keys,
        Press,
        Release,
        Text,
        Click,
        Delay,
    }
    let kind_of = |s: &RawStep| {
        if s.keys.is_some() {
            S::Keys
        } else if s.press.is_some() {
            S::Press
        } else if s.release.is_some() {
            S::Release
        } else if s.text.is_some() {
            S::Text
        } else if s.click.is_some() {
            S::Click
        } else {
            S::Delay
        }
    };
    let label = |k: S| match k {
        S::Keys => t.step_keys,
        S::Press => t.step_press,
        S::Release => t.step_release,
        S::Text => t.step_text,
        S::Click => t.step_click,
        S::Delay => t.step_delay,
    };
    let make = |k: S| match k {
        S::Keys => RawStep {
            keys: Some("Ctrl+C".into()),
            ..Default::default()
        },
        S::Press => RawStep {
            press: Some("Shift".into()),
            ..Default::default()
        },
        S::Release => RawStep {
            release: Some("Shift".into()),
            ..Default::default()
        },
        S::Text => RawStep {
            text: Some(String::new()),
            ..Default::default()
        },
        S::Click => RawStep {
            click: Some(MouseButton::Left),
            ..Default::default()
        },
        S::Delay => RawStep {
            delay: Some(100),
            ..Default::default()
        },
    };
    let all = [S::Keys, S::Press, S::Release, S::Text, S::Click, S::Delay];
    let mut changed = false;
    let (mut up, mut down, mut remove) = (None, None, None);
    for (i, step) in steps.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("{}.", i + 1));
            let k = kind_of(step);
            let mut chosen = k;
            ComboBox::from_id_salt(format!("{id}-step{i}"))
                .width(170.0)
                .selected_text(label(k))
                .show_ui(ui, |ui| {
                    for s in all {
                        ui.selectable_value(&mut chosen, s, label(s));
                    }
                });
            if chosen != k {
                *step = make(chosen);
                changed = true;
            }
            let text_field = |ui: &mut Ui, v: &mut Option<String>| {
                let mut s = v.clone().unwrap_or_default();
                let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(220.0));
                if r.changed() {
                    *v = Some(s);
                    true
                } else {
                    false
                }
            };
            changed |= match chosen {
                S::Keys => text_field(ui, &mut step.keys),
                S::Press => text_field(ui, &mut step.press),
                S::Release => text_field(ui, &mut step.release),
                S::Text => text_field(ui, &mut step.text),
                S::Click => {
                    let mut b = step.click.unwrap_or(MouseButton::Left);
                    let before = b;
                    ComboBox::from_id_salt(format!("{id}-click{i}"))
                        .selected_text(format!("{b:?}"))
                        .show_ui(ui, |ui| {
                            for m in [
                                MouseButton::Left,
                                MouseButton::Right,
                                MouseButton::Middle,
                                MouseButton::Back,
                                MouseButton::Forward,
                            ] {
                                ui.selectable_value(&mut b, m, format!("{m:?}"));
                            }
                        });
                    step.click = Some(b);
                    b != before
                }
                S::Delay => {
                    let mut ms = step.delay.unwrap_or(100);
                    let r = ui.add(egui::DragValue::new(&mut ms).range(0..=2000).suffix(" ms"));
                    step.delay = Some(ms);
                    r.changed()
                }
            };
            if ui.small_button(icon::CARET_UP).clicked() {
                up = Some(i);
            }
            if ui.small_button(icon::CARET_DOWN).clicked() {
                down = Some(i);
            }
            if ui.small_button(icon::X).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = up.filter(|i| *i > 0) {
        steps.swap(i, i - 1);
        changed = true;
    }
    if let Some(i) = down.filter(|i| i + 1 < steps.len()) {
        steps.swap(i, i + 1);
        changed = true;
    }
    if let Some(i) = remove.filter(|_| steps.len() > 1) {
        steps.remove(i);
        changed = true;
    }
    if ui
        .button(format!("{} {}", icon::PLUS, t.add_step))
        .clicked()
    {
        steps.push(make(S::Keys));
        changed = true;
    }
    changed
}

/// The key and action of `buttons` that writes control `cid`, if any.
///
/// The configuration may name a control as an alias (`back`) or as a number
/// (`"0x00C3"`); the list shows the control id, so the lookup must too.
fn action_for_control(
    buttons: &BTreeMap<String, RawAction>,
    cid: u16,
) -> Option<(String, RawAction)> {
    buttons
        .iter()
        .find(|(key, _)| parse_control(key) == Some(cid))
        .map(|(key, action)| (key.clone(), action.clone()))
}

/// Every button action anywhere in the configuration (including menu items).
fn all_actions(raw: &mut RawConfig) -> impl Iterator<Item = &mut RawAction> {
    let buttons = raw.profiles.iter_mut().flat_map(|p| {
        let apps = p.apps.iter_mut().flat_map(|a| a.buttons.values_mut());
        p.buttons.values_mut().chain(apps)
    });
    let menu_items = raw
        .menus
        .values_mut()
        .flat_map(|m| m.items.iter_mut().map(|i| &mut i.action));
    buttons.chain(menu_items)
}

fn rename_macro_uses(raw: &mut RawConfig, old: &str, new: &str) {
    for a in all_actions(raw) {
        if let RawAction::Macro { name, .. } = a {
            if name == old {
                *name = new.to_owned();
            }
        }
    }
}

fn remove_macro_uses(raw: &mut RawConfig, gone: &str) {
    for a in all_actions(raw) {
        if matches!(a, RawAction::Macro { name, .. } if name == gone) {
            *a = RawAction::Disable;
        }
    }
}

fn rename_menu_uses(raw: &mut RawConfig, old: &str, new: &str) {
    for a in all_actions(raw) {
        if let RawAction::Radial { menu } = a {
            if menu == old {
                *menu = new.to_owned();
            }
        }
    }
}

fn remove_menu_uses(raw: &mut RawConfig, gone: &str) {
    for a in all_actions(raw) {
        if matches!(a, RawAction::Radial { menu } if menu == gone) {
            *a = RawAction::Disable;
        }
    }
}

/// Sample devices for demo mode.
fn demo_snapshot() -> Snapshot {
    use crate::status::{ControlInfo, DeviceDetails, HostsStatus};
    let control = |cid: u16, key: &str, label: &str, remappable: bool| ControlInfo {
        cid,
        key: key.into(),
        label: label.into(),
        remappable,
        gestures: false,
    };
    let ergo = DeviceDetails {
        firmware: "RBM 26.01.B0013".into(),
        controls: vec![
            control(0x50, "left", "Left click", false),
            control(0x51, "right", "Right click", false),
            control(0x52, "middle", "Middle button (wheel click)", true),
            control(0x53, "back", "Back", true),
            control(0x56, "forward", "Forward", true),
            control(0x5B, "tilt_left", "Wheel tilt left", true),
            control(0x5D, "tilt_right", "Wheel tilt right", true),
            control(
                0xFD,
                "precision",
                "Precision mode / DPI switch button",
                true,
            ),
        ],
        dpi_values: (400..=4000).step_by(50).collect(),
        tech: Some(demo_tech()),
        ..Default::default()
    };
    let device = |slot: u8,
                  name: &str,
                  kind: &str,
                  online: bool,
                  battery: Option<&str>,
                  details: Option<DeviceDetails>| DeviceStatus {
        slot,
        name: name.into(),
        kind: kind.into(),
        online,
        battery: battery.map(Into::into),
        battery_level: battery
            .and_then(|b| b.trim_end_matches('%').parse().ok().map(|p| (p, false))),
        battery_age: battery.map(|_| 12),
        dpi: online.then_some(750),
        profile: Some("MX Ergo".into()),
        remapped: if online { 5 } else { 0 },
        hosts: online.then(|| HostsStatus {
            count: 2,
            current: 0,
            names: vec![Some("DESKTOP-HOME".into()), Some("WORK-LAPTOP".into())],
        }),
        details,
    };
    Snapshot {
        links: vec![LinkStatus {
            key: "demo".into(),
            name: "Bolt receiver (046d:c548)".into(),
            is_receiver: true,
            can_pair: true,
            pairing: None,
            traffic: demo_traffic(),
            devices: vec![
                device(1, "Wave Keys", "keyboard", false, None, None),
                device(2, "MX Ergo S", "trackball", true, Some("30%"), Some(ergo)),
                device(
                    3,
                    "MX Master 3S",
                    "mouse",
                    true,
                    Some("85% (charging)"),
                    Some(DeviceDetails::default()),
                ),
            ],
        }],
    }
}

fn kind_of(kind: &str) -> DeviceKind {
    serde_json::from_value(serde_json::Value::String(kind.to_owned())).unwrap_or(DeviceKind::Other)
}

fn key_display(name: &str) -> &str {
    match name {
        "capslock" => "Caps Lock",
        "numlock" => "Num Lock",
        "scrolllock" => "Scroll Lock",
        "insert" => "Insert",
        "windows" => "Windows",
        other => other,
    }
}

/// Starts the background service (detached). With `after_update` the new
/// copy waits for the previous one to release the single-instance lock.
fn start_service(exe: &Path, config: Option<&Path>, after_update: bool) {
    let mut cmd = std::process::Command::new(exe);
    if after_update {
        cmd.arg("--after-update");
    }
    if let Some(c) = config {
        cmd.arg("--config").arg(c);
    }
    if let Err(e) = cmd.spawn() {
        log::warn!("cannot start the service: {e}");
    }
}

/// Opens a text file in the system's text editor.
fn open_text_file(path: &Path) {
    #[cfg(windows)]
    let r = std::process::Command::new("notepad.exe").arg(path).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = r {
        log::warn!("cannot open {}: {e}", path.display());
    }
}

fn open_folder(path: &Path) {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer.exe").arg(path).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = r {
        log::warn!("cannot open {}: {e}", path.display());
    }
}

/// File names of the programs running now (Windows), for per-program
/// buttons.
fn running_programs() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let mut out = Vec::new();
        // SAFETY: standard process snapshot walk; the handle is closed below.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut entry) != 0;
            while ok {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                out.push(String::from_utf16_lossy(&entry.szExeFile[..end]));
                ok = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        const SYSTEM: &[&str] = &[
            "svchost.exe",
            "csrss.exe",
            "conhost.exe",
            "dllhost.exe",
            "system",
            "[system process]",
            "registry",
            "smss.exe",
            "wininit.exe",
            "services.exe",
            "lsass.exe",
            "fontdrvhost.exe",
            "dwm.exe",
            "winlogon.exe",
            "sihost.exe",
            "runtimebroker.exe",
            "searchhost.exe",
            "ctfmon.exe",
            "taskhostw.exe",
            "logirev.exe",
        ];
        out.retain(|p| !SYSTEM.contains(&p.to_lowercase().as_str()));
        out.sort_by_key(|p| p.to_lowercase());
        out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        out
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Technical details shown by the demo (developer mode).
fn demo_tech() -> crate::hidpp::diagnostics::TechInfo {
    use crate::hidpp::diagnostics::{FirmwareEntity, TechInfo};
    use crate::hidpp::features::FeatureEntry;
    let f = |id: u16, index: u8, version: u8, flags: u8| FeatureEntry {
        id,
        index,
        version,
        flags,
    };
    TechInfo {
        unit_id: "1A2B3C4D".into(),
        serial: Some("2412LZ0ABC12".into()),
        model_ids: vec!["BLE B03F".into(), "eQuad B03E".into()],
        transports: vec!["BLE".into(), "eQuad".into()],
        firmware: vec![
            FirmwareEntity {
                kind: "application".into(),
                version: "RBM 26.01.B0013".into(),
                pid: Some(0xB03E),
                active: true,
            },
            FirmwareEntity {
                kind: "bootloader".into(),
                version: "BL1 46.00.B0008".into(),
                pid: Some(0xB03E),
                active: false,
            },
            FirmwareEntity {
                kind: "hardware".into(),
                version: String::new(),
                pid: None,
                active: false,
            },
        ],
        features: vec![
            f(0x0000, 0, 2, 0),
            f(0x0001, 1, 1, 0),
            f(0x0003, 2, 4, 0),
            f(0x0005, 3, 0, 0),
            f(0x1004, 4, 3, 0),
            f(0x1B04, 5, 5, 0),
            f(0x2201, 6, 2, 0),
            f(0x1814, 7, 1, 0),
            f(0x1815, 8, 1, 0),
            f(0x1D4B, 9, 0, 0),
            f(0x00C3, 10, 0, 0),
            f(0x1802, 11, 0, 0x60),
            f(0x1E00, 12, 0, 0x40),
            f(0x1EB0, 13, 0, 0x60),
        ],
    }
}

/// HID++ messages shown by the demo (developer mode).
fn demo_traffic() -> Vec<crate::hidpp::traffic::TrafficEntry> {
    let frames: [(bool, &[u8]); 6] = [
        (true, &[0x10, 0x02, 0x00, 0x1A, 0x22, 0x01, 0x00]),
        (false, &[0x11, 0x02, 0x00, 0x1A, 0x06, 0x00, 0x02]),
        (true, &[0x10, 0x02, 0x06, 0x2A, 0x00, 0x00, 0x00]),
        (false, &[0x11, 0x02, 0x06, 0x2A, 0x00, 0x02, 0xEE]),
        (false, &[0x11, 0x02, 0x05, 0x00, 0x00, 0x53]),
        (false, &[0x11, 0x02, 0x05, 0x00, 0x00, 0x00]),
    ];
    frames
        .iter()
        .enumerate()
        .map(|(i, (outgoing, b))| crate::hidpp::traffic::TrafficEntry {
            seq: i as u64 + 1,
            time_ms: 1_761_300_000_000 + i as u64 * 37,
            outgoing: *outgoing,
            bytes: b.to_vec(),
        })
        .collect()
}

/// Bytes as `10 02 00 1A ...`, trailing zero padding removed.
fn hex_bytes(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .rposition(|&b| b != 0)
        .map_or(4, |i| (i + 1).max(4));
    bytes[..end.min(bytes.len())]
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A short explanation of a HID++ message: feature name and function.
fn describe_frame(bytes: &[u8], features: &[crate::hidpp::features::FeatureEntry]) -> String {
    let (Some(&slot), Some(&b2), Some(&b3)) = (bytes.get(1), bytes.get(2), bytes.get(3)) else {
        return String::new();
    };
    match b2 {
        0xFF => return "error".into(),
        0x8F => return "HID++ 1.0 error".into(),
        0x80..=0x83 => return format!("register {b3:#04x}"),
        0x41 => return "device connection".into(),
        _ if slot == 0xFF || b2 >= 0x40 => return format!("receiver {b2:#04x}"),
        _ => {}
    }
    match features.iter().find(|f| f.index == b2) {
        Some(f) => {
            let kind = if b3 & 0x0F == 0 { "event" } else { "call" };
            format!("{} fn {} {kind}", feature_name(f.id), b3 >> 4)
        }
        None => String::new(),
    }
}

fn or_dash(s: &str) -> String {
    if s.is_empty() {
        "—".into()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn renames_and_removes_macro_uses() {
        let mut raw = RawConfig::parse(
            "[macros.a]\nsteps = [ { keys = \"A\" } ]\n[[profile]]\nname = \"p\"\nbuttons.back = { action = \"macro\", name = \"a\" }\n[menus.m]\nitems = [ { label = \"x\", action = { action = \"macro\", name = \"a\" } } ]",
        )
        .unwrap();
        rename_macro_uses(&mut raw, "a", "b");
        assert!(
            matches!(&raw.profiles[0].buttons["back"], RawAction::Macro { name, .. } if name == "b")
        );
        assert!(
            matches!(&raw.menus["m"].items[0].action, RawAction::Macro { name, .. } if name == "b")
        );
        remove_macro_uses(&mut raw, "b");
        assert!(matches!(
            raw.profiles[0].buttons["back"],
            RawAction::Disable
        ));
        assert!(matches!(raw.menus["m"].items[0].action, RawAction::Disable));
    }

    #[test]
    fn moves_menu_items_on_the_wheel() {
        let item = |label: &str| RawMenuItem {
            label: label.into(),
            icon: None,
            action: RawAction::Disable,
        };
        let mut items = vec![item("a"), item("b"), item("c")];
        move_menu_item(&mut items, 0, 2);
        assert_eq!(
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["b", "c", "a"]
        );
        move_menu_item(&mut items, 2, 0);
        assert_eq!(
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        move_menu_item(&mut items, 1, 1);
        move_menu_item(&mut items, 3, 0);
        assert_eq!(
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn renames_and_removes_menu_uses() {
        let mut raw = RawConfig::parse(
            "[menus.old]\nitems = [ { label = \"x\", action = { action = \"desktop\", direction = \"next\" } } ]\n[[profile]]\nname = \"p\"\nbuttons.back = { action = \"radial\", menu = \"old\" }",
        )
        .unwrap();
        rename_menu_uses(&mut raw, "old", "new");
        assert!(
            matches!(&raw.profiles[0].buttons["back"], RawAction::Radial { menu } if menu == "new")
        );
        remove_menu_uses(&mut raw, "new");
        assert!(matches!(
            raw.profiles[0].buttons["back"],
            RawAction::Disable
        ));
    }

    #[test]
    fn finds_actions_written_with_a_number() {
        let raw = RawConfig::parse(
            "[[profile]]\nname = \"p\"\n[profile.buttons]\n\"0x00C3\" = { action = \"disable\" }",
        )
        .unwrap();
        let (key, action) = action_for_control(&raw.profiles[0].buttons, 0xC3).expect("found");
        assert_eq!(key, "0x00C3");
        assert!(matches!(action, RawAction::Disable));
        assert!(action_for_control(&raw.profiles[0].buttons, 0x53).is_none());
    }

    #[test]
    fn parses_device_kinds() {
        assert_eq!(kind_of("trackball"), DeviceKind::Trackball);
        assert_eq!(kind_of("weird"), DeviceKind::Other);
        let _ = Config::parse("", Path::new(".")).unwrap();
    }

    #[test]
    fn describes_frames() {
        use crate::hidpp::features::FeatureEntry;
        let f = [FeatureEntry {
            id: 0x2201,
            index: 6,
            version: 2,
            flags: 0,
        }];
        assert_eq!(hex_bytes(&[0x10, 0x02, 0x06, 0x2A, 0, 0, 0]), "10 02 06 2A");
        assert_eq!(
            describe_frame(&[0x10, 0x02, 0x06, 0x2A], &f),
            "AdjustableDpi fn 2 call"
        );
        assert_eq!(
            describe_frame(&[0x11, 0x02, 0x06, 0x00], &f),
            "AdjustableDpi fn 0 event"
        );
        assert_eq!(
            describe_frame(&[0x10, 0x02, 0x41, 0x04], &f),
            "device connection"
        );
        assert_eq!(
            describe_frame(&[0x10, 0xFF, 0x81, 0x02], &f),
            "register 0x02"
        );
    }
}
