//! Chat history split per partner: kept in memory for the session and, when enabled,
//! appended to `~/.local/share/yap/logs/*.jsonl`.
//!
//! A log file is JSON Lines: a header line, then one line per chat entry. Appending
//! (rather than rewriting) keeps saves cheap and means a crash loses at most the
//! message being written.

use crate::app::chat::{Entry, EntryKind};
use crate::protocol::PartnerInfo;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Header {
    yap_log: u32,
    started: DateTime<Local>,
    partner: Option<PartnerInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    pub started: DateTime<Local>,
    /// When the last entry was written; `None` while the chat is live.
    pub ended: Option<DateTime<Local>>,
    pub partner: Option<PartnerInfo>,
    /// Messages (not status lines) exchanged.
    pub messages: usize,
    /// `None` for chats from earlier sessions until they're opened.
    entries: Option<Vec<Entry>>,
    pub path: Option<PathBuf>,
    /// How many entries are already on disk.
    written: usize,
}

impl Conversation {
    fn new(started: DateTime<Local>, partner: Option<PartnerInfo>) -> Self {
        Conversation { started, ended: None, partner, messages: 0, entries: Some(Vec::new()), path: None, written: 0 }
    }

    pub fn is_live(&self) -> bool {
        self.ended.is_none()
    }

    /// "Dominant Female Fox", or a placeholder for malformed files.
    pub fn title(&self) -> String {
        match &self.partner {
            Some(p) => format!("{} {} {}", p.role, p.gender, p.species),
            None => "unknown partner".into(),
        }
    }

    pub fn matches(&self, filter: &str) -> bool {
        let f = filter.to_lowercase();
        f.is_empty()
            || self.title().to_lowercase().contains(&f)
            || self.started.format("%Y-%m-%d").to_string().contains(&f)
    }

    fn push(&mut self, entry: Entry) {
        if matches!(entry.kind, EntryKind::You(_) | EntryKind::Partner(_)) {
            self.messages += 1;
        }
        if let Some(entries) = &mut self.entries {
            entries.push(entry);
        }
    }

    pub fn loaded_entries(&self) -> Option<&[Entry]> {
        self.entries.as_deref()
    }

    /// Plain-text transcript.
    pub fn transcript(&self) -> Option<String> {
        let mut out = format!("Chat with {} · started {}\n\n", self.title(), self.started.format("%Y-%m-%d %H:%M:%S"));
        for e in self.entries.as_ref()? {
            out.push_str(&e.transcript_line());
            out.push('\n');
        }
        Some(out)
    }
}

#[derive(Debug, Default)]
pub struct Logs {
    /// Oldest first.
    pub items: Vec<Conversation>,
    /// Index of the live conversation, if a partner is connected.
    live: Option<usize>,
}

impl Logs {
    /// Index every log file in `dir`. Entries aren't kept in memory until opened.
    pub fn load_dir(dir: &Path) -> (Self, Vec<String>) {
        let mut logs = Logs::default();
        let mut warnings = Vec::new();
        let Ok(read) = std::fs::read_dir(dir) else {
            return (logs, warnings);
        };
        let mut paths: Vec<PathBuf> = read
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .collect();
        paths.sort();
        for path in paths {
            match read_file(&path) {
                Ok(mut conv) => {
                    conv.entries = None;
                    logs.items.push(conv);
                }
                Err(e) => warnings.push(format!("skipped chat log {}: {e:#}", path.display())),
            }
        }
        logs.items.sort_by_key(|c| c.started);
        (logs, warnings)
    }

    pub fn live(&self) -> Option<&Conversation> {
        self.live.map(|i| &self.items[i])
    }

    /// Begin a new conversation with a freshly matched partner, ending any live one.
    pub fn start(&mut self, at: DateTime<Local>, partner: PartnerInfo) {
        self.end(at);
        self.items.push(Conversation::new(at, Some(partner)));
        self.live = Some(self.items.len() - 1);
    }

    /// Add to the live conversation. Returns false if there isn't one.
    pub fn record(&mut self, entry: Entry) -> bool {
        match self.live {
            Some(i) => {
                self.items[i].push(entry);
                true
            }
            None => false,
        }
    }

    /// Add a late note (e.g. "your previous partner has been blocked") to the most
    /// recent conversation from this session.
    pub fn record_after(&mut self, entry: Entry) {
        if let Some(conv) = self.items.last_mut().filter(|c| c.entries.is_some()) {
            conv.push(entry);
        }
    }

    pub fn end(&mut self, at: DateTime<Local>) {
        if let Some(i) = self.live.take() {
            self.items[i].ended = Some(at);
        }
    }

    /// Load a past conversation's entries from disk if needed.
    pub fn open(&mut self, index: usize) -> Result<&[Entry]> {
        let conv = self.items.get_mut(index).context("no such chat")?;
        if conv.entries.is_none() {
            let path = conv.path.clone().context("chat has no file")?;
            conv.entries = read_file(&path)?.entries;
        }
        Ok(conv.entries.as_deref().unwrap_or_default())
    }

    /// Remove a conversation, deleting its file. The live chat can't be deleted.
    pub fn delete(&mut self, index: usize) -> Result<Conversation> {
        anyhow::ensure!(self.live != Some(index), "can't delete the chat in progress");
        let conv = self.items.get(index).context("no such chat")?;
        if let Some(path) = &conv.path {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("deleting {}", path.display())),
            }
        }
        if let Some(live) = self.live.as_mut().filter(|l| **l > index) {
            *live -= 1;
        }
        Ok(self.items.remove(index))
    }

    /// Append anything not yet on disk for this session's conversations.
    pub fn save(&mut self, dir: &Path) -> Result<()> {
        for conv in &mut self.items {
            let Some(entries) = &conv.entries else { continue };
            if conv.written == entries.len() && conv.path.is_some() {
                continue;
            }
            let path = match &conv.path {
                Some(p) => p.clone(),
                None => {
                    create_private_dir(dir)?;
                    let path = unique_path(dir, conv.started);
                    let header = Header { yap_log: 1, started: conv.started, partner: conv.partner.clone() };
                    let mut file = open_private(&path)?;
                    writeln!(file, "{}", serde_json::to_string(&header)?)?;
                    conv.path = Some(path.clone());
                    path
                }
            };
            let mut file = open_private(&path)?;
            for entry in &entries[conv.written..] {
                writeln!(file, "{}", serde_json::to_string(entry)?)?;
            }
            conv.written = entries.len();
        }
        Ok(())
    }
}

fn unique_path(dir: &Path, started: DateTime<Local>) -> PathBuf {
    let stem = started.format("%Y%m%d-%H%M%S").to_string();
    (0..)
        .map(|n| if n == 0 { dir.join(format!("{stem}.jsonl")) } else { dir.join(format!("{stem}-{n}.jsonl")) })
        .find(|p| !p.exists())
        .expect("infinite iterator")
}

fn read_file(path: &Path) -> Result<Conversation> {
    let file = std::fs::File::open(path)?;
    let mut lines = std::io::BufReader::new(file).lines();
    let header: Header = serde_json::from_str(&lines.next().context("empty file")??).context("bad header")?;
    let mut conv = Conversation::new(header.started, header.partner);
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // A torn final line (crash mid-write) shouldn't make the whole chat unreadable.
        let Ok(entry) = serde_json::from_str::<Entry>(&line) else { continue };
        conv.ended = Some(entry.at);
        conv.push(entry);
    }
    conv.ended.get_or_insert(conv.started);
    conv.written = conv.entries.as_ref().map_or(0, Vec::len);
    conv.path = Some(path.to_owned());
    Ok(conv)
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if !dir.exists() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    Ok(())
}

fn open_private(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path).with_context(|| format!("opening {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Local> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).unwrap().into()
    }

    fn fox() -> PartnerInfo {
        PartnerInfo {
            gender: "Female".into(),
            species: "Fox".into(),
            kinks: "any".into(),
            role: "Dominant".into(),
            language: None,
        }
    }

    fn entry(secs: i64, kind: EntryKind) -> Entry {
        Entry { at: at(secs), kind }
    }

    #[test]
    fn conversations_split_per_partner() {
        let mut logs = Logs::default();
        assert!(!logs.record(entry(0, EntryKind::System("searching".into()))), "nothing live yet");
        logs.start(at(1), fox());
        logs.record(entry(2, EntryKind::Partner("hi".into())));
        logs.record(entry(3, EntryKind::You("hey".into())));
        logs.start(at(10), PartnerInfo { species: "Wolf".into(), ..fox() });
        logs.record(entry(11, EntryKind::Partner("yo".into())));
        logs.end(at(12));
        logs.record_after(entry(13, EntryKind::System("blocked".into())));

        assert_eq!(logs.items.len(), 2);
        assert_eq!(logs.items[0].messages, 2);
        assert_eq!(logs.items[0].ended, Some(at(10)), "starting a new chat ends the old one");
        assert_eq!(logs.items[1].title(), "Dominant Female Wolf");
        assert_eq!(logs.items[1].loaded_entries().unwrap().len(), 2);
        assert!(logs.live().is_none());
    }

    #[test]
    fn saves_incrementally_and_reloads_lazily() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join("logs");
        let mut logs = Logs::default();
        logs.start(at(0), fox());
        logs.record(entry(1, EntryKind::Partner("one".into())));
        logs.save(&logs_dir).unwrap();
        logs.record(entry(2, EntryKind::You("two".into())));
        logs.save(&logs_dir).unwrap();
        logs.save(&logs_dir).unwrap();

        let path = logs.items[0].path.clone().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3, "header + 2 entries, no duplicates");

        let (mut loaded, warnings) = Logs::load_dir(&logs_dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].messages, 2);
        assert_eq!(loaded.items[0].ended, Some(at(2)));
        assert!(loaded.items[0].loaded_entries().is_none(), "entries load on demand");
        let entries = loaded.open(0).unwrap();
        assert_eq!(entries[1].kind, EntryKind::You("two".into()));
        assert!(loaded.items[0].transcript().unwrap().contains("You: two"));
    }

    #[cfg(unix)]
    #[test]
    fn log_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join("logs");
        let mut logs = Logs::default();
        logs.start(at(0), fox());
        logs.save(&logs_dir).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&logs_dir), 0o700);
        assert_eq!(mode(logs.items[0].path.as_ref().unwrap()), 0o600);
    }

    #[test]
    fn tolerates_torn_lines_and_skips_garbage_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut logs = Logs::default();
        logs.start(at(0), fox());
        logs.record(entry(1, EntryKind::Partner("kept".into())));
        logs.save(dir.path()).unwrap();
        let path = logs.items[0].path.clone().unwrap();
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "{{\"at\":\"2023-").unwrap();
        std::fs::write(dir.path().join("junk.jsonl"), "not json\n").unwrap();

        let (mut loaded, warnings) = Logs::load_dir(dir.path());
        assert_eq!(warnings.len(), 1);
        assert_eq!(loaded.open(0).unwrap().len(), 1);
    }

    #[test]
    fn delete_removes_file_but_not_live_chat() {
        let dir = tempfile::tempdir().unwrap();
        let mut logs = Logs::default();
        logs.start(at(0), fox());
        logs.start(at(5), fox());
        logs.save(dir.path()).unwrap();
        assert!(logs.delete(1).is_err());
        let path = logs.items[0].path.clone().unwrap();
        logs.delete(0).unwrap();
        assert!(!path.exists());
        assert_eq!(logs.live().unwrap().started, at(5), "live index shifts down");
    }

    #[test]
    fn same_second_chats_get_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut logs = Logs::default();
        logs.start(at(0), fox());
        logs.start(at(0), fox());
        logs.save(dir.path()).unwrap();
        assert_ne!(logs.items[0].path, logs.items[1].path);
    }
}
