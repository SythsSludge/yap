//! When to look for partners: how busy the site is and how long searches take, by
//! hour of the week. Kept on this machine only.

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use std::path::Path;

const HOURS: usize = 7 * 24;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    /// Per hour of the week (Monday 00:00 first): searches that found someone you
    /// kept, and the total seconds they took.
    pub searches: Vec<[u64; 2]>,
    /// Per hour of the week: samples of the online count, and their sum.
    pub online: Vec<[u64; 2]>,
    /// The minute of the last online sample, so samples come at most once a minute.
    #[serde(skip)]
    last_sample: Option<i64>,
}

impl Default for Activity {
    fn default() -> Self {
        Activity { searches: vec![[0, 0]; HOURS], online: vec![[0, 0]; HOURS], last_sample: None }
    }
}

fn bucket(at: DateTime<Local>) -> usize {
    at.weekday().num_days_from_monday() as usize * 24 + at.hour() as usize
}

/// A best time to look: the hour, the average wait, and how many searches that's from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quick {
    pub hour: u32,
    pub wait_secs: u64,
    pub searches: u64,
}

impl Activity {
    pub fn load(path: &Path) -> Result<Self> {
        let mut a: Activity = match std::fs::read_to_string(path) {
            Ok(src) => serde_json::from_str(&src).with_context(|| format!("{} is not valid", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Activity::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        a.searches.resize(HOURS, [0, 0]);
        a.online.resize(HOURS, [0, 0]);
        Ok(a)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::config::write_atomic(path, &serde_json::to_string(self)?)
    }

    /// A search that ended in a match you kept, after `secs`.
    pub fn searched(&mut self, at: DateTime<Local>, secs: u64) {
        let b = &mut self.searches[bucket(at)];
        b[0] += 1;
        b[1] += secs;
    }

    /// The server's online count. Returns false if it was too soon after the last one.
    pub fn online(&mut self, at: DateTime<Local>, count: u64) -> bool {
        let minute = at.timestamp() / 60;
        if self.last_sample == Some(minute) {
            return false;
        }
        self.last_sample = Some(minute);
        let b = &mut self.online[bucket(at)];
        b[0] += 1;
        b[1] += count;
        true
    }

    /// Average online count for each hour of the day, across the week.
    pub fn online_by_hour(&self) -> [Option<u64>; 24] {
        let mut out = [None; 24];
        for (hour, slot) in out.iter_mut().enumerate() {
            let (n, sum) = (0..7).map(|d| self.online[d * 24 + hour]).fold((0, 0), |a, b| (a.0 + b[0], a.1 + b[1]));
            *slot = sum.checked_div(n);
        }
        out
    }

    /// The hours of the day with the shortest average wait, from at least `min` searches.
    pub fn quickest(&self, min: u64, count: usize) -> Vec<Quick> {
        let mut hours: Vec<Quick> = (0..24)
            .filter_map(|hour| {
                let (n, secs) =
                    (0..7).map(|d| self.searches[d * 24 + hour]).fold((0, 0), |a, b| (a.0 + b[0], a.1 + b[1]));
                (n >= min).then(|| Quick { hour: hour as u32, wait_secs: secs / n, searches: n })
            })
            .collect();
        hours.sort_by_key(|q| (q.wait_secs, std::cmp::Reverse(q.searches)));
        hours.truncate(count);
        hours
    }
}

/// `▁▂▅█` bars for `values`, scaled between the smallest and largest.
pub fn sparkline(values: &[Option<u64>]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let known: Vec<u64> = values.iter().flatten().copied().collect();
    let (Some(&lo), Some(&hi)) = (known.iter().min(), known.iter().max()) else {
        return " ".repeat(values.len());
    };
    values
        .iter()
        .map(|v| match v {
            None => ' ',
            Some(_) if hi == lo => BARS[3],
            Some(v) => BARS[((v - lo) * 7 / (hi - lo)) as usize],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        // 2026-09-21 is a Monday.
        Local.with_ymd_and_hms(2026, 9, 21 + day, hour, minute, 0).unwrap()
    }

    #[test]
    fn quickest_hours_need_enough_searches() {
        let mut a = Activity::default();
        for (day, secs) in [(0, 10), (1, 20), (2, 30)] {
            a.searched(at(day, 22, 0), secs);
        }
        for secs in [100, 200, 300] {
            a.searched(at(3, 9, 0), secs);
        }
        a.searched(at(4, 3, 0), 1);
        let q = a.quickest(3, 5);
        assert_eq!(q, [Quick { hour: 22, wait_secs: 20, searches: 3 }, Quick { hour: 9, wait_secs: 200, searches: 3 }]);
    }

    #[test]
    fn online_samples_once_a_minute_and_average_by_hour() {
        let mut a = Activity::default();
        assert!(a.online(at(0, 20, 0), 100));
        assert!(!a.online(at(0, 20, 0), 900), "same minute");
        assert!(a.online(at(1, 20, 5), 300));
        let by_hour = a.online_by_hour();
        assert_eq!(by_hour[20], Some(200));
        assert_eq!(by_hour[3], None);
        assert_eq!(sparkline(&[Some(0), None, Some(70), Some(35)]), "▁ █▄");
    }

    #[test]
    fn saves_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("activity.json");
        let mut a = Activity::default();
        a.searched(at(0, 1, 0), 5);
        a.save(&path).unwrap();
        assert_eq!(Activity::load(&path).unwrap().searches, a.searches);
    }
}
