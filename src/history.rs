//! Everyone you've been matched with: who they were, how it ended and how long it
//! lasted. No messages are kept, only these facts, and only on this machine.

use crate::protocol::PartnerInfo;
use anyhow::Result;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    YouLeft,
    YouBlocked,
    TheyLeft,
    TheyDropped,
    ConnectionLost,
    Skipped,
}

impl Outcome {
    pub const ALL: [Outcome; 6] = [
        Outcome::YouLeft,
        Outcome::TheyLeft,
        Outcome::TheyDropped,
        Outcome::YouBlocked,
        Outcome::ConnectionLost,
        Outcome::Skipped,
    ];

    pub fn describe(self) -> &'static str {
        match self {
            Outcome::YouLeft => "you left",
            Outcome::YouBlocked => "you blocked",
            Outcome::TheyLeft => "they left",
            Outcome::TheyDropped => "they dropped",
            Outcome::ConnectionLost => "connection lost",
            Outcome::Skipped => "auto-skipped",
        }
    }
}

/// Which auto-skip rule a partner broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipRule {
    Limit,
    SharedKinks,
    Language,
}

impl SkipRule {
    pub fn describe(self) -> &'static str {
        match self {
            SkipRule::Limit => "one of your limits",
            SkipRule::SharedKinks => "too few shared kinks",
            SkipRule::Language => "a different language",
        }
    }

    /// For narrow columns.
    pub fn short(self) -> &'static str {
        match self {
            SkipRule::Limit => "limit",
            SkipRule::SharedKinks => "few shared kinks",
            SkipRule::Language => "language",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// When you were matched.
    pub at: DateTime<Local>,
    pub gender: String,
    pub species: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<SkipRule>,
    #[serde(default)]
    pub secs: u64,
    #[serde(default)]
    pub sent: u32,
    #[serde(default)]
    pub received: u32,
    #[serde(default)]
    pub shared_kinks: u32,
    #[serde(default)]
    pub profile: String,
}

impl Record {
    pub fn new(at: DateTime<Local>, info: &PartnerInfo, outcome: Outcome) -> Self {
        Record {
            at,
            gender: info.gender.clone(),
            species: info.species.clone(),
            role: info.role.clone(),
            language: info.language.clone(),
            outcome,
            skip: None,
            secs: 0,
            sent: 0,
            received: 0,
            shared_kinks: 0,
            profile: String::new(),
        }
    }

    pub fn partner(&self) -> String {
        format!("{} {} {}", self.role, self.gender, self.species)
    }

    fn chatted(&self) -> bool {
        self.outcome != Outcome::Skipped
    }
}

/// Numbers worth showing, worked out from the records.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub met: usize,
    pub skipped: usize,
    pub outcomes: Vec<(Outcome, usize)>,
    pub average_secs: u64,
    /// Most-met species: name, chats, average length.
    pub species: Vec<(String, usize, u64)>,
    pub skip_rules: Vec<(SkipRule, usize)>,
    /// Average chat length by shared kinks: none, one or two, three or more.
    pub by_shared: [(usize, u64); 3],
    /// Chats where nobody said anything.
    pub silent: usize,
}

#[derive(Debug, Default)]
pub struct History {
    pub records: Vec<Record>,
    /// How many records are already in the file.
    written: usize,
}

impl History {
    /// Read `history.jsonl`, skipping lines that don't parse.
    pub fn load(path: &Path) -> (Self, Vec<String>) {
        let src = match std::fs::read_to_string(path) {
            Ok(src) => src,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (History::default(), Vec::new()),
            Err(e) => return (History::default(), vec![format!("couldn't read {}: {e}", path.display())]),
        };
        let mut bad = 0;
        let records: Vec<Record> = src
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).map_err(|_| bad += 1).ok())
            .collect();
        let warnings =
            if bad > 0 { vec![format!("skipped {bad} unreadable lines in {}", path.display())] } else { Vec::new() };
        let written = records.len();
        (History { records, written }, warnings)
    }

    pub fn push(&mut self, record: Record) {
        self.records.push(record);
    }

    pub fn is_dirty(&self) -> bool {
        self.written < self.records.len()
    }

    /// Append records that aren't in the file yet.
    pub fn save(&mut self, path: &Path) -> Result<()> {
        if !self.is_dirty() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = crate::logs::open_private(path)?;
        let mut out = String::new();
        for r in &self.records[self.written..] {
            out.push_str(&serde_json::to_string(r)?);
            out.push('\n');
        }
        file.write_all(out.as_bytes())?;
        self.written = self.records.len();
        Ok(())
    }

    /// Forget everything, including the file.
    pub fn clear(&mut self, path: &Path) -> Result<()> {
        self.records.clear();
        self.written = 0;
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    pub fn summary(&self) -> Summary {
        let chats: Vec<&Record> = self.records.iter().filter(|r| r.chatted()).collect();
        let avg = |rs: &[&Record]| rs.iter().map(|r| r.secs).sum::<u64>().checked_div(rs.len() as u64).unwrap_or(0);

        let mut outcomes: BTreeMap<Outcome, usize> = BTreeMap::new();
        let mut rules: BTreeMap<SkipRule, usize> = BTreeMap::new();
        for r in &self.records {
            *outcomes.entry(r.outcome).or_default() += 1;
            if let Some(rule) = r.skip {
                *rules.entry(rule).or_default() += 1;
            }
        }
        let mut species: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
        for r in &chats {
            species.entry(r.species.as_str()).or_default().push(r);
        }
        let mut species: Vec<(String, usize, u64)> =
            species.into_iter().map(|(name, rs)| (name.to_owned(), rs.len(), avg(&rs))).collect();
        species.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
        species.truncate(5);

        let bucket = |lo: u32, hi: u32| {
            let rs: Vec<&Record> = chats.iter().copied().filter(|r| (lo..=hi).contains(&r.shared_kinks)).collect();
            (rs.len(), avg(&rs))
        };
        let mut rules: Vec<(SkipRule, usize)> = rules.into_iter().collect();
        rules.sort_by_key(|r| std::cmp::Reverse(r.1));
        Summary {
            met: chats.len(),
            skipped: self.records.len() - chats.len(),
            outcomes: Outcome::ALL.iter().filter_map(|o| outcomes.get(o).map(|&n| (*o, n))).collect(),
            average_secs: avg(&chats),
            species,
            skip_rules: rules,
            by_shared: [bucket(0, 0), bucket(1, 2), bucket(3, u32::MAX)],
            silent: chats.iter().filter(|r| r.sent == 0 && r.received == 0).count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(species: &str, outcome: Outcome, secs: u64, shared: u32) -> Record {
        let info = PartnerInfo {
            gender: "Female".into(),
            species: species.into(),
            kinks: "Musk".into(),
            role: "Dominant".into(),
            language: None,
        };
        Record { secs, shared_kinks: shared, ..Record::new(Local::now(), &info, outcome) }
    }

    #[test]
    fn saves_incrementally_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let mut h = History::default();
        h.push(record("Fox", Outcome::TheyLeft, 60, 1));
        h.save(&path).unwrap();
        h.push(record("Wolf", Outcome::YouLeft, 30, 0));
        h.save(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2, "appended, not rewritten");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "not json\n").unwrap();
        let (loaded, warnings) = History::load(&path);
        assert_eq!(loaded.records, h.records);
        assert_eq!(warnings.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let mut loaded = loaded;
        loaded.clear(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn summary_groups_and_averages() {
        let mut h = History::default();
        h.push(record("Fox", Outcome::TheyLeft, 600, 3));
        h.push(record("Fox", Outcome::YouLeft, 300, 1));
        h.push(record("Wolf", Outcome::TheyLeft, 60, 0));
        h.push(Record { skip: Some(SkipRule::Language), ..record("Cat", Outcome::Skipped, 0, 0) });
        let s = h.summary();
        assert_eq!((s.met, s.skipped), (3, 1));
        assert_eq!(s.average_secs, 320);
        assert_eq!(s.species[0], ("Fox".to_owned(), 2, 450));
        assert_eq!(s.outcomes[0], (Outcome::YouLeft, 1));
        assert!(s.outcomes.contains(&(Outcome::TheyLeft, 2)));
        assert_eq!(s.skip_rules, [(SkipRule::Language, 1)]);
        assert_eq!(s.by_shared, [(1, 60), (1, 300), (1, 600)]);
        assert_eq!(s.silent, 3);
    }
}
