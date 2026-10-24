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

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ComboBox, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea,
    Sense, Stroke, TextureHandle, ThemePreference, Ui,
};

use super::form::{dpi_slider, ActionForm, FormContext};
use super::i18n::{control_label, fill, Lang, T};
use super::style::{self, icon, palette, ACCENT, BAD, GOOD};
use crate::config::{
    BacklightMode, Match, RawAction, RawApp, RawBacklight, RawConfig, RawMacro, RawProfile,
    RawSmartShift, RawStep, SmartShiftMode,
};
use crate::hidpp::features::{DeviceKind, DISABLE_KEY_NAMES};
use crate::hidpp::frame::RECEIVER_INDEX;
use crate::input::keys::MouseButton;
use crate::ipc::{Client, ToService};
use crate::status::{DeviceStatus, LinkStatus, Snapshot};

/// Wait after the last edit before saving.
const SAVE_DELAY: Duration = Duration::from_millis(600);
/// How often to retry connecting to the service.
const RECONNECT_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Page {
    Devices,
    Device { link: String, slot: u8 },
    Macros,
    Settings,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Buttons,
    Pointer,
    Keyboard,
    Programs,
    Computers,
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

pub struct SettingsApp {
    lang: Lang,
    t: &'static T,
    client: Option<Client>,
    last_attempt: Option<Instant>,
    snapshot: Snapshot,
    config_path: PathBuf,
    raw: RawConfig,
    load_error: Option<String>,
    dirty_at: Option<Instant>,
    save: SaveState,
    page: Page,
    tab: Tab,
    editor: Option<Editor>,
    new_program: String,
    running_programs: Vec<String>,
    logo: Option<TextureHandle>,
    exe: PathBuf,
    custom_config: bool,
    /// Demo mode (`LOGIREV_DEMO=1`): sample devices, nothing is saved.
    /// Used for screenshots and to try the interface without hardware.
    demo: bool,
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
            config_path,
            raw,
            load_error,
            dirty_at: None,
            save: SaveState::Idle,
            page: Page::Devices,
            tab: Tab::Buttons,
            editor: None,
            new_program: String::new(),
            running_programs: Vec::new(),
            logo: None,
            exe: std::env::current_exe().unwrap_or_default(),
            custom_config,
            demo: std::env::var_os("LOGIREV_DEMO").is_some(),
        };
        let mut app = app;
        if app.demo {
            app.snapshot = demo_snapshot();
            app.page = match std::env::var("LOGIREV_PAGE").as_deref() {
                Ok("device") | Ok("pointer") | Ok("computers") | Ok("editor") => Page::Device {
                    link: "demo".into(),
                    slot: 2,
                },
                Ok("macros") => Page::Macros,
                Ok("settings") => Page::Settings,
                Ok("about") => Page::About,
                _ => Page::Devices,
            };
            app.tab = match std::env::var("LOGIREV_PAGE").as_deref() {
                Ok("pointer") => Tab::Pointer,
                Ok("computers") => Tab::Computers,
                _ => Tab::Buttons,
            };
            if std::env::var("LOGIREV_PAGE").as_deref() == Ok("editor") {
                app.editor = Some(Editor {
                    device: "MX Ergo S".into(),
                    kind: DeviceKind::Trackball,
                    app: None,
                    button_key: "back".into(),
                    button_label: "Back".into(),
                    gestures: false,
                    dpi_values: (400..=4000).step_by(50).collect(),
                    form: ActionForm::from_raw(Some(&RawAction::Keys {
                        keys: "Alt+Left".into(),
                    })),
                });
            }
        }
        app.apply_theme(&cc.egui_ctx);
        app
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
        if self.demo {
            self.dirty_at = None;
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
        if let Some(snap) = self.client.as_ref().and_then(Client::snapshot) {
            self.snapshot = snap;
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

    fn sidebar(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(36.0, 36.0)));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("logirev").size(20.0).strong());
                ui.label(
                    RichText::new(env!("CARGO_PKG_VERSION"))
                        .small()
                        .color(pal.muted),
                );
            });
        });
        ui.add_space(18.0);

        let device_page = matches!(self.page, Page::Device { .. });
        for (page, ic, label) in [
            (Page::Devices, icon::MOUSE, t.nav_devices),
            (Page::Macros, icon::MAGIC_WAND, t.nav_macros),
            (Page::Settings, icon::GEAR, t.nav_settings),
            (Page::About, icon::INFO, t.nav_about),
        ] {
            let selected = self.page == page || (device_page && page == Page::Devices);
            let text = RichText::new(format!("{ic}   {label}")).size(16.0);
            let button = egui::Button::selectable(selected, text)
                .min_size(egui::vec2(ui.available_width(), 40.0));
            if ui.add(button).clicked() {
                self.page = page;
            }
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(4.0);
            if self.connected() {
                ui.label(
                    RichText::new(format!("{} {}", icon::CHECK_CIRCLE, t.service_running))
                        .color(GOOD)
                        .small(),
                );
            } else {
                if ui
                    .button(format!("{} {}", icon::POWER, t.service_start))
                    .clicked()
                {
                    start_service(
                        &self.exe,
                        self.custom_config.then_some(self.config_path.as_path()),
                    );
                    self.last_attempt = None;
                }
                ui.label(
                    RichText::new(format!("{} {}", icon::X_CIRCLE, t.service_stopped))
                        .color(BAD)
                        .small(),
                );
            }
            match &self.save {
                SaveState::Saved(at) if at.elapsed() < Duration::from_secs(3) => {
                    ui.label(
                        RichText::new(format!("{} {}", icon::CHECK, t.saved))
                            .color(GOOD)
                            .small(),
                    );
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                }
                _ => {}
            }
        });
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
            Page::Macros => self.macros_page(ui),
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
        ui.label(RichText::new(t.devices_title).size(28.0).strong());
        ui.label(RichText::new(t.devices_subtitle).color(pal.muted));
        ui.add_space(12.0);

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
            ui.label(
                RichText::new(format!("{link_icon}  {}", link.name))
                    .strong()
                    .color(pal.muted),
            );
            if link.devices.is_empty() {
                ui.label(t.no_devices);
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
                for d in &link.devices {
                    if self.device_card(ui, link, d) {
                        self.page = Page::Device {
                            link: link.key.clone(),
                            slot: d.slot,
                        };
                        self.tab = Tab::Buttons;
                    }
                }
            });
            ui.add_space(16.0);
        }
    }

    /// One device card; returns true when clicked.
    fn device_card(&self, ui: &mut Ui, _link: &LinkStatus, d: &DeviceStatus) -> bool {
        let t = self.t;
        let dark = ui.visuals().dark_mode;
        let pal = palette(dark);
        let id = ui.scope_id().with(("card", &d.name, d.slot));
        let hovered = d.online && ui.ctx().data(|m| m.get_temp::<bool>(id)).unwrap_or(false);
        let frame = Frame::new()
            .fill(if hovered { pal.card_hover } else { pal.card })
            .stroke(Stroke::new(1.0, if hovered { ACCENT } else { pal.border }))
            .corner_radius(CornerRadius::same(16))
            .inner_margin(Margin::same(18));
        let inner = frame.show(ui, |ui| {
            ui.set_width(250.0);
            ui.set_min_height(118.0);
            // The card sits in a wrapping row: lay its content out vertically.
            ui.vertical(|ui| {
                let faded = |c: Color32| if d.online { c } else { c.gamma_multiply(0.45) };
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(style::kind_icon(&d.kind))
                            .size(38.0)
                            .color(faded(ACCENT)),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&d.name)
                                .size(17.0)
                                .strong()
                                .color(faded(ui.visuals().text_color())),
                        );
                        let place = if d.slot == RECEIVER_INDEX {
                            t.direct.to_owned()
                        } else {
                            fill(t.slot, &[&d.slot.to_string()])
                        };
                        ui.label(RichText::new(place).small().color(pal.muted));
                    });
                });
                ui.add_space(6.0);
                if d.online {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} {}", icon::CHECK_CIRCLE, t.connected))
                                .color(GOOD),
                        );
                        if let Some(b) = &d.battery {
                            let (ic, color) = style::battery_icon(b);
                            ui.label(
                                RichText::new(format!("{ic} {}", b.split(' ').next().unwrap_or(b)))
                                    .color(color)
                                    .size(16.0),
                            );
                        }
                    });
                    if d.remapped > 0 {
                        ui.label(
                            RichText::new(fill(t.buttons_remapped, &[&d.remapped.to_string()]))
                                .small()
                                .color(pal.muted),
                        );
                    }
                } else {
                    ui.label(RichText::new(t.asleep).color(pal.muted));
                    ui.label(RichText::new(t.asleep_hint).small().color(pal.muted));
                }
            });
        });
        let resp = ui.interact(
            inner.response.rect,
            id,
            if d.online {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        ui.ctx().data_mut(|m| m.insert_temp(id, resp.hovered()));
        if resp.hovered() && d.online {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked()
    }

    fn find_device(&self, link: &str, slot: u8) -> Option<(LinkStatus, DeviceStatus)> {
        let l = self.snapshot.links.iter().find(|l| l.key == link)?;
        let d = l.devices.iter().find(|d| d.slot == slot)?;
        Some((l.clone(), d.clone()))
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
                ui.label(RichText::new(&d.name).size(26.0).strong());
                ui.horizontal(|ui| {
                    if let Some(b) = &d.battery {
                        let (ic, color) = style::battery_icon(b);
                        ui.label(RichText::new(format!("{ic} {b}")).color(color));
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

        match self.tab {
            Tab::Buttons => self.buttons_tab(ui, &d, kind, None),
            Tab::Pointer => self.pointer_tab(ui, &d, kind),
            Tab::Keyboard => self.keyboard_tab(ui, &d, kind),
            Tab::Programs => self.programs_tab(ui, &d, kind),
            Tab::Computers => self.computers_tab(ui, link, &d),
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
        for c in &details.controls {
            let current = match app {
                None => profile.buttons.get(&c.key).cloned(),
                Some(i) => profile
                    .apps
                    .get(i)
                    .and_then(|a| a.buttons.get(&c.key).cloned()),
            };
            card_row(ui, pal.card, pal.border, |ui| {
                ui.label(
                    RichText::new(icon::CURSOR_CLICK)
                        .size(20.0)
                        .color(if c.remappable { ACCENT } else { pal.muted }),
                );
                ui.vertical(|ui| {
                    ui.label(RichText::new(control_label(t, &c.key, &c.label)).strong());
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
                        if ui
                            .button(format!("{} {}", icon::PENCIL, t.change))
                            .clicked()
                        {
                            self.editor = Some(Editor {
                                device: d.name.clone(),
                                kind,
                                app,
                                button_key: c.key.clone(),
                                button_label: control_label(t, &c.key, &c.label).to_owned(),
                                gestures: c.gestures,
                                dpi_values: details.dpi_values.clone(),
                                form: ActionForm::from_raw(current.as_ref()),
                            });
                        }
                    });
                }
            });
        }
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
                let mut keep = p.dpi.is_none();
                if ui.checkbox(&mut keep, t.keep_device_value).changed() {
                    p.dpi = if keep {
                        None
                    } else {
                        Some(d.dpi.unwrap_or(details.dpi_values[0]))
                    };
                }
                if let Some(dpi) = p.dpi.as_mut() {
                    dpi_slider(ui, dpi, &details.dpi_values);
                }
            }
            if details.pointer_speed {
                ui.label(RichText::new(t.pointer_speed).strong());
                let mut keep = p.pointer_speed.is_none();
                if ui.checkbox(&mut keep, t.keep_device_value).changed() {
                    p.pointer_speed = if keep { None } else { Some(1.0) };
                }
                if let Some(speed) = p.pointer_speed.as_mut() {
                    ui.add(egui::Slider::new(speed, 0.2..=2.0).step_by(0.05));
                }
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
            ui.set_width(520.0);
            ui.label(
                RichText::new(fill(t.edit_button, &[&ed.button_label]))
                    .size(20.0)
                    .strong(),
            );
            ui.add_space(8.0);
            ui.label(RichText::new(t.action).strong());
            let fc = FormContext {
                t,
                dpi_values: &ed.dpi_values,
                gestures: ed.gestures,
                macros: self.raw.macros.keys().cloned().collect(),
                dark: ui.visuals().dark_mode,
            };
            ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
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
    // Macros
    // ------------------------------------------------------------------

    fn macros_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.macros_title).size(28.0).strong());
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
                        let mut new_name = name.clone();
                        if ui
                            .add(egui::TextEdit::singleline(&mut new_name).desired_width(200.0))
                            .lost_focus()
                            && new_name != name
                        {
                            let clean: String = new_name
                                .trim()
                                .chars()
                                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                                .collect();
                            if !clean.is_empty() {
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
    // Settings and about
    // ------------------------------------------------------------------

    fn settings_page(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = palette(ui.visuals().dark_mode);
        ui.label(RichText::new(t.settings_title).size(28.0).strong());
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
            &format!("{}  {}", icon::BATTERY_LOW, t.battery),
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
            },
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
                            std::thread::sleep(Duration::from_millis(1500));
                            start_service(
                                &self.exe,
                                self.custom_config.then_some(self.config_path.as_path()),
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
                        );
                        self.last_attempt = None;
                    }
                });
            },
        );

        let g = &self.raw.general;
        if g.language != before.language
            || g.theme != before.theme
            || g.battery_warning != before.battery_warning
            || self.raw.logging.level != level_before
        {
            self.lang = Lang::from_setting(&self.raw.general.language);
            self.t = self.lang.texts();
            self.apply_theme(ui.ctx());
            self.changed();
        }
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
            ui.label(RichText::new("logirev").size(34.0).strong());
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
        let pal = palette(ui.visuals().dark_mode);
        egui::Panel::left("nav")
            .exact_size(220.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(pal.sidebar)
                    .inner_margin(Margin::same(16)),
            )
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(Margin::symmetric(28, 20)),
            )
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(900.0));
                        self.content(ui);
                    });
            });
        self.editor_dialog(&ctx);
    }
}

// ----------------------------------------------------------------------
// Small widgets and helpers
// ----------------------------------------------------------------------

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
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(17.0).strong());
            ui.add_space(4.0);
            add(ui);
        });
    ui.add_space(10.0);
}

/// A one-line card.
fn card_row(ui: &mut Ui, fill_color: Color32, border: Color32, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add);
        });
    ui.add_space(6.0);
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

/// A setting that can be on, off or "leave it as it is".
fn tri_state(ui: &mut Ui, value: &mut Option<bool>, label: &str, t: &T) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.selectable_value(value, None, t.not_set);
        ui.selectable_value(value, Some(true), "✔");
        ui.selectable_value(value, Some(false), "✖");
    });
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

/// Every button action anywhere in the configuration.
fn all_actions(raw: &mut RawConfig) -> impl Iterator<Item = &mut RawAction> {
    raw.profiles.iter_mut().flat_map(|p| {
        let apps = p.apps.iter_mut().flat_map(|a| a.buttons.values_mut());
        p.buttons.values_mut().chain(apps)
    })
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

/// Starts the background service (detached).
fn start_service(exe: &Path, config: Option<&Path>) {
    let mut cmd = std::process::Command::new(exe);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn renames_and_removes_macro_uses() {
        let mut raw = RawConfig::parse(
            "[macros.a]\nsteps = [ { keys = \"A\" } ]\n[[profile]]\nname = \"p\"\nbuttons.back = { action = \"macro\", name = \"a\" }",
        )
        .unwrap();
        rename_macro_uses(&mut raw, "a", "b");
        assert!(
            matches!(&raw.profiles[0].buttons["back"], RawAction::Macro { name, .. } if name == "b")
        );
        remove_macro_uses(&mut raw, "b");
        assert!(matches!(
            raw.profiles[0].buttons["back"],
            RawAction::Disable
        ));
    }

    #[test]
    fn parses_device_kinds() {
        assert_eq!(kind_of("trackball"), DeviceKind::Trackball);
        assert_eq!(kind_of("weird"), DeviceKind::Other);
        let _ = Config::parse("", Path::new(".")).unwrap();
    }
}
