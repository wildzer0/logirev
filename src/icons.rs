//! Icons of the interface, shared by the settings window and the radial menu.
//!
//! They are characters of the Phosphor font (`res/fonts/Phosphor.ttf`, MIT),
//! so an icon can be embedded in any text. The font itself is included once
//! ([`PHOSPHOR_TTF`]) and used both by egui (settings window) and by GDI
//! (radial menu), so the binary contains a single copy.

// Some icons are only used by the Windows-only radial menu.
#![allow(dead_code)]

/// Phosphor icon characters used by the interface.
pub(crate) mod icon {
    pub const BATTERY_FULL: &str = "\u{E0C0}";
    pub const BATTERY_HIGH: &str = "\u{E0C2}";
    pub const BATTERY_MEDIUM: &str = "\u{E0C6}";
    pub const BATTERY_LOW: &str = "\u{E0C4}";
    pub const BATTERY_EMPTY: &str = "\u{E0BE}";
    pub const BATTERY_CHARGING: &str = "\u{E0BA}";
    pub const MOUSE: &str = "\u{E33A}";
    pub const KEYBOARD: &str = "\u{E2D8}";
    pub const GEAR: &str = "\u{E272}";
    pub const INFO: &str = "\u{E2CE}";
    pub const PLUS: &str = "\u{E3D4}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const PENCIL: &str = "\u{E3B4}";
    pub const RECORD: &str = "\u{E3EE}";
    pub const X: &str = "\u{E4F6}";
    pub const CHECK: &str = "\u{E182}";
    pub const CARET_LEFT: &str = "\u{E138}";
    pub const CARET_RIGHT: &str = "\u{E13A}";
    pub const CARET_UP: &str = "\u{E13C}";
    pub const CARET_DOWN: &str = "\u{E136}";
    pub const DESKTOP: &str = "\u{E560}";
    pub const SUN: &str = "\u{E472}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const FOLDER_OPEN: &str = "\u{E256}";
    pub const FILE_TEXT: &str = "\u{E23A}";
    pub const POWER: &str = "\u{E3DA}";
    pub const BROADCAST: &str = "\u{E0F2}";
    pub const USB: &str = "\u{E956}";
    pub const BLUETOOTH: &str = "\u{E0DA}";
    pub const SLIDERS: &str = "\u{E432}";
    pub const MAGIC_WAND: &str = "\u{E6B6}";
    pub const ARROWS_CLOCKWISE: &str = "\u{E094}";
    pub const CHECK_CIRCLE: &str = "\u{E184}";
    pub const X_CIRCLE: &str = "\u{E4F8}";
    pub const CURSOR_CLICK: &str = "\u{E7C8}";
    pub const APP_WINDOW: &str = "\u{E5DA}";
    pub const PRESENTATION: &str = "\u{E654}";
    pub const LOCK: &str = "\u{E2FA}";
    pub const HAND_POINTING: &str = "\u{E29A}";
    pub const ARROW_LEFT: &str = "\u{E058}";
    pub const ARROW_RIGHT: &str = "\u{E06C}";
    pub const ARROW_U_UP_LEFT: &str = "\u{E08A}";
    pub const ARROW_U_UP_RIGHT: &str = "\u{E08C}";
    pub const ARROW_CLOCKWISE: &str = "\u{E036}";
    pub const ARROW_COUNTER_CLOCKWISE: &str = "\u{E038}";
    pub const COPY: &str = "\u{E1CA}";
    pub const CLIPBOARD: &str = "\u{E196}";
    pub const SCISSORS: &str = "\u{EAE0}";
    pub const SELECTION_ALL: &str = "\u{E746}";
    pub const SQUARES_FOUR: &str = "\u{E464}";
    pub const CAMERA: &str = "\u{E10E}";
    pub const ZOOM_IN: &str = "\u{E310}";
    pub const ZOOM_OUT: &str = "\u{E30E}";
    pub const PLUS_SQUARE: &str = "\u{ED4A}";
    pub const X_SQUARE: &str = "\u{E4FA}";
    pub const PLAY_PAUSE: &str = "\u{E8BE}";
    pub const SKIP_FORWARD: &str = "\u{E5A6}";
    pub const SKIP_BACK: &str = "\u{E5A4}";
    pub const SPEAKER_HIGH: &str = "\u{E44A}";
    pub const SPEAKER_LOW: &str = "\u{E44C}";
    pub const SPEAKER_X: &str = "\u{E45C}";
    pub const PROHIBIT: &str = "\u{E3DE}";
    pub const MOUSE_LEFT_CLICK: &str = "\u{E334}";
    pub const MOUSE_MIDDLE_CLICK: &str = "\u{E338}";
    pub const MOUSE_SCROLL: &str = "\u{E332}";
    pub const TERMINAL_WINDOW: &str = "\u{EAE8}";
    pub const CROSSHAIR: &str = "\u{E1D6}";
    pub const GAUGE: &str = "\u{E628}";
    pub const LIGHTBULB: &str = "\u{E2DC}";
    pub const STACK: &str = "\u{E466}";
    pub const HAND_SWIPE_RIGHT: &str = "\u{EC92}";
    pub const CODE: &str = "\u{E1BC}";
    pub const CPU: &str = "\u{E610}";
    pub const PAUSE: &str = "\u{E39E}";
    pub const PLAY: &str = "\u{E3D0}";
    pub const CLIPBOARD_TEXT: &str = "\u{E198}";
    pub const FLOPPY_DISK: &str = "\u{E248}";
}

// Re-exported so callers can write `icons::CODE` or `icons::icon::CODE`.
pub(crate) use icon::*;

/// The Phosphor font, embedded once and shared by egui and GDI.
#[cfg(any(feature = "gui", windows))]
pub(crate) static PHOSPHOR_TTF: &[u8] = include_bytes!("../res/fonts/Phosphor.ttf");

#[cfg(test)]
mod tests {
    #[test]
    fn icons_are_single_characters() {
        assert_eq!(super::icon::CODE.chars().count(), 1);
        assert_eq!(super::icon::X_CIRCLE.chars().count(), 1);
        assert_eq!(super::icon::TERMINAL_WINDOW.chars().count(), 1);
    }
}
