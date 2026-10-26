//! Look of the settings window: icons, colours, fonts and spacing.
//!
//! Icons come from the Phosphor icon font (`res/fonts/Phosphor.ttf`, MIT):
//! each icon is a character, so it can be written inside any text, e.g.
//! `format!("{} Settings", icon::GEAR)`.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme, Visuals,
};

pub(crate) use crate::icons::icon;

/// Accent colours (same in both themes).
pub const ACCENT: Color32 = Color32::from_rgb(37, 99, 235);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(34, 211, 238);
pub const GOOD: Color32 = Color32::from_rgb(22, 163, 74);
pub const WARN: Color32 = Color32::from_rgb(217, 119, 6);
pub const BAD: Color32 = Color32::from_rgb(220, 38, 38);

/// Colours that depend on the theme.
pub struct Palette {
    pub card: Color32,
    pub card_hover: Color32,
    pub border: Color32,
    pub muted: Color32,
    pub sidebar: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            card: Color32::from_rgb(30, 34, 43),
            card_hover: Color32::from_rgb(37, 42, 54),
            border: Color32::from_rgb(48, 54, 66),
            muted: Color32::from_rgb(148, 156, 170),
            sidebar: Color32::from_rgb(22, 25, 32),
        }
    } else {
        Palette {
            card: Color32::WHITE,
            card_hover: Color32::from_rgb(244, 247, 252),
            border: Color32::from_rgb(226, 231, 238),
            muted: Color32::from_rgb(100, 110, 125),
            sidebar: Color32::from_rgb(240, 243, 248),
        }
    }
}

/// Installs fonts (text + icons) and the light/dark styles.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "phosphor".into(),
        FontData::from_static(crate::icons::PHOSPHOR_TTF).into(),
    );
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        // Second: the text font stays first, icons come right after it.
        let at = list.len().min(1);
        list.insert(at, "phosphor".into());
    }
    ctx.set_fonts(fonts);

    for (theme, dark) in [(Theme::Dark, true), (Theme::Light, false)] {
        let p = palette(dark);
        let mut v = if dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        v.panel_fill = if dark {
            Color32::from_rgb(17, 19, 24)
        } else {
            Color32::from_rgb(247, 249, 252)
        };
        v.window_fill = p.card;
        v.extreme_bg_color = if dark {
            Color32::from_rgb(12, 14, 18)
        } else {
            Color32::WHITE
        };
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.hyperlink_color = ACCENT_LIGHT;
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::new(1.0, p.border);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(8);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
        v.widgets.active.bg_fill = ACCENT;
        ctx.set_visuals_of(theme, v);
        ctx.style_mut_of(theme, |s| {
            s.spacing.item_spacing = egui::vec2(10.0, 10.0);
            s.spacing.button_padding = egui::vec2(12.0, 6.0);
            s.spacing.interact_size.y = 30.0;
            s.spacing.slider_width = 260.0;
            use egui::{FontId, TextStyle};
            s.text_styles
                .insert(TextStyle::Heading, FontId::proportional(24.0));
            s.text_styles
                .insert(TextStyle::Body, FontId::proportional(15.0));
            s.text_styles
                .insert(TextStyle::Button, FontId::proportional(15.0));
            s.text_styles
                .insert(TextStyle::Small, FontId::proportional(12.5));
        });
    }
}

/// Battery icon and colour for a level such as "30%" or "80% (charging)".
pub fn battery_icon(text: &str) -> (&'static str, Color32) {
    let percent: u8 = text
        .split('%')
        .next()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    if text.contains("charging") {
        return (icon::BATTERY_CHARGING, GOOD);
    }
    match percent {
        0..=10 => (icon::BATTERY_EMPTY, BAD),
        11..=25 => (icon::BATTERY_LOW, WARN),
        26..=60 => (icon::BATTERY_MEDIUM, GOOD),
        61..=90 => (icon::BATTERY_HIGH, GOOD),
        _ => (icon::BATTERY_FULL, GOOD),
    }
}

/// Icon for a device type ("mouse", "keyboard"...).
pub fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "keyboard" | "numpad" => icon::KEYBOARD,
        "presenter" | "remote" => icon::PRESENTATION,
        "trackpad" => icon::HAND_POINTING,
        _ => icon::MOUSE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_battery_icons() {
        assert_eq!(battery_icon("5%").0, icon::BATTERY_EMPTY);
        assert_eq!(battery_icon("30%"), (icon::BATTERY_MEDIUM, GOOD));
        assert_eq!(battery_icon("100% (full)").0, icon::BATTERY_FULL);
        assert_eq!(battery_icon("40% (charging)").0, icon::BATTERY_CHARGING);
    }
}
