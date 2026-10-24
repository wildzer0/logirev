//! The form used to choose what a button does.
//!
//! [`ActionForm`] holds the editable fields of every kind of action at once,
//! so switching the kind back and forth does not lose what was typed. It is
//! built from a [`RawAction`] (as stored in the configuration file) and
//! turned back into one with [`ActionForm::to_raw`].

use eframe::egui::{self, ComboBox, RichText, Ui};

use super::i18n::{fill, T};
use super::style::{icon, palette};
use crate::config::{BacklightChange, RawAction, RawHost, ScrollDirection};
use crate::input::keys::{MouseButton, KEYS};

/// The kinds of action offered in the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Default,
    Disable,
    Keys,
    Click,
    Scroll,
    Command,
    SlowDpi,
    CycleDpi,
    ChangeHost,
    Brightness,
    Backlight,
    Macro,
    Gesture,
    /// An action written by hand that the form does not edit (kept as is).
    Custom,
}

impl Kind {
    fn label(self, t: &T) -> &'static str {
        match self {
            Kind::Default => t.act_default,
            Kind::Disable => t.act_disable,
            Kind::Keys => t.act_keys,
            Kind::Click => t.act_click,
            Kind::Scroll => t.act_scroll,
            Kind::Command => t.act_command,
            Kind::SlowDpi => t.act_slow,
            Kind::CycleDpi => t.act_cycle,
            Kind::ChangeHost => t.act_host,
            Kind::Brightness => t.act_brightness,
            Kind::Backlight => t.act_backlight,
            Kind::Macro => t.act_macro,
            Kind::Gesture => t.act_gesture,
            Kind::Custom => t.act_custom,
        }
    }
}

/// What the form needs to know about the device and the configuration.
pub struct FormContext<'a> {
    pub t: &'static T,
    pub dpi_values: &'a [u16],
    /// The button can follow movement (gestures).
    pub gestures: bool,
    pub macros: Vec<String>,
    pub dark: bool,
}

/// Editable fields of every kind of action.
#[derive(Debug, Clone)]
pub struct ActionForm {
    pub kind: Kind,
    /// Key names, modifiers first (`["Ctrl", "Shift", "T"]`).
    keys: Vec<String>,
    recording: bool,
    button: MouseButton,
    scroll_dir: ScrollDirection,
    scroll_amount: u8,
    program: String,
    args: String,
    slow_dpi: u16,
    cycle: Vec<u16>,
    /// 0 = next computer, 1..=3 = that computer.
    host: u8,
    host_all: bool,
    brightness_set: bool,
    brightness_value: i32,
    backlight: BacklightChange,
    macro_name: String,
    repeat: bool,
    repeat_ms: u32,
    /// Gesture directions: up, down, left, right, press.
    gesture: Vec<ActionForm>,
    threshold: u16,
    /// The original action when `kind` is `Custom`.
    custom: Option<RawAction>,
}

impl Default for ActionForm {
    fn default() -> Self {
        Self {
            kind: Kind::Default,
            keys: Vec::new(),
            recording: false,
            button: MouseButton::Middle,
            scroll_dir: ScrollDirection::Down,
            scroll_amount: 1,
            program: String::new(),
            args: String::new(),
            slow_dpi: 400,
            cycle: vec![800, 1600],
            host: 0,
            host_all: false,
            brightness_set: false,
            brightness_value: 10,
            backlight: BacklightChange::Toggle,
            macro_name: String::new(),
            repeat: false,
            repeat_ms: 150,
            gesture: Vec::new(),
            threshold: 50,
            custom: None,
        }
    }
}

impl ActionForm {
    /// The form for an existing action (`None` = default behaviour).
    pub fn from_raw(raw: Option<&RawAction>) -> Self {
        let mut f = Self::default();
        let Some(raw) = raw else { return f };
        match raw {
            RawAction::Default => {}
            RawAction::Disable => f.kind = Kind::Disable,
            RawAction::Keys { keys } => {
                f.kind = Kind::Keys;
                f.keys = keys
                    .split('+')
                    .map(|k| k.trim().to_owned())
                    .filter(|k| !k.is_empty())
                    .collect();
            }
            RawAction::Click { button } => {
                f.kind = Kind::Click;
                f.button = *button;
            }
            RawAction::Scroll { direction, amount } => {
                f.kind = Kind::Scroll;
                f.scroll_dir = *direction;
                f.scroll_amount = (*amount).max(1);
            }
            RawAction::Command { command, args } => {
                f.kind = Kind::Command;
                f.program = command.clone();
                f.args = args.join(" ");
            }
            RawAction::ToggleDpi { dpi } => {
                f.kind = Kind::SlowDpi;
                f.slow_dpi = *dpi;
            }
            RawAction::CycleDpi { dpi } => {
                f.kind = Kind::CycleDpi;
                f.cycle = dpi.clone();
            }
            RawAction::ChangeHost { host, all } => {
                f.kind = Kind::ChangeHost;
                f.host = match host {
                    RawHost::Number(n) => (*n).clamp(1, 3),
                    RawHost::Name(_) => 0,
                };
                f.host_all = *all;
            }
            RawAction::Brightness { change, set } => {
                f.kind = Kind::Brightness;
                f.brightness_set = set.is_some();
                f.brightness_value = set.map(i32::from).or(change.map(i32::from)).unwrap_or(10);
            }
            RawAction::Backlight { change } => {
                f.kind = Kind::Backlight;
                f.backlight = *change;
            }
            RawAction::Macro { name, repeat } => {
                f.kind = Kind::Macro;
                f.macro_name = name.clone();
                f.repeat = repeat.is_some();
                f.repeat_ms = repeat.unwrap_or(150);
            }
            RawAction::Gesture {
                up,
                down,
                left,
                right,
                click,
                threshold,
            } => {
                f.kind = Kind::Gesture;
                f.gesture = [up, down, left, right, click]
                    .iter()
                    .map(|a| ActionForm::from_raw(a.as_deref()))
                    .collect();
                f.threshold = threshold.unwrap_or(50);
            }
            other => {
                f.kind = Kind::Custom;
                f.custom = Some(other.clone());
            }
        }
        f
    }

    /// The action to store; `None` means "default behaviour".
    pub fn to_raw(&self) -> Option<RawAction> {
        Some(match self.kind {
            Kind::Default => return None,
            Kind::Disable => RawAction::Disable,
            Kind::Keys if self.keys.is_empty() => return None,
            Kind::Keys => RawAction::Keys {
                keys: self.keys.join("+"),
            },
            Kind::Click => RawAction::Click {
                button: self.button,
            },
            Kind::Scroll => RawAction::Scroll {
                direction: self.scroll_dir,
                amount: self.scroll_amount,
            },
            Kind::Command if self.program.trim().is_empty() => return None,
            Kind::Command => RawAction::Command {
                command: self.program.trim().to_owned(),
                args: self.args.split_whitespace().map(str::to_owned).collect(),
            },
            Kind::SlowDpi => RawAction::ToggleDpi { dpi: self.slow_dpi },
            Kind::CycleDpi if self.cycle.is_empty() => return None,
            Kind::CycleDpi => RawAction::CycleDpi {
                dpi: self.cycle.clone(),
            },
            Kind::ChangeHost => RawAction::ChangeHost {
                host: if self.host == 0 {
                    RawHost::Name("next".into())
                } else {
                    RawHost::Number(self.host)
                },
                all: self.host_all,
            },
            Kind::Brightness => {
                if self.brightness_set {
                    RawAction::Brightness {
                        change: None,
                        set: Some(self.brightness_value.clamp(0, 100) as u8),
                    }
                } else {
                    let v = self.brightness_value.clamp(-100, 100) as i8;
                    RawAction::Brightness {
                        change: Some(if v == 0 { 10 } else { v }),
                        set: None,
                    }
                }
            }
            Kind::Backlight => RawAction::Backlight {
                change: self.backlight,
            },
            Kind::Macro if self.macro_name.is_empty() => return None,
            Kind::Macro => RawAction::Macro {
                name: self.macro_name.clone(),
                repeat: self.repeat.then_some(self.repeat_ms.max(30)),
            },
            Kind::Gesture => {
                let sub = |i: usize| {
                    self.gesture
                        .get(i)
                        .and_then(ActionForm::to_raw)
                        .map(Box::new)
                };
                RawAction::Gesture {
                    up: sub(0),
                    down: sub(1),
                    left: sub(2),
                    right: sub(3),
                    click: sub(4),
                    threshold: (self.threshold != 50).then_some(self.threshold),
                }
            }
            Kind::Custom => return self.custom.clone(),
        })
    }

    /// One-line description, shown in button lists.
    pub fn summary(&self, t: &T) -> String {
        match self.kind {
            Kind::Keys if !self.keys.is_empty() => self.keys.join(" + "),
            Kind::Click => button_label(self.button, t).to_owned(),
            Kind::Scroll => format!(
                "{} {} ×{}",
                t.act_scroll,
                dir_label(self.scroll_dir, t),
                self.scroll_amount
            ),
            Kind::Command => format!("{} {}", t.act_command, file_name(&self.program)),
            Kind::SlowDpi => format!("{} · {} DPI", t.act_slow, self.slow_dpi),
            Kind::CycleDpi => format!(
                "DPI {}",
                self.cycle
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(" → ")
            ),
            Kind::ChangeHost => {
                if self.host == 0 {
                    t.host_next.to_owned()
                } else {
                    fill(t.host_n, &[&self.host.to_string()])
                }
            }
            Kind::Brightness => {
                if self.brightness_set {
                    format!("{} {}%", t.act_brightness, self.brightness_value)
                } else {
                    format!("{} {:+}%", t.act_brightness, self.brightness_value)
                }
            }
            Kind::Macro => format!("{} \"{}\"", t.act_macro, self.macro_name),
            k => k.label(t).to_owned(),
        }
    }

    /// Draws the form. `nested` = inside a gesture (no gestures there).
    pub fn ui(&mut self, ui: &mut Ui, fc: &FormContext, id: &str, nested: bool) {
        let t = fc.t;
        let mut kinds = vec![
            Kind::Default,
            Kind::Disable,
            Kind::Keys,
            Kind::Click,
            Kind::Scroll,
            Kind::Command,
        ];
        if !nested {
            if !fc.dpi_values.is_empty() {
                kinds.extend([Kind::SlowDpi, Kind::CycleDpi]);
            }
            kinds.extend([
                Kind::ChangeHost,
                Kind::Brightness,
                Kind::Backlight,
                Kind::Macro,
                Kind::Gesture,
            ]);
        } else {
            kinds.extend([Kind::ChangeHost, Kind::Brightness, Kind::Macro]);
        }
        if self.kind == Kind::Custom {
            kinds.push(Kind::Custom);
        }

        ComboBox::from_id_salt(format!("{id}-kind"))
            .width(280.0)
            .selected_text(self.kind.label(t))
            .show_ui(ui, |ui| {
                for k in kinds {
                    ui.selectable_value(&mut self.kind, k, k.label(t));
                }
            });
        ui.add_space(4.0);

        match self.kind {
            Kind::Keys => self.keys_ui(ui, fc, id),
            Kind::Click => {
                ComboBox::from_id_salt(format!("{id}-btn"))
                    .selected_text(button_label(self.button, t))
                    .show_ui(ui, |ui| {
                        for b in [
                            MouseButton::Left,
                            MouseButton::Right,
                            MouseButton::Middle,
                            MouseButton::Back,
                            MouseButton::Forward,
                        ] {
                            ui.selectable_value(&mut self.button, b, button_label(b, t));
                        }
                    });
            }
            Kind::Scroll => {
                ui.horizontal(|ui| {
                    for d in [
                        ScrollDirection::Up,
                        ScrollDirection::Down,
                        ScrollDirection::Left,
                        ScrollDirection::Right,
                    ] {
                        ui.selectable_value(&mut self.scroll_dir, d, dir_label(d, t));
                    }
                });
                ui.add(egui::Slider::new(&mut self.scroll_amount, 1..=10).text(t.amount));
            }
            Kind::Command => {
                ui.label(t.program);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.program)
                            .desired_width(300.0)
                            .hint_text("calc.exe"),
                    );
                    #[cfg(windows)]
                    if ui.button(t.browse).clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Programs", &["exe", "bat", "cmd", "lnk"])
                            .pick_file()
                        {
                            self.program = p.display().to_string();
                        }
                    }
                });
                ui.label(t.arguments);
                ui.add(egui::TextEdit::singleline(&mut self.args).desired_width(300.0));
            }
            Kind::SlowDpi => {
                ui.label(t.slow_dpi);
                dpi_slider(ui, &mut self.slow_dpi, fc.dpi_values);
            }
            Kind::CycleDpi => {
                ui.label(t.cycle_values);
                let mut remove = None;
                for (i, v) in self.cycle.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        dpi_slider(ui, v, fc.dpi_values);
                        if ui.small_button(icon::TRASH).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    self.cycle.remove(i);
                }
                if ui
                    .button(format!("{} {}", icon::PLUS, t.add_value))
                    .clicked()
                {
                    let last = self.cycle.last().copied().unwrap_or(800);
                    self.cycle.push(last);
                }
            }
            Kind::ChangeHost => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.host, 0, t.host_next);
                    for n in 1..=3u8 {
                        ui.selectable_value(&mut self.host, n, fill(t.host_n, &[&n.to_string()]));
                    }
                });
                ui.checkbox(&mut self.host_all, t.host_all);
            }
            Kind::Brightness => {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(!self.brightness_set, t.brightness_change)
                        .clicked()
                    {
                        self.brightness_set = false;
                        self.brightness_value = 10;
                    }
                    if ui
                        .selectable_label(self.brightness_set, t.brightness_set)
                        .clicked()
                    {
                        self.brightness_set = true;
                        self.brightness_value = 50;
                    }
                });
                if self.brightness_set {
                    ui.add(egui::Slider::new(&mut self.brightness_value, 0..=100).suffix("%"));
                } else {
                    ui.add(egui::Slider::new(&mut self.brightness_value, -50..=50).suffix("%"));
                }
            }
            Kind::Backlight => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.backlight, BacklightChange::Up, t.backlight_up);
                    ui.selectable_value(
                        &mut self.backlight,
                        BacklightChange::Down,
                        t.backlight_down,
                    );
                    ui.selectable_value(
                        &mut self.backlight,
                        BacklightChange::Toggle,
                        t.backlight_toggle,
                    );
                });
            }
            Kind::Macro => {
                if fc.macros.is_empty() {
                    ui.label(RichText::new(t.no_macros).color(palette(fc.dark).muted));
                } else {
                    if !fc.macros.contains(&self.macro_name) {
                        self.macro_name = fc.macros[0].clone();
                    }
                    ComboBox::from_id_salt(format!("{id}-macro"))
                        .selected_text(&self.macro_name)
                        .show_ui(ui, |ui| {
                            for m in &fc.macros {
                                ui.selectable_value(&mut self.macro_name, m.clone(), m);
                            }
                        });
                    if !nested {
                        ui.checkbox(&mut self.repeat, t.repeat_held);
                        if self.repeat {
                            ui.add(
                                egui::Slider::new(&mut self.repeat_ms, 30..=1000).text(t.every_ms),
                            );
                        }
                    }
                }
            }
            Kind::Gesture => self.gesture_ui(ui, fc, id),
            Kind::Custom => {
                ui.label(RichText::new(t.custom_hint).color(palette(fc.dark).muted));
            }
            Kind::Default | Kind::Disable => {}
        }
    }

    fn keys_ui(&mut self, ui: &mut Ui, fc: &FormContext, id: &str) {
        let t = fc.t;
        ui.label(
            RichText::new(t.shortcut_hint)
                .small()
                .color(palette(fc.dark).muted),
        );
        ui.horizontal(|ui| {
            let shown = if self.recording {
                t.recording.to_owned()
            } else if self.keys.is_empty() {
                "—".to_owned()
            } else {
                self.keys.join(" + ")
            };
            ui.label(RichText::new(shown).strong().size(18.0));
            let label = format!("{} {}", icon::RECORD, t.record);
            if ui.selectable_label(self.recording, label).clicked() {
                self.recording = !self.recording;
            }
        });
        if self.recording {
            if let Some(keys) = recorded_shortcut(ui.ctx()) {
                self.keys = keys;
                self.recording = false;
            }
            ui.ctx().request_repaint();
        }

        // Manual selection: modifiers + one key.
        let (mut ctrl, mut shift, mut alt, mut win) = (
            self.has("Ctrl"),
            self.has("Shift"),
            self.has("Alt"),
            self.has("Super"),
        );
        let mut key = self
            .keys
            .iter()
            .find(|k| !is_modifier(k))
            .cloned()
            .unwrap_or_default();
        let before = (ctrl, shift, alt, win, key.clone());
        ui.horizontal(|ui| {
            ui.toggle_value(&mut ctrl, "Ctrl");
            ui.toggle_value(&mut shift, "Shift");
            ui.toggle_value(&mut alt, "Alt");
            ui.toggle_value(&mut win, "Win");
            ComboBox::from_id_salt(format!("{id}-key"))
                .selected_text(if key.is_empty() { t.key } else { key.as_str() })
                .height(320.0)
                .show_ui(ui, |ui| {
                    for k in KEYS.iter().filter(|k| !is_modifier(k.name)) {
                        ui.selectable_value(&mut key, k.name.to_owned(), k.name);
                    }
                });
        });
        if before != (ctrl, shift, alt, win, key.clone()) {
            let mut keys = Vec::new();
            for (on, name) in [
                (ctrl, "Ctrl"),
                (shift, "Shift"),
                (alt, "Alt"),
                (win, "Super"),
            ] {
                if on {
                    keys.push(name.to_owned());
                }
            }
            if !key.is_empty() {
                keys.push(key);
            }
            self.keys = keys;
        }
    }

    fn has(&self, name: &str) -> bool {
        self.keys.iter().any(|k| k.eq_ignore_ascii_case(name))
    }

    fn gesture_ui(&mut self, ui: &mut Ui, fc: &FormContext, id: &str) {
        let t = fc.t;
        if !fc.gestures {
            ui.colored_label(
                super::style::WARN,
                format!("{} {}", icon::WARNING, t.gesture_no_movement),
            );
        }
        if self.gesture.len() != 5 {
            self.gesture = vec![ActionForm::default(); 5];
        }
        let labels = [
            t.dir_up,
            t.dir_down,
            t.dir_left,
            t.dir_right,
            t.gesture_click,
        ];
        egui::Grid::new(format!("{id}-gesture"))
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                for (i, label) in labels.iter().enumerate() {
                    ui.label(RichText::new(*label).strong());
                    ui.vertical(|ui| self.gesture[i].ui(ui, fc, &format!("{id}-g{i}"), true));
                    ui.end_row();
                }
            });
    }
}

fn is_modifier(name: &str) -> bool {
    matches!(
        name,
        "Ctrl" | "Shift" | "Alt" | "Super" | "RightCtrl" | "RightShift" | "RightAlt"
    )
}

fn button_label(b: MouseButton, t: &T) -> &'static str {
    match b {
        MouseButton::Left => t.button_left,
        MouseButton::Right => t.button_right,
        MouseButton::Middle => t.button_middle,
        MouseButton::Back => t.button_back,
        MouseButton::Forward => t.button_forward,
    }
}

fn dir_label(d: ScrollDirection, t: &T) -> &'static str {
    match d {
        ScrollDirection::Up => t.dir_up,
        ScrollDirection::Down => t.dir_down,
        ScrollDirection::Left => t.dir_left,
        ScrollDirection::Right => t.dir_right,
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// A slider over the DPI values the device accepts.
pub fn dpi_slider(ui: &mut Ui, value: &mut u16, values: &[u16]) {
    if values.is_empty() {
        ui.add(egui::DragValue::new(value).range(100..=8000).speed(50));
        return;
    }
    let mut index = values
        .iter()
        .position(|v| *v >= *value)
        .unwrap_or(values.len() - 1);
    let resp = ui.add(
        egui::Slider::new(&mut index, 0..=values.len() - 1)
            .show_value(false)
            .custom_formatter(|i, _| {
                values
                    .get(i as usize)
                    .map(u16::to_string)
                    .unwrap_or_default()
            }),
    );
    *value = values[index];
    ui.label(RichText::new(format!("{} DPI", value)).strong());
    let _ = resp;
}

/// The shortcut pressed in this frame, as key names (modifiers first).
fn recorded_shortcut(ctx: &egui::Context) -> Option<Vec<String>> {
    ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let name = key_name(*key)?;
                let mut keys = Vec::new();
                if modifiers.ctrl {
                    keys.push("Ctrl".to_owned());
                }
                if modifiers.shift {
                    keys.push("Shift".to_owned());
                }
                if modifiers.alt {
                    keys.push("Alt".to_owned());
                }
                keys.push(name.to_owned());
                Some(keys)
            }
            _ => None,
        })
    })
}

/// The logirev key name for an egui key, if logirev knows it.
pub fn key_name(key: egui::Key) -> Option<&'static str> {
    use egui::Key as K;
    let name = match key {
        K::ArrowDown => "Down",
        K::ArrowLeft => "Left",
        K::ArrowRight => "Right",
        K::ArrowUp => "Up",
        K::Escape => "Escape",
        K::Tab => "Tab",
        K::Backspace => "Backspace",
        K::Enter => "Enter",
        K::Space => "Space",
        K::Insert => "Insert",
        K::Delete => "Delete",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "PageUp",
        K::PageDown => "PageDown",
        K::Comma => "Comma",
        K::Backslash => "Backslash",
        K::Slash => "Slash",
        K::OpenBracket => "LeftBracket",
        K::CloseBracket => "RightBracket",
        K::Backtick => "Grave",
        K::Minus => "Minus",
        K::Period => "Period",
        K::Equals | K::Plus => "Equal",
        K::Semicolon => "Semicolon",
        K::Quote => "Apostrophe",
        K::BrowserBack => "BrowserBack",
        K::BrowserForward => "BrowserForward",
        K::BrowserRefresh => "BrowserRefresh",
        K::BrowserHome => "BrowserHome",
        K::MediaPlayPause => "MediaPlayPause",
        K::MediaTrackNext => "MediaNext",
        K::MediaTrackPrevious => "MediaPrevious",
        K::MediaStop => "MediaStop",
        K::AudioVolumeMute => "VolumeMute",
        K::AudioVolumeDown => "VolumeDown",
        K::AudioVolumeUp => "VolumeUp",
        K::LaunchMail => "Mail",
        K::CapsLock => "CapsLock",
        K::ScrollLock => "ScrollLock",
        K::PrintScreen => "PrintScreen",
        K::Pause => "Pause",
        K::Menu => "Menu",
        other => {
            // Letters, digits (Num0..Num9) and F1..F24 share our names.
            let n = other.name();
            let n = n.strip_prefix("Num").filter(|d| d.len() == 1).unwrap_or(n);
            return KEYS
                .iter()
                .find(|k| k.name.eq_ignore_ascii_case(n))
                .map(|k| k.name);
        }
    };
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RawConfig;

    fn round_trip(toml_action: &str) {
        let cfg = RawConfig::parse(&format!(
            "[[profile]]\nname = \"p\"\nbuttons.back = {toml_action}"
        ))
        .unwrap();
        let raw = &cfg.profiles[0].buttons["back"];
        let form = ActionForm::from_raw(Some(raw));
        assert_ne!(form.kind, Kind::Custom, "{toml_action}");
        let back = form.to_raw().unwrap();
        assert_eq!(
            toml::to_string(&back).unwrap(),
            toml::to_string(raw).unwrap(),
            "{toml_action}"
        );
    }

    #[test]
    fn forms_preserve_actions() {
        round_trip(r#"{ action = "keys", keys = "Ctrl+Shift+T" }"#);
        round_trip(r#"{ action = "click", button = "middle" }"#);
        round_trip(r#"{ action = "scroll", direction = "left", amount = 3 }"#);
        round_trip(r#"{ action = "toggle-dpi", dpi = 400 }"#);
        round_trip(r#"{ action = "cycle-dpi", dpi = [400, 800] }"#);
        round_trip(r#"{ action = "change-host", host = 2, all = true }"#);
        round_trip(r#"{ action = "brightness", change = -10 }"#);
        round_trip(r#"{ action = "brightness", set = 70 }"#);
        round_trip(r#"{ action = "backlight", change = "up" }"#);
        round_trip(r#"{ action = "macro", name = "m", repeat = 200 }"#);
    }

    #[test]
    fn keeps_hand_written_actions() {
        let cfg = RawConfig::parse(
            "[[profile]]\nname = \"p\"\nbuttons.back = { action = \"sequence\", steps = [ { keys = \"A\" } ] }",
        )
        .unwrap();
        let raw = cfg.profiles[0].buttons["back"].clone();
        let form = ActionForm::from_raw(Some(&raw));
        assert_eq!(form.kind, Kind::Custom);
        assert!(form.to_raw().is_some());
        assert!(ActionForm::from_raw(None).to_raw().is_none());
    }

    #[test]
    fn maps_egui_keys() {
        assert_eq!(key_name(egui::Key::A), Some("A"));
        assert_eq!(key_name(egui::Key::Num7), Some("7"));
        assert_eq!(key_name(egui::Key::F5), Some("F5"));
        assert_eq!(key_name(egui::Key::ArrowLeft), Some("Left"));
    }
}
