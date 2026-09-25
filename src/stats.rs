//! Local chat statistics: how many partners, how much time, and so on.
//! Nothing here ever leaves your machine.

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    /// Partners you were matched with (and didn't auto-skip).
    pub partners: u64,
    /// Partners skipped by your auto-skip rules.
    pub skipped: u64,
    pub sent: u64,
    pub received: u64,
    /// Chats that have ended, and their total and longest length.
    pub chats: u64,
    pub chat_secs: u64,
    pub longest_secs: u64,
    pub since: Option<DateTime<Local>>,
}

impl Stats {
    pub fn starting(now: DateTime<Local>) -> Self {
        Stats { since: Some(now), ..Stats::default() }
    }

    /// A finished chat of `secs` seconds.
    pub fn chat_ended(&mut self, secs: u64) {
        self.chats += 1;
        self.chat_secs += secs;
        self.longest_secs = self.longest_secs.max(secs);
    }

    pub fn average_secs(&self) -> u64 {
        self.chat_secs.checked_div(self.chats).unwrap_or(0)
    }

    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(src) => toml::from_str(&src).with_context(|| format!("{} is not a valid stats file", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Stats::starting(Local::now())),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::config::write_atomic(path, &toml::to_string_pretty(self)?)
    }
}

/// `42s`, `7m`, `3h 12m`, `2d 4h`.
pub fn human_duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60);
    match (d, h, m) {
        (0, 0, 0) => format!("{secs}s"),
        (0, 0, m) => format!("{m}m"),
        (0, h, 0) => format!("{h}h"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, 0, _) => format!("{d}d"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(human_duration(0), "0s");
        assert_eq!(human_duration(42), "42s");
        assert_eq!(human_duration(7 * 60 + 5), "7m");
        assert_eq!(human_duration(3 * 3600), "3h");
        assert_eq!(human_duration(3 * 3600 + 12 * 60), "3h 12m");
        assert_eq!(human_duration(2 * 86_400 + 4 * 3600 + 59), "2d 4h");
        assert_eq!(human_duration(86_400), "1d");
    }

    #[test]
    fn chat_lengths_accumulate() {
        let mut s = Stats::default();
        assert_eq!(s.average_secs(), 0);
        s.chat_ended(60);
        s.chat_ended(180);
        assert_eq!((s.chats, s.chat_secs, s.longest_secs, s.average_secs()), (2, 240, 180, 120));
    }

    #[test]
    fn persists_and_starts_fresh_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stats.toml");
        let fresh = Stats::load(&path).unwrap();
        assert!(fresh.since.is_some());
        let mut s = fresh.clone();
        s.partners = 5;
        s.save(&path).unwrap();
        assert_eq!(Stats::load(&path).unwrap(), s);
        std::fs::write(&path, "partners = \"many\"").unwrap();
        assert!(Stats::load(&path).is_err());
    }
}
