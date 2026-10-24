//! Key names used in the configuration file and their OS key codes.
//!
//! Users write shortcuts as text, e.g. `"Ctrl+Shift+T"` or `"VolumeUp"`.
//! Each name maps to a Windows *virtual-key code* and a Linux *evdev* code so
//! the same configuration works on both systems. Names are case-insensitive.

use serde::Deserialize;

/// One keyboard key.
#[derive(Debug, PartialEq, Eq)]
pub struct Key {
    /// Canonical name, as shown in the documentation.
    pub name: &'static str,
    /// Windows virtual-key code (`VK_*`).
    pub vk: u16,
    /// Linux input event code (`KEY_*`).
    pub evdev: u16,
    /// Windows "extended key" flag (navigation cluster, right modifiers...).
    pub extended: bool,
}

const fn k(name: &'static str, vk: u16, evdev: u16) -> Key {
    Key {
        name,
        vk,
        evdev,
        extended: false,
    }
}

const fn kx(name: &'static str, vk: u16, evdev: u16) -> Key {
    Key {
        name,
        vk,
        evdev,
        extended: true,
    }
}

/// Every supported key.
#[rustfmt::skip]
pub static KEYS: &[Key] = &[
    // Modifiers
    k("Ctrl", 0xA2, 29),
    k("Shift", 0xA0, 42),
    k("Alt", 0xA4, 56),
    kx("Super", 0x5B, 125),
    kx("RightCtrl", 0xA3, 97),
    k("RightShift", 0xA1, 54),
    kx("RightAlt", 0xA5, 100),
    // Letters
    k("A", 0x41, 30), k("B", 0x42, 48), k("C", 0x43, 46), k("D", 0x44, 32),
    k("E", 0x45, 18), k("F", 0x46, 33), k("G", 0x47, 34), k("H", 0x48, 35),
    k("I", 0x49, 23), k("J", 0x4A, 36), k("K", 0x4B, 37), k("L", 0x4C, 38),
    k("M", 0x4D, 50), k("N", 0x4E, 49), k("O", 0x4F, 24), k("P", 0x50, 25),
    k("Q", 0x51, 16), k("R", 0x52, 19), k("S", 0x53, 31), k("T", 0x54, 20),
    k("U", 0x55, 22), k("V", 0x56, 47), k("W", 0x57, 17), k("X", 0x58, 45),
    k("Y", 0x59, 21), k("Z", 0x5A, 44),
    // Digits (top row)
    k("1", 0x31, 2), k("2", 0x32, 3), k("3", 0x33, 4), k("4", 0x34, 5),
    k("5", 0x35, 6), k("6", 0x36, 7), k("7", 0x37, 8), k("8", 0x38, 9),
    k("9", 0x39, 10), k("0", 0x30, 11),
    // Function keys
    k("F1", 0x70, 59), k("F2", 0x71, 60), k("F3", 0x72, 61), k("F4", 0x73, 62),
    k("F5", 0x74, 63), k("F6", 0x75, 64), k("F7", 0x76, 65), k("F8", 0x77, 66),
    k("F9", 0x78, 67), k("F10", 0x79, 68), k("F11", 0x7A, 87), k("F12", 0x7B, 88),
    k("F13", 0x7C, 183), k("F14", 0x7D, 184), k("F15", 0x7E, 185), k("F16", 0x7F, 186),
    k("F17", 0x80, 187), k("F18", 0x81, 188), k("F19", 0x82, 189), k("F20", 0x83, 190),
    k("F21", 0x84, 191), k("F22", 0x85, 192), k("F23", 0x86, 193), k("F24", 0x87, 194),
    // Editing and whitespace
    k("Escape", 0x1B, 1),
    k("Enter", 0x0D, 28),
    k("Tab", 0x09, 15),
    k("Space", 0x20, 57),
    k("Backspace", 0x08, 14),
    k("CapsLock", 0x14, 58),
    // Punctuation (US layout positions)
    k("Minus", 0xBD, 12), k("Equal", 0xBB, 13),
    k("LeftBracket", 0xDB, 26), k("RightBracket", 0xDD, 27),
    k("Backslash", 0xDC, 43), k("Semicolon", 0xBA, 39),
    k("Apostrophe", 0xDE, 40), k("Grave", 0xC0, 41),
    k("Comma", 0xBC, 51), k("Period", 0xBE, 52), k("Slash", 0xBF, 53),
    // Navigation cluster
    kx("Insert", 0x2D, 110), kx("Delete", 0x2E, 111),
    kx("Home", 0x24, 102), kx("End", 0x23, 107),
    kx("PageUp", 0x21, 104), kx("PageDown", 0x22, 109),
    kx("Left", 0x25, 105), kx("Right", 0x27, 106),
    kx("Up", 0x26, 103), kx("Down", 0x28, 108),
    kx("PrintScreen", 0x2C, 99), k("ScrollLock", 0x91, 70), k("Pause", 0x13, 119),
    kx("Menu", 0x5D, 127),
    // Numeric keypad
    k("Num0", 0x60, 82), k("Num1", 0x61, 79), k("Num2", 0x62, 80), k("Num3", 0x63, 81),
    k("Num4", 0x64, 75), k("Num5", 0x65, 76), k("Num6", 0x66, 77), k("Num7", 0x67, 71),
    k("Num8", 0x68, 72), k("Num9", 0x69, 73),
    // Media and browser keys
    k("VolumeMute", 0xAD, 113), k("VolumeDown", 0xAE, 114), k("VolumeUp", 0xAF, 115),
    k("MediaNext", 0xB0, 163), k("MediaPrevious", 0xB1, 165),
    k("MediaStop", 0xB2, 166), k("MediaPlayPause", 0xB3, 164),
    k("BrowserBack", 0xA6, 158), k("BrowserForward", 0xA7, 159),
    k("BrowserRefresh", 0xA8, 173), k("BrowserHome", 0xAC, 172),
    k("Mail", 0xB4, 155), k("Calculator", 0xB7, 140),
];

/// Alternative spellings accepted in the configuration.
const ALIASES: &[(&str, &str)] = &[
    ("control", "Ctrl"),
    ("win", "Super"),
    ("windows", "Super"),
    ("meta", "Super"),
    ("cmd", "Super"),
    ("altgr", "RightAlt"),
    ("esc", "Escape"),
    ("return", "Enter"),
    ("del", "Delete"),
    ("ins", "Insert"),
    ("pgup", "PageUp"),
    ("pgdn", "PageDown"),
    ("plus", "Equal"),
    ("mute", "VolumeMute"),
    ("playpause", "MediaPlayPause"),
];

/// Finds a key by name or alias (case-insensitive).
pub fn lookup(name: &str) -> Option<&'static Key> {
    let name = ALIASES
        .iter()
        .find(|(a, _)| a.eq_ignore_ascii_case(name))
        .map(|(_, canonical)| *canonical)
        .unwrap_or(name);
    KEYS.iter().find(|k| k.name.eq_ignore_ascii_case(name))
}

/// Parses a shortcut such as `"Ctrl+Alt+Delete"` into its keys, in order.
pub fn parse_chord(text: &str) -> Result<Vec<&'static Key>, String> {
    let keys: Result<Vec<_>, _> = text
        .split('+')
        .map(str::trim)
        .map(|part| lookup(part).ok_or_else(|| format!("unknown key \"{part}\" in \"{text}\"")))
        .collect();
    match keys {
        Ok(k) if k.is_empty() => Err("empty shortcut".into()),
        other => other,
    }
}

/// The key (and whether Shift is needed) that types `c` on a US keyboard
/// layout. Used to type text on Linux, where there is no "type this
/// character" API; Windows types any character directly.
pub fn char_to_key(c: char) -> Option<(&'static Key, bool)> {
    const SHIFTED: &[(char, &str)] = &[
        ('!', "1"),
        ('@', "2"),
        ('#', "3"),
        ('$', "4"),
        ('%', "5"),
        ('^', "6"),
        ('&', "7"),
        ('*', "8"),
        ('(', "9"),
        (')', "0"),
        ('_', "Minus"),
        ('+', "Equal"),
        ('{', "LeftBracket"),
        ('}', "RightBracket"),
        ('|', "Backslash"),
        (':', "Semicolon"),
        ('"', "Apostrophe"),
        ('~', "Grave"),
        ('<', "Comma"),
        ('>', "Period"),
        ('?', "Slash"),
    ];
    const PLAIN: &[(char, &str)] = &[
        (' ', "Space"),
        ('\n', "Enter"),
        ('\t', "Tab"),
        ('-', "Minus"),
        ('=', "Equal"),
        ('[', "LeftBracket"),
        (']', "RightBracket"),
        ('\\', "Backslash"),
        (';', "Semicolon"),
        ('\'', "Apostrophe"),
        ('`', "Grave"),
        (',', "Comma"),
        ('.', "Period"),
        ('/', "Slash"),
    ];
    if c.is_ascii_alphanumeric() {
        return lookup(&c.to_string()).map(|k| (k, c.is_ascii_uppercase()));
    }
    if let Some((_, name)) = PLAIN.iter().find(|(ch, _)| *ch == c) {
        return lookup(name).map(|k| (k, false));
    }
    SHIFTED
        .iter()
        .find(|(ch, _)| *ch == c)
        .and_then(|(_, name)| lookup(name))
        .map(|k| (k, true))
}

/// A mouse button that an action can press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chords_and_aliases() {
        let keys = parse_chord("ctrl + Shift+t").unwrap();
        let names: Vec<_> = keys.iter().map(|k| k.name).collect();
        assert_eq!(names, ["Ctrl", "Shift", "T"]);
        assert_eq!(lookup("Win").unwrap().name, "Super");
        assert!(parse_chord("Ctrl+Bogus").is_err());
    }

    #[test]
    fn maps_characters_to_keys() {
        assert_eq!(
            char_to_key('a').map(|(k, s)| (k.name, s)),
            Some(("A", false))
        );
        assert_eq!(
            char_to_key('Q').map(|(k, s)| (k.name, s)),
            Some(("Q", true))
        );
        assert_eq!(
            char_to_key('7').map(|(k, s)| (k.name, s)),
            Some(("7", false))
        );
        assert_eq!(
            char_to_key('@').map(|(k, s)| (k.name, s)),
            Some(("2", true))
        );
        assert_eq!(
            char_to_key(' ').map(|(k, s)| (k.name, s)),
            Some(("Space", false))
        );
        assert_eq!(char_to_key('\n').map(|(k, _)| k.name), Some("Enter"));
        assert!(char_to_key('é').is_none());
    }

    #[test]
    fn key_names_are_unique() {
        for (i, a) in KEYS.iter().enumerate() {
            assert!(
                KEYS[i + 1..]
                    .iter()
                    .all(|b| !b.name.eq_ignore_ascii_case(a.name)),
                "{}",
                a.name
            );
        }
    }
}
