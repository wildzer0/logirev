//! Logging: console output plus a size-capped log file.
//!
//! Each line looks like:
//!
//! ```text
//! 2026-10-24T18:03:12.417Z INFO  daemon: slot 1: "MX Ergo S" connected, applying profile "trackball"
//! ```
//!
//! Timestamps are UTC (the trailing `Z`) so that logs from different machines
//! line up. When the file reaches its maximum size (1 MiB by default) it is
//! renamed to `<name>.1.log`, replacing any previous backup, and a new file is
//! started: the current log therefore never exceeds the limit.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

use crate::pcap::backup_path;

/// Installs the global logger.
///
/// * `level` — the most verbose level written anywhere.
/// * `file` — optional log file path and its maximum size in bytes.
/// * `console` — whether lines are also printed on standard error.
pub fn init(level: LevelFilter, file: Option<(&Path, u64)>, console: bool) -> io::Result<()> {
    let file = match file {
        Some((path, max)) => Some(RotatingFile::open(path, max)?),
        None => None,
    };
    let logger = Logger {
        console,
        file: file.map(Mutex::new),
    };
    log::set_boxed_logger(Box::new(logger)).map_err(|e| io::Error::other(e.to_string()))?;
    log::set_max_level(level);
    Ok(())
}

/// The level is not stored here: `log::max_level()` is the single source of
/// truth, so a configuration reload can change it at runtime.
struct Logger {
    console: bool,
    file: Option<Mutex<RotatingFile>>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Strip the crate prefix: "logirev::daemon" -> "daemon".
        let target = record.target();
        let module = target.strip_prefix("logirev::").unwrap_or(target);
        let line = format!(
            "{} {:<5} {}: {}\n",
            utc_timestamp(SystemTime::now()),
            record.level(),
            module,
            record.args()
        );
        if self.console {
            let _ = io::stderr().write_all(line.as_bytes());
        }
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                // A failing log write must never take the daemon down.
                let _ = f.write_line(line.as_bytes());
            }
        }
    }

    fn flush(&self) {
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = f.file.flush();
            }
        }
    }
}

/// An append-only file that rotates itself before exceeding `max_size`.
struct RotatingFile {
    path: PathBuf,
    max_size: u64,
    file: File,
    size: u64,
}

impl RotatingFile {
    fn open(path: &Path, max_size: u64) -> io::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let size = file.metadata()?.len();
        let mut this = Self {
            path: path.to_path_buf(),
            max_size: max_size.max(4096),
            file,
            size,
        };
        if this.size >= this.max_size {
            this.rotate()?;
        }
        Ok(this)
    }

    fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        if self.size + line.len() as u64 > self.max_size {
            self.rotate()?;
        }
        self.file.write_all(line)?;
        self.size += line.len() as u64;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let backup = backup_path(&self.path, "log");
        let _ = fs::remove_file(&backup);
        fs::rename(&self.path, &backup)?;
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.size = 0;
        Ok(())
    }
}

/// Formats a time as ISO-8601 UTC with millisecond precision.
///
/// Implemented by hand (days-from-civil algorithm by Howard Hinnant) to avoid
/// pulling a date/time dependency for a single format string.
fn utc_timestamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_known_instants() {
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let t = UNIX_EPOCH + Duration::from_millis(1_709_164_800_250); // leap day
        assert_eq!(utc_timestamp(t), "2024-02-29T00:00:00.250Z");
    }

    #[test]
    fn file_never_exceeds_limit() {
        let dir = std::env::temp_dir().join(format!("logirev-log-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.log");
        let mut f = RotatingFile::open(&path, 4096).unwrap();
        for _ in 0..500 {
            f.write_line(b"0123456789012345678901234567890123456789\n")
                .unwrap();
        }
        assert!(fs::metadata(&path).unwrap().len() <= 4096);
        assert!(backup_path(&path, "log").exists());
        fs::remove_dir_all(&dir).ok();
    }
}
