//! Automatic updates from the project's GitHub releases.
//!
//! * **Checking** asks GitHub's public API for the latest release (one small
//!   request; nothing about the user or the devices is sent) and compares its
//!   version with this program's.
//! * **Installing** (Windows) downloads the new `logirev.exe` and
//!   `logirevctl.exe` published with the release, checks them against the
//!   release's `SHA256SUMS.txt`, and swaps them in place: a running `.exe`
//!   cannot be overwritten but can be renamed, so the current files become
//!   `*.exe.old` (deleted at the next start) and the new ones take their
//!   names. The caller then restarts logirev (`--after-update`).
//!
//! The service checks once a day (if `[general] check_updates` is on) and
//! shows a notification; the settings window checks when opened and offers
//! the "Update now" button.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use serde::Deserialize;

use crate::daemon::Event;
use crate::http;
use crate::sha256;

/// The executables a release replaces (without `.exe`).
const PROGRAMS: [&str; 2] = ["logirev", "logirevctl"];
/// First check after start, then once a day.
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the checker looks at the setting while waiting (it does not go
/// online unless [`enabled`] is true).
const POLL: Duration = Duration::from_secs(60);

/// Whether `[general] check_updates` is on. The daemon updates it when the
/// settings are reloaded, so turning the check off works without a restart.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turns the daily check on or off.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, AtomicOrdering::Relaxed);
}

/// Whether the daily check is on.
pub fn enabled() -> bool {
    ENABLED.load(AtomicOrdering::Relaxed)
}

/// A published release, as returned by the GitHub API.
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    /// e.g. `v1.0.1`.
    pub tag_name: String,
    /// The release page, for "what's new" and manual downloads.
    pub html_url: String,
    /// Release notes (Markdown).
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    /// Files attached to the release.
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
/// A file attached to a release.
pub struct Asset {
    /// File name.
    pub name: String,
    /// Direct download address.
    pub browser_download_url: String,
}

impl Release {
    /// The version without the leading `v`.
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// `https://api.github.com/repos/<owner>/<repo>/releases/latest`, from the
/// repository address in `Cargo.toml`.
fn latest_url() -> String {
    let repo = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let path = repo
        .strip_prefix("https://github.com/")
        .unwrap_or("wildzer0/logirev");
    format!("https://api.github.com/repos/{path}/releases/latest")
}

/// The newest published release (pre-releases excluded by GitHub).
pub fn latest() -> Result<Release, String> {
    let body = http::get(&latest_url())?;
    serde_json::from_slice(&body).map_err(|e| format!("unexpected answer from GitHub: {e}"))
}

/// The latest release if it is newer than this program.
pub fn check() -> Result<Option<Release>, String> {
    let release = latest()?;
    Ok(
        (compare_versions(release.version(), env!("CARGO_PKG_VERSION")) == Ordering::Greater)
            .then_some(release),
    )
}

/// Compares `1.2.3`, `1.2.3-rc.1`... (a pre-release is older than the
/// release with the same numbers).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn parse(v: &str) -> ([u64; 3], Option<&str>) {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v, None),
        };
        let mut nums = [0u64; 3];
        for (slot, part) in nums.iter_mut().zip(core.split('.')) {
            *slot = part.parse().unwrap_or(0);
        }
        (nums, pre)
    }
    let ((na, pa), (nb, pb)) = (parse(a), parse(b));
    na.cmp(&nb).then_with(|| match (pa, pb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(y),
    })
}

/// Whether this build can install updates by itself.
pub fn can_install() -> bool {
    cfg!(windows)
}

/// Name of a program's file inside a release.
fn asset_name(program: &str, tag: &str) -> String {
    format!("{program}-{tag}-windows-x64.exe")
}

/// `SHA256SUMS.txt` lines: `<hash>  <file name>`.
fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            Some((
                parts.next()?.to_lowercase(),
                parts.next()?.trim_start_matches('*').to_owned(),
            ))
        })
        .collect()
}

/// Downloads, verifies and installs `release` next to the running program.
pub fn install(release: &Release) -> Result<(), String> {
    if !can_install() {
        return Err("automatic installation is only available on Windows".into());
    }
    let dir = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(Path::to_path_buf)
        .ok_or("cannot find the program folder")?;

    let sums_asset = release
        .asset("SHA256SUMS.txt")
        .ok_or("the release has no SHA256SUMS.txt")?;
    let sums = parse_sums(&String::from_utf8_lossy(&http::get(
        &sums_asset.browser_download_url,
    )?));

    // Download and verify everything before touching any file.
    let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for program in PROGRAMS {
        let target = dir.join(format!("{program}.exe"));
        if program != "logirev" && !target.exists() {
            continue; // the user did not keep this tool
        }
        let name = asset_name(program, &release.tag_name);
        let asset = release
            .asset(&name)
            .ok_or_else(|| format!("the release has no {name}"))?;
        let data = http::get(&asset.browser_download_url)?;
        let expected = sums
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(h, _)| h.as_str())
            .ok_or_else(|| format!("{name} is not listed in SHA256SUMS.txt"))?;
        if sha256::hex_digest(&data) != expected {
            return Err(format!(
                "{name}: the download is damaged (checksum mismatch)"
            ));
        }
        files.push((target, data));
    }

    // Swap the files; undo on failure.
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (target, data) in &files {
        let old = old_path(target);
        let _ = std::fs::remove_file(&old);
        if target.exists() {
            if let Err(e) = std::fs::rename(target, &old) {
                rollback(&done);
                return Err(format!("cannot replace {}: {e}", target.display()));
            }
        }
        if let Err(e) = std::fs::write(target, data) {
            let _ = std::fs::rename(&old, target);
            rollback(&done);
            return Err(format!(
                "cannot write {}: {e} (is the folder read-only?)",
                target.display()
            ));
        }
        done.push((target.clone(), old));
    }
    info!("updated to {}", release.tag_name);
    Ok(())
}

fn old_path(target: &Path) -> PathBuf {
    target.with_extension("exe.old")
}

fn rollback(done: &[(PathBuf, PathBuf)]) {
    for (target, old) in done.iter().rev() {
        let _ = std::fs::remove_file(target);
        let _ = std::fs::rename(old, target);
    }
}

/// Deletes the `*.exe.old` files left by a previous update (they may still
/// be in use for a moment; then they are removed at the next start).
pub fn cleanup_old_files() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    else {
        return;
    };
    for program in PROGRAMS {
        let old = old_path(&dir.join(format!("{program}.exe")));
        if old.exists() && std::fs::remove_file(&old).is_ok() {
            debug!("removed {}", old.display());
        }
    }
}

/// Starts the background checker of the service: first check after a
/// minute, then daily; a newer version is reported as
/// [`Event::UpdateAvailable`].
pub fn spawn_checker(tx: Sender<Event>) {
    let spawned = std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let mut due = Instant::now() + FIRST_CHECK;
            loop {
                std::thread::sleep(POLL);
                if !enabled() {
                    // Check soon after the user turns it back on.
                    due = Instant::now() + FIRST_CHECK;
                    continue;
                }
                if Instant::now() < due {
                    continue;
                }
                match check() {
                    Ok(Some(release)) => {
                        if tx
                            .send(Event::UpdateAvailable(release.version().to_owned()))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Ok(None) => debug!("logirev is up to date"),
                    Err(e) => debug!("update check failed: {e}"),
                }
                due = Instant::now() + CHECK_EVERY;
            }
        });
    if let Err(e) = spawned {
        warn!("cannot start the update checker: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(compare_versions("1.0.1", "1.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("v1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.0.0", "1.0.0-rc.1"), Ordering::Greater);
        assert_eq!(
            compare_versions("1.0.0-rc.2", "1.0.0-rc.1"),
            Ordering::Greater
        );
        assert_eq!(compare_versions("0.9.9", "1.0.0"), Ordering::Less);
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Ordering::Greater);
    }

    /// Talks to the real GitHub API: `cargo test -- --ignored live`.
    #[test]
    #[ignore]
    fn live_latest_release() {
        let r = latest().expect("GitHub answers");
        assert!(r.tag_name.starts_with('v'), "{}", r.tag_name);
        assert!(r.assets.iter().any(|a| a.name == "SHA256SUMS.txt"));
    }

    #[test]
    fn parses_release_and_sums() {
        let json = r#"{"tag_name":"v1.2.0","html_url":"https://x","body":"notes",
            "assets":[{"name":"logirev-v1.2.0-windows-x64.exe","browser_download_url":"https://d/1"},
                      {"name":"SHA256SUMS.txt","browser_download_url":"https://d/2"}],
            "other_field": 5}"#;
        let r: Release = serde_json::from_str(json).unwrap();
        assert_eq!(r.version(), "1.2.0");
        assert!(r.asset(&asset_name("logirev", &r.tag_name)).is_some());
        let sums = parse_sums("ABCDEF  logirev-v1.2.0-windows-x64.exe\nbad\n0123 *x.zip\n");
        assert_eq!(
            sums,
            vec![
                (
                    "abcdef".to_owned(),
                    "logirev-v1.2.0-windows-x64.exe".to_owned()
                ),
                ("0123".to_owned(), "x.zip".to_owned()),
            ]
        );
        assert!(latest_url().ends_with("/repos/wildzer0/logirev/releases/latest"));
    }
}
