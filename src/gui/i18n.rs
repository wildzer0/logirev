//! Interface texts in English and Italian.
//!
//! Every text is a field of [`T`]; [`EN`] and [`IT`] are filled from one
//! table, so a missing translation is a compile error. Texts with a `{}`
//! placeholder are filled with [`fill`].

/// Interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    It,
}

impl Lang {
    /// From the `[general] language` setting (`auto` follows the system).
    pub fn from_setting(setting: &str) -> Self {
        match setting {
            "it" => Self::It,
            "en" => Self::En,
            _ => system_language(),
        }
    }

    pub fn texts(self) -> &'static T {
        match self {
            Self::En => &EN,
            Self::It => &IT,
        }
    }
}

/// The user's language as configured in the operating system.
fn system_language() -> Lang {
    #[cfg(windows)]
    {
        // SAFETY: plain query without arguments.
        let lang = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        // Primary language id 0x10 = Italian.
        if lang & 0x3FF == 0x10 {
            return Lang::It;
        }
        Lang::En
    }
    #[cfg(not(windows))]
    {
        let env = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
            .unwrap_or_default();
        if env.starts_with("it") {
            Lang::It
        } else {
            Lang::En
        }
    }
}

/// The localised name of a button, from its configuration alias.
pub fn control_label<'a>(t: &'a T, key: &str, fallback: &'a str) -> &'a str {
    match key {
        "left" => t.ctl_left,
        "right" => t.ctl_right,
        "middle" => t.ctl_middle,
        "back" => t.ctl_back,
        "forward" => t.ctl_forward,
        "tilt_left" => t.ctl_tilt_left,
        "tilt_right" => t.ctl_tilt_right,
        "gesture" => t.ctl_gesture,
        "smartshift" => t.ctl_smartshift,
        "virtual_gesture" => t.ctl_virtual_gesture,
        "precision" | "dpi_switch" => t.ctl_precision,
        _ => fallback,
    }
}

/// Replaces each `{}` in `text` with the next value.
pub fn fill(text: &str, values: &[&str]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    for v in values {
        match rest.find("{}") {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push_str(v);
                rest = &rest[i + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

macro_rules! texts {
    ($($key:ident => $en:expr, $it:expr;)*) => {
        /// All interface texts of one language. Some are only used on
        /// Windows (file picker, start with Windows).
        #[cfg_attr(not(windows), allow(dead_code))]
        pub struct T { $(pub $key: &'static str,)* }
        pub static EN: T = T { $($key: $en,)* };
        pub static IT: T = T { $($key: $it,)* };
    };
}

texts! {
    // Navigation
    nav_devices => "Devices", "Dispositivi";
    nav_macros => "Macros", "Macro";
    nav_settings => "Settings", "Impostazioni";
    nav_about => "About", "Informazioni";

    // Service state
    service_running => "Service running", "Servizio attivo";
    service_stopped => "Service not running", "Servizio non attivo";
    service_start => "Start", "Avvia";
    service_stopped_hint => "logirev is not running, so devices are not configured and nothing is shown here. Start it to continue.",
        "logirev non è in esecuzione: le periferiche non vengono configurate e qui non compare nulla. Avvialo per continuare.";

    // Devices page
    devices_title => "Your devices", "Le tue periferiche";
    devices_subtitle => "Click a connected device to customise it. Changes are saved and applied immediately.",
        "Clicca una periferica connessa per personalizzarla. Le modifiche vengono salvate e applicate subito.";
    no_receiver_title => "No receiver found", "Nessun ricevitore trovato";
    no_receiver_hint => "Plug in your Logitech Bolt or Unifying receiver. If it is already plugged in, close Logi Options+ and try again.",
        "Collega il ricevitore Logitech Bolt o Unifying. Se è già collegato, chiudi Logi Options+ e riprova.";
    no_devices => "No device paired with this receiver.", "Nessuna periferica associata a questo ricevitore.";
    connected => "Connected", "Connesso";
    asleep => "Not connected", "Non connesso";
    asleep_hint => "Move it or press a key to wake it up.", "Muovila o premi un tasto per svegliarla.";
    slot => "Slot {}", "Slot {}";
    direct => "Direct connection", "Collegamento diretto";
    buttons_remapped => "{} buttons customised", "{} tasti personalizzati";

    // Device page
    back => "Devices", "Periferiche";
    tab_buttons => "Buttons", "Tasti";
    tab_pointer => "Pointer & wheel", "Puntatore e rotella";
    tab_keyboard => "Keyboard", "Tastiera";
    tab_programs => "Per program", "Per programma";
    tab_computers => "Computers", "Computer";
    firmware => "Firmware {}", "Firmware {}";
    battery => "Battery", "Batteria";
    device_gone => "This device is no longer connected.", "Questa periferica non è più connessa.";

    // Button names
    ctl_left => "Left click", "Clic sinistro";
    ctl_right => "Right click", "Clic destro";
    ctl_middle => "Middle button (wheel click)", "Tasto centrale (clic della rotella)";
    ctl_back => "Back", "Indietro";
    ctl_forward => "Forward", "Avanti";
    ctl_tilt_left => "Wheel tilt left", "Rotella inclinata a sinistra";
    ctl_tilt_right => "Wheel tilt right", "Rotella inclinata a destra";
    ctl_gesture => "Gesture button", "Tasto gesture";
    ctl_smartshift => "Wheel mode button", "Tasto modalità rotella";
    ctl_virtual_gesture => "Virtual gesture button", "Tasto gesture virtuale";
    ctl_precision => "Precision mode button", "Tasto modalità precisione";

    // Buttons tab
    buttons_hint => "Choose what each button does. \"Default\" keeps the button's normal behaviour.",
        "Scegli cosa fa ogni tasto. \"Predefinito\" mantiene il comportamento normale del tasto.";
    change => "Change", "Modifica";
    not_remappable => "Cannot be changed", "Non modificabile";
    gestures_supported => "supports gestures", "supporta le gesture";

    // Pointer tab
    speed_title => "Speed", "Velocità";
    dpi => "Sensitivity (DPI)", "Sensibilità (DPI)";
    keep_device_value => "Keep the device's own value", "Mantieni il valore della periferica";
    pointer_speed => "Pointer speed", "Velocità del puntatore";
    wheel_title => "Scroll wheel", "Rotella";
    hires_wheel => "High-resolution scrolling", "Scorrimento ad alta risoluzione";
    hires_hint => "Smoother scrolling. Without Logi Options+ some programs may scroll very fast.",
        "Scorrimento più fluido. Senza Logi Options+ alcuni programmi potrebbero scorrere molto velocemente.";
    invert_wheel => "Reverse scroll direction", "Inverti la direzione di scorrimento";
    smartshift => "SmartShift", "SmartShift";
    smartshift_smart => "Smart (frees itself when scrolling fast)", "Intelligente (si sblocca scorrendo veloce)";
    smartshift_ratchet => "Always clicking", "Sempre a scatti";
    smartshift_free => "Always free-spinning", "Sempre a rotazione libera";
    smartshift_threshold => "Sensitivity", "Sensibilità";
    not_set => "Leave as it is", "Lascia com'è";
    thumb_wheel_invert => "Reverse the thumb wheel", "Inverti la rotella laterale";
    report_rate => "Report rate", "Frequenza di aggiornamento";

    // Keyboard tab
    fn_keys => "F1–F12 keys", "Tasti F1–F12";
    fn_standard => "F1, F2... (media functions with Fn)", "F1, F2... (funzioni multimediali con Fn)";
    fn_special => "Media functions (F1, F2... with Fn)", "Funzioni multimediali (F1, F2... con Fn)";
    disable_keys => "Disable keys", "Disattiva tasti";
    backlight => "Backlight", "Retroilluminazione";
    backlight_off => "Off", "Spenta";
    backlight_on => "On", "Accesa";
    backlight_auto => "Automatic (follows the room light)", "Automatica (segue la luce ambiente)";
    backlight_manual => "Fixed level", "Livello fisso";
    level => "Level", "Livello";

    // Programs tab
    programs_hint => "Give buttons a different job while a program is in front. The other buttons keep their normal settings.",
        "Assegna ai tasti un compito diverso quando un programma è in primo piano. Gli altri tasti mantengono le impostazioni normali.";
    programs_windows_only => "Per-program buttons are available on Windows.", "I tasti per programma sono disponibili su Windows.";
    add_program => "Add program", "Aggiungi programma";
    program_name => "Program file, e.g. chrome.exe", "File del programma, es. chrome.exe";
    running_programs => "Running programs", "Programmi aperti";
    remove => "Remove", "Rimuovi";
    when_in => "When using {}", "Quando usi {}";
    same_as_usual => "As usual", "Come al solito";

    // Computers tab
    computers_hint => "Easy-Switch: send this device to another computer it is paired with.",
        "Easy-Switch: invia questa periferica a un altro computer con cui è associata.";
    computer_current => "In use", "In uso";
    send_here => "Switch", "Passa";

    // Action editor
    edit_button => "Button: {}", "Tasto: {}";
    action => "Action", "Azione";
    save => "Save", "Salva";
    cancel => "Cancel", "Annulla";
    act_default => "Default", "Predefinito";
    act_disable => "Do nothing", "Nessuna azione";
    act_keys => "Keyboard shortcut", "Scorciatoia da tastiera";
    act_click => "Mouse button", "Tasto del mouse";
    act_scroll => "Scroll", "Scorri";
    act_command => "Open a program", "Apri un programma";
    act_slow => "Precision mode (lower DPI)", "Modalità precisione (DPI più bassi)";
    act_cycle => "Change DPI at each press", "Cambia DPI a ogni pressione";
    act_host => "Switch computer", "Cambia computer";
    act_brightness => "Screen brightness", "Luminosità dello schermo";
    act_backlight => "Keyboard backlight", "Retroilluminazione tastiera";
    act_macro => "Macro", "Macro";
    act_gesture => "Gestures (hold and move)", "Gesture (tieni premuto e muovi)";
    act_custom => "Custom (set in the file)", "Personalizzata (nel file)";
    custom_hint => "This button uses an action written by hand in the settings file. Choose another action to replace it.",
        "Questo tasto usa un'azione scritta a mano nel file delle impostazioni. Scegli un'altra azione per sostituirla.";
    record => "Record", "Registra";
    recording => "Press the keys...", "Premi i tasti...";
    key => "Key", "Tasto";
    shortcut_hint => "Click Record and press the shortcut, or pick the keys below.",
        "Clicca Registra e premi la scorciatoia, oppure scegli i tasti qui sotto.";
    button_left => "Left click", "Clic sinistro";
    button_right => "Right click", "Clic destro";
    button_middle => "Middle click", "Clic centrale";
    button_back => "Back", "Indietro";
    button_forward => "Forward", "Avanti";
    dir_up => "Up", "Su";
    dir_down => "Down", "Giù";
    dir_left => "Left", "Sinistra";
    dir_right => "Right", "Destra";
    amount => "Amount", "Quantità";
    program => "Program", "Programma";
    arguments => "Arguments (optional)", "Argomenti (facoltativi)";
    browse => "Browse...", "Sfoglia...";
    slow_dpi => "DPI while active", "DPI mentre è attiva";
    cycle_values => "DPI values", "Valori DPI";
    add_value => "Add value", "Aggiungi valore";
    host_next => "Next computer", "Computer successivo";
    host_n => "Computer {}", "Computer {}";
    host_all => "Switch all my devices together", "Sposta insieme tutte le periferiche";
    brightness_change => "Change by", "Cambia di";
    brightness_set => "Set to", "Imposta a";
    backlight_up => "Brighter", "Più luminosa";
    backlight_down => "Darker", "Meno luminosa";
    backlight_toggle => "On / off", "Accendi / spegni";
    no_macros => "No macro yet: create one in the Macros page.", "Nessuna macro: creane una nella pagina Macro.";
    repeat_held => "Repeat while the button is held", "Ripeti finché il tasto è premuto";
    every_ms => "Every (ms)", "Ogni (ms)";
    gesture_click => "Press without moving", "Pressione senza movimento";
    gesture_no_movement => "This button cannot follow movement: only the \"press without moving\" action will work.",
        "Questo tasto non può seguire il movimento: funzionerà solo l'azione \"pressione senza movimento\".";

    // Macros page
    macros_title => "Macros", "Macro";
    macros_hint => "A macro is a list of steps run one after the other. Assign it to a button with the \"Macro\" action.",
        "Una macro è una lista di passi eseguiti uno dopo l'altro. Assegnala a un tasto con l'azione \"Macro\".";
    new_macro => "New macro", "Nuova macro";
    macro_name => "Name", "Nome";
    add_step => "Add step", "Aggiungi passo";
    step_keys => "Press shortcut", "Premi scorciatoia";
    step_press => "Hold keys", "Tieni premuti tasti";
    step_release => "Release keys", "Rilascia tasti";
    step_text => "Type text", "Scrivi testo";
    step_click => "Click", "Clic";
    step_delay => "Wait (ms)", "Attendi (ms)";
    delete => "Delete", "Elimina";

    // Settings page
    settings_title => "Settings", "Impostazioni";
    language => "Language", "Lingua";
    lang_auto => "Automatic", "Automatica";
    theme => "Appearance", "Aspetto";
    theme_system => "System", "Sistema";
    theme_light => "Light", "Chiaro";
    theme_dark => "Dark", "Scuro";
    start_with_windows => "Start with Windows", "Avvia con Windows";
    battery_warning => "Warn when the battery drops to", "Avvisa quando la batteria scende al";
    off => "Off", "Disattivato";
    log_level => "Log detail", "Dettaglio del log";
    files => "Files", "File";
    open_log => "Open log", "Apri log";
    open_settings_file => "Open settings file", "Apri file impostazioni";
    open_folder => "Open folder", "Apri cartella";
    service => "Service", "Servizio";
    restart_service => "Restart service", "Riavvia servizio";
    stop_service => "Stop service", "Arresta servizio";

    // Saving
    saved => "Saved", "Salvato";
    save_error => "Not saved: {}", "Non salvato: {}";

    // Updates
    updates => "Updates", "Aggiornamenti";
    check_updates_auto => "Check for updates automatically (asks GitHub once a day)",
        "Controlla automaticamente gli aggiornamenti (chiede a GitHub una volta al giorno)";
    current_version => "Installed version: {}", "Versione installata: {}";
    check_now => "Check now", "Controlla ora";
    checking => "Checking...", "Controllo in corso...";
    up_to_date => "logirev is up to date.", "logirev è aggiornato.";
    update_available => "Version {} is available.", "È disponibile la versione {}.";
    update_now => "Update now", "Aggiorna ora";
    update_later => "Later", "Più tardi";
    updating => "Downloading and installing...", "Download e installazione in corso...";
    update_failed => "Update failed: {}", "Aggiornamento non riuscito: {}";
    whats_new => "What's new", "Novità";
    download_page => "Open the download page", "Apri la pagina di download";
    restarting => "Updated. Restarting logirev...", "Aggiornato. Riavvio di logirev...";

    // About page
    about_text => "A small, portable app that configures Logitech mice, trackballs and keyboards — without Logi Options+.",
        "Una piccola app portatile che configura mouse, trackball e tastiere Logitech, senza Logi Options+.";
    license_text => "Free for personal and non-commercial use (PolyForm Noncommercial 1.0.0).",
        "Gratuita per uso personale e non commerciale (PolyForm Noncommercial 1.0.0).";
    not_affiliated => "Not affiliated with Logitech. Icons: Phosphor Icons (MIT).",
        "Non affiliata a Logitech. Icone: Phosphor Icons (MIT).";
    website => "Project page", "Pagina del progetto";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders() {
        assert_eq!(fill("Slot {}", &["2"]), "Slot 2");
        assert_eq!(fill("{} of {}", &["1", "3"]), "1 of 3");
        assert_eq!(fill("none", &["x"]), "none");
    }

    #[test]
    fn languages_differ() {
        assert_ne!(EN.nav_devices, IT.nav_devices);
        assert_eq!(Lang::from_setting("it").texts().nav_devices, "Dispositivi");
    }
}
