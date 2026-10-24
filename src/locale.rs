//! Which language to talk to the user in (English or Italian).
//!
//! Used by the settings window and by the service for its notifications and
//! on-screen messages.

/// Whether to use Italian, from the `[general] language` setting
/// (`it`, `en`, or `auto` to follow the operating system).
pub fn italian(setting: &str) -> bool {
    match setting {
        "it" => true,
        "en" => false,
        _ => system_is_italian(),
    }
}

/// Picks the English or the Italian text.
pub fn pick<'a>(italian: bool, en: &'a str, it: &'a str) -> &'a str {
    if italian {
        it
    } else {
        en
    }
}

/// The user's language as configured in the operating system.
fn system_is_italian() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: plain query without arguments.
        let lang = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        // Primary language id 0x10 = Italian.
        lang & 0x3FF == 0x10
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
            .unwrap_or_default()
            .starts_with("it")
    }
}
