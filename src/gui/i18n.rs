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

/// The localised name of button `cid` ("Tasto 0x0123" when unknown).
pub fn control_label(t: &T, cid: u16) -> String {
    crate::hidpp::controls::name_or_number(cid, std::ptr::eq(t, &IT))
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

    // Pairing
    pair_new => "Pair a new device", "Associa nuova periferica";
    pairing_title => "Pair a new device", "Associa una nuova periferica";
    pairing_unifying_hint => "Turn the device on, or press its connect button. The receiver is listening for 30 seconds.",
        "Accendi la periferica o premi il suo tasto di connessione. Il ricevitore resta in ascolto per 30 secondi.";
    pairing_bolt_hint => "Press and hold the device's connect (Easy-Switch) button until its light blinks fast.",
        "Tieni premuto il tasto di connessione (Easy-Switch) della periferica finché la luce lampeggia velocemente.";
    searching => "Searching for devices...", "Ricerca delle periferiche...";
    pair_this => "Pair", "Associa";
    pairing_in_progress => "Pairing...", "Associazione in corso...";
    passkey_keyboard => "On the keyboard, type this code and then press Enter:",
        "Sulla tastiera, digita questo codice e poi premi Invio:";
    passkey_mouse => "Click the mouse buttons in this order:", "Premi i tasti del mouse in quest'ordine:";
    passkey_mouse_finish => "Then press the left and right buttons together.", "Poi premi insieme il tasto sinistro e quello destro.";
    left_short => "Left", "Sinistro";
    right_short => "Right", "Destro";
    paired_ok => "Done! The device is paired (slot {}).", "Fatto! La periferica è associata (slot {}).";
    pairing_failed => "The pairing did not work: {}", "L'associazione non è riuscita: {}";
    pairing_cancelled => "Pairing stopped.", "Associazione interrotta.";
    try_again => "Try again", "Riprova";
    close => "Close", "Chiudi";
    unpair => "Remove pairing", "Rimuovi associazione";
    unpair_confirm => "Remove {} from the receiver?", "Rimuovere {} dal ricevitore?";
    unpair_hint => "It stops working with this receiver until you pair it again (you can do it at any time).",
        "Smetterà di funzionare con questo ricevitore finché non la associ di nuovo (puoi farlo in qualsiasi momento).";

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

    // Buttons tab
    buttons_hint => "Choose what each button does. \"Default\" keeps the button's normal behaviour.",
        "Scegli cosa fa ogni tasto. \"Predefinito\" mantiene il comportamento normale del tasto.";
    change => "Change", "Modifica";
    not_remappable => "Cannot be changed", "Non modificabile";
    gestures_supported => "supports gestures", "supporta le gesture";

    // Pointer tab
    speed_title => "Speed", "Velocità";
    dpi => "Sensitivity (DPI)", "Sensibilità (DPI)";
    keep_device_value => "Device's own value", "Valore della periferica";
    choose_value => "Choose a value", "Scegli un valore";
    dpi_hint => "How far the pointer moves for the same hand movement. Lower = slower and more precise, higher = faster.",
        "Quanto si sposta il puntatore a parità di movimento della mano. Più basso = più lento e preciso, più alto = più veloce.";
    device_value_now => "logirev does not change it. Now: {}", "logirev non lo modifica. Ora: {}";
    device_value_unknown => "logirev does not change it.", "logirev non lo modifica.";
    turn_on => "On", "Attivo";
    turn_off => "Off", "Disattivo";
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
    not_set => "Don't change", "Non modificare";
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
    current_action => "Now: {}", "Ora: {}";
    tab_tech => "Technical", "Tecnico";
    developer => "Developer mode", "Modalità sviluppatore";
    developer_on => "Show technical details", "Mostra i dettagli tecnici";
    developer_hint => "Adds a \"Technical\" tab to each device: identity, firmware components, every feature (hidden ones included) and the live HID++ messages. Everything is read-only.",
        "Aggiunge a ogni periferica una scheda \"Tecnico\": identità, componenti del firmware, tutte le funzioni (anche quelle nascoste) e i messaggi HID++ in tempo reale. È tutto in sola lettura.";
    tech_hint => "Read-only information reported by the device. Useful for bug reports.",
        "Informazioni in sola lettura fornite dalla periferica. Utili per segnalare un problema.";
    tech_loading => "Reading the technical details...", "Lettura dei dettagli tecnici...";
    tech_identity => "Identity", "Identità";
    tech_serial => "Serial number", "Numero di serie";
    tech_unit_id => "Unit id", "ID dell'unità";
    tech_model_ids => "Model ids", "ID del modello";
    tech_transports => "Connections", "Connessioni";
    tech_slot => "Receiver slot", "Slot del ricevitore";
    tech_receiver => "Receiver", "Ricevitore";
    tech_firmware => "Firmware components", "Componenti del firmware";
    tech_running => "running", "in esecuzione";
    tech_features => "Features ({})", "Funzioni ({})";
    tech_features_hint => "\"hidden\" and \"engineering\" features are meant for Logitech's own tools: logirev only lists them.",
        "Le funzioni \"hidden\" e \"engineering\" sono per gli strumenti di Logitech: logirev si limita a elencarle.";
    tech_traffic => "HID++ messages", "Messaggi HID++";
    tech_traffic_hint => "The latest messages between logirev and the receiver. {} sent, {} received.",
        "Gli ultimi messaggi tra logirev e il ricevitore. {} inviati, {} ricevuti.";
    tech_only_device => "Only this device", "Solo questa periferica";
    tech_pause => "Pause", "Pausa";
    tech_resume => "Resume", "Riprendi";
    tech_no_traffic => "No messages yet: press a remapped button or change a setting.",
        "Ancora nessun messaggio: premi un tasto riassegnato o cambia un'impostazione.";
    tech_copy => "Copy report", "Copia rapporto";
    tech_save => "Save report...", "Salva rapporto...";
    tech_copied => "Copied to the clipboard", "Copiato negli appunti";
    reset_default => "Restore default", "Ripristina predefinito";
    action_details => "Settings of this action", "Impostazioni di questa azione";
    grp_basic => "Mouse", "Mouse";
    grp_edit => "Text editing", "Modifica del testo";
    grp_system => "Windows", "Windows";
    grp_browser => "Browser", "Browser";
    grp_media => "Music and volume", "Musica e volume";
    grp_more => "More actions", "Altre azioni";
    p_back => "Back", "Indietro";
    p_forward => "Forward", "Avanti";
    p_middle => "Middle click", "Clic centrale";
    p_precision => "Precision mode", "Modalità precisione";
    p_scroll_left => "Scroll left", "Scorri a sinistra";
    p_scroll_right => "Scroll right", "Scorri a destra";
    p_copy => "Copy", "Copia";
    p_cut => "Cut", "Taglia";
    p_paste => "Paste", "Incolla";
    p_undo => "Undo", "Annulla";
    p_redo => "Redo", "Ripeti";
    p_select_all => "Select all", "Seleziona tutto";
    p_task_view => "Task view", "Visualizzazione attività";
    p_desktop => "Show desktop", "Mostra desktop";
    p_switch_app => "Switch window", "Cambia finestra";
    p_lock => "Lock the PC", "Blocca il PC";
    p_screenshot => "Screenshot", "Cattura schermo";
    p_zoom_in => "Zoom in", "Ingrandisci";
    p_zoom_out => "Zoom out", "Riduci";
    p_new_tab => "New tab", "Nuova scheda";
    p_close_tab => "Close tab", "Chiudi scheda";
    p_next_tab => "Next tab", "Scheda successiva";
    p_prev_tab => "Previous tab", "Scheda precedente";
    p_refresh => "Reload page", "Ricarica pagina";
    p_play => "Play / Pause", "Riproduci / Pausa";
    p_next_track => "Next track", "Brano successivo";
    p_prev_track => "Previous track", "Brano precedente";
    p_vol_up => "Volume up", "Alza volume";
    p_vol_down => "Volume down", "Abbassa volume";
    p_mute => "Mute", "Disattiva audio";
    adv_keys => "Other shortcut", "Altra scorciatoia";
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
    saved => "Settings applied", "Impostazioni applicate";
    applying => "Applying settings...", "Applico le impostazioni...";
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
