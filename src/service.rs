//! Start-at-login integration.
//!
//! * **Linux** — a *systemd user service*: a small text file in
//!   `~/.config/systemd/user/` that tells the session manager to start
//!   `logirev` at log-in and restart it if it ever crashes.
//! * **Windows** — handled by the tray icon ("Start with Windows").

use std::path::{Path, PathBuf};

/// Name of the systemd unit.
pub const UNIT_NAME: &str = "logirev.service";

/// Text of the systemd unit that runs `exe` with `config`.
pub fn systemd_unit(exe: &Path, config: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=logirev - Logitech device configuration\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         ExecStart=\"{}\" --config \"{}\"\n\
         Restart=on-failure\n\
         RestartSec=3\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe.display(),
        config.display()
    )
}

/// Where the user's systemd units live (`$XDG_CONFIG_HOME/systemd/user`).
pub fn systemd_user_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("systemd").join("user"))
}

/// Writes the unit file and returns its path.
pub fn install_systemd_unit(exe: &Path, config: &Path) -> Result<PathBuf, String> {
    let dir = systemd_user_dir().ok_or("cannot find the home directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join(UNIT_NAME);
    std::fs::write(&path, systemd_unit(exe, config))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_points_at_binary_and_config() {
        let unit = systemd_unit(
            Path::new("/opt/logirev/logirev"),
            Path::new("/opt/logirev/logirev.toml"),
        );
        assert!(unit
            .contains("ExecStart=\"/opt/logirev/logirev\" --config \"/opt/logirev/logirev.toml\""));
        assert!(unit.contains("WantedBy=default.target"));
    }
}
