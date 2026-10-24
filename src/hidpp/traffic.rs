//! The most recent HID++ messages of a receiver, for the developer mode.
//!
//! Recording is off by default and costs nothing then; when the developer
//! mode is on, the last [`CAPACITY`] messages in both directions are kept in
//! memory (a few kilobytes) and shown in the settings window.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How many messages are kept.
pub const CAPACITY: usize = 200;

/// One recorded message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficEntry {
    /// Increasing number of the message.
    pub seq: u64,
    /// Time, in milliseconds since 1970 (UTC).
    pub time_ms: u64,
    /// Sent by logirev (true) or by the hardware (false).
    pub outgoing: bool,
    /// The whole report, report id included.
    pub bytes: Vec<u8>,
}

/// A shared, size-limited record of recent messages.
#[derive(Debug, Default)]
pub struct TrafficLog {
    enabled: AtomicBool,
    seq: AtomicU64,
    entries: Mutex<VecDeque<TrafficEntry>>,
}

impl TrafficLog {
    /// Starts or stops recording; stopping forgets what was recorded.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        if !on {
            if let Ok(mut e) = self.entries.lock() {
                e.clear();
            }
        }
    }

    /// Records one message (does nothing while recording is off).
    pub fn record(&self, outgoing: bool, bytes: &[u8]) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        if let Ok(mut e) = self.entries.lock() {
            if e.len() == CAPACITY {
                e.pop_front();
            }
            e.push_back(TrafficEntry {
                seq,
                time_ms,
                outgoing,
                bytes: bytes.to_vec(),
            });
        }
    }

    /// Number of the latest message (changes whenever one is recorded).
    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::Relaxed)
    }

    /// A copy of the recorded messages, oldest first.
    pub fn recent(&self) -> Vec<TrafficEntry> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_latest_while_enabled() {
        let log = TrafficLog::default();
        log.record(true, &[1]);
        assert!(log.recent().is_empty());
        log.set_enabled(true);
        for i in 0..(CAPACITY + 5) {
            log.record(i % 2 == 0, &[i as u8]);
        }
        let r = log.recent();
        assert_eq!(r.len(), CAPACITY);
        assert_eq!(r[0].bytes, [5]);
        assert_eq!(log.last_seq(), (CAPACITY + 5) as u64);
        log.set_enabled(false);
        assert!(log.recent().is_empty());
    }
}
