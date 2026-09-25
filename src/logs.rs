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
use std::collections::{BTreeMap, HashMap};
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
    /// A name you gave this chat.
    pub name: Option<String>,
    /// Pinned chats sort to the top.
    pub pinned: bool,
}

/// Names and pins, kept beside the logs so the append-only files are never rewritten.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct MetaFile {
    #[serde(default)]
    chats: BTreeMap<String, ChatMeta>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct ChatMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
}

const META_FILE: &str = "meta.toml";

impl Conversation {
    fn new(started: DateTime<Local>, partner: Option<PartnerInfo>) -> Self {
        Conversation {
            started,
            ended: None,
            partner,
            messages: 0,
            entries: Some(Vec::new()),
            path: None,
            written: 0,
            name: None,
            pinned: false,
        }
    }

    pub fn is_live(&self) -> bool {
        self.ended.is_none()
    }

    /// Your name for the chat, else "Dominant Female Fox".
    pub fn title(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        self.partner_title()
    }

    pub fn partner_title(&self) -> String {
        match &self.partner {
            Some(p) => format!("{} {} {}", p.role, p.gender, p.species),
            None => "unknown partner".into(),
        }
    }

    /// Match on name, partner, date or (if loaded) anything said.
    pub fn matches(&self, filter: &str) -> bool {
        let f = filter.to_lowercase();
        f.is_empty()
            || self.title().to_lowercase().contains(&f)
            || self.partner_title().to_lowercase().contains(&f)
            || self.started.format("%Y-%m-%d").to_string().contains(&f)
            || self.text_match(&f).is_some()
    }

    /// A short excerpt around the first message containing `needle` (lowercase).
    pub fn text_match(&self, needle: &str) -> Option<String> {
        if needle.chars().count() < 2 {
            return None;
        }
        self.entries.as_ref()?.iter().find_map(|e| {
            let text = e.message_text()?;
            let lower = text.to_lowercase();
            // Byte offsets only line up when lowercasing kept lengths.
            let at = lower.find(needle).filter(|_| lower.len() == text.len())?;
            let start = text[..at].char_indices().rev().nth(20).map_or(0, |(i, _)| i);
            let end = text[at..].char_indices().nth(needle.chars().count() + 30).map_or(text.len(), |(i, _)| at + i);
            let mut excerpt = String::new();
            if start > 0 {
                excerpt.push('…');
            }
            excerpt.push_str(&text[start..end]);
            if end < text.len() {
                excerpt.push('…');
            }
            Some(excerpt)
        })
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
    /// Names or pins changed since the last save.
    meta_dirty: bool,
    /// Index of each session's live conversation, keyed by session id.
    live: HashMap<u64, usize>,
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
        match std::fs::read_to_string(dir.join(META_FILE)).map(|s| toml::from_str::<MetaFile>(&s)) {
            Ok(Ok(meta)) => {
                for conv in &mut logs.items {
                    let key = conv.path.as_deref().and_then(Path::file_name).and_then(|n| n.to_str());
                    if let Some(m) = key.and_then(|k| meta.chats.get(k)) {
                        conv.name = m.name.clone();
                        conv.pinned = m.pinned;
                    }
                }
            }
            Ok(Err(e)) => warnings.push(format!("ignored chat names/pins in {META_FILE}: {e}")),
            Err(_) => {}
        }
        (logs, warnings)
    }

    /// Name a chat (an empty name clears it).
    pub fn rename(&mut self, index: usize, name: &str) {
        if let Some(conv) = self.items.get_mut(index) {
            let name = name.trim();
            conv.name = (!name.is_empty()).then(|| name.to_owned());
            self.meta_dirty = true;
        }
    }

    pub fn toggle_pin(&mut self, index: usize) -> bool {
        match self.items.get_mut(index) {
            Some(conv) => {
                conv.pinned ^= true;
                self.meta_dirty = true;
                conv.pinned
            }
            None => false,
        }
    }

    /// Load every chat's messages so searches can look inside them.
    pub fn load_all(&mut self) {
        for conv in &mut self.items {
            if conv.entries.is_none()
                && let Some(path) = &conv.path
                && let Ok(full) = read_file(path)
            {
                conv.entries = full.entries;
            }
        }
    }

    pub fn live(&self, session: u64) -> Option<&Conversation> {
        self.live.get(&session).map(|&i| &self.items[i])
    }

    /// Begin a new conversation with a freshly matched partner, ending the session's
    /// previous one.
    pub fn start(&mut self, session: u64, at: DateTime<Local>, partner: PartnerInfo) {
        self.end(session, at);
        self.items.push(Conversation::new(at, Some(partner)));
        self.live.insert(session, self.items.len() - 1);
    }

    /// Add to the session's live conversation. Returns false if there isn't one.
    pub fn record(&mut self, session: u64, entry: Entry) -> bool {
        match self.live.get(&session) {
            Some(&i) => {
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

    pub fn end(&mut self, session: u64, at: DateTime<Local>) {
        if let Some(i) = self.live.remove(&session) {
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
        anyhow::ensure!(!self.live.values().any(|&i| i == index), "can't delete a chat in progress");
        let conv = self.items.get(index).context("no such chat")?;
        if conv.name.is_some() || conv.pinned {
            self.meta_dirty = true;
        }
        if let Some(path) = &conv.path {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("deleting {}", path.display())),
            }
        }
        for live in self.live.values_mut().filter(|l| **l > index) {
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
        if std::mem::take(&mut self.meta_dirty) {
            self.save_meta(dir)?;
        }
        Ok(())
    }

    fn save_meta(&self, dir: &Path) -> Result<()> {
        let chats = self
            .items
            .iter()
            .filter(|c| c.name.is_some() || c.pinned)
            .filter_map(|c| {
                let file = c.path.as_deref()?.file_name()?.to_str()?.to_owned();
                Some((file, ChatMeta { name: c.name.clone(), pinned: c.pinned }))
            })
            .collect();
        create_private_dir(dir)?;
        crate::config::write_atomic(&dir.join(META_FILE), &toml::to_string_pretty(&MetaFile { chats })?)
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
        assert!(!logs.record(0, entry(0, EntryKind::System("searching".into()))), "nothing live yet");
        logs.start(0, at(1), fox());
        logs.record(0, entry(2, EntryKind::Partner("hi".into())));
        logs.record(0, entry(3, EntryKind::You("hey".into())));
        logs.start(0, at(10), PartnerInfo { species: "Wolf".into(), ..fox() });
        logs.record(0, entry(11, EntryKind::Partner("yo".into())));
        logs.end(0, at(12));
        logs.record_after(entry(13, EntryKind::System("blocked".into())));

        assert_eq!(logs.items.len(), 2);
        assert_eq!(logs.items[0].messages, 2);
        assert_eq!(logs.items[0].ended, Some(at(10)), "starting a new chat ends the old one");
        assert_eq!(logs.items[1].title(), "Dominant Female Wolf");
        assert_eq!(logs.items[1].loaded_entries().unwrap().len(), 2);
        assert!(logs.live(0).is_none());
    }

    #[test]
    fn saves_incrementally_and_reloads_lazily() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join("logs");
        let mut logs = Logs::default();
        logs.start(0, at(0), fox());
        logs.record(0, entry(1, EntryKind::Partner("one".into())));
        logs.save(&logs_dir).unwrap();
        logs.record(0, entry(2, EntryKind::You("two".into())));
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
        logs.start(0, at(0), fox());
        logs.save(&logs_dir).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&logs_dir), 0o700);
        assert_eq!(mode(logs.items[0].path.as_ref().unwrap()), 0o600);
    }

    #[test]
    fn tolerates_torn_lines_and_skips_garbage_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut logs = Logs::default();
        logs.start(0, at(0), fox());
        logs.record(0, entry(1, EntryKind::Partner("kept".into())));
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
        logs.start(0, at(0), fox());
        logs.start(0, at(5), fox());
        logs.save(dir.path()).unwrap();
        assert!(logs.delete(1).is_err());
        let path = logs.items[0].path.clone().unwrap();
        logs.delete(0).unwrap();
        assert!(!path.exists());
        assert_eq!(logs.live(0).unwrap().started, at(5), "live index shifts down");
    }

    #[test]
    fn sessions_have_independent_live_chats() {
        let mut logs = Logs::default();
        logs.start(1, at(0), fox());
        logs.start(2, at(1), PartnerInfo { species: "Wolf".into(), ..fox() });
        logs.record(1, entry(2, EntryKind::Partner("to one".into())));
        logs.record(2, entry(3, EntryKind::Partner("to two".into())));
        logs.end(1, at(4));
        assert!(logs.live(1).is_none());
        assert_eq!(logs.live(2).unwrap().messages, 1);
        assert_eq!(logs.items[0].messages, 1);
        assert!(logs.delete(1).is_err(), "session 2's chat is live");
        logs.delete(0).unwrap();
        assert_eq!(logs.live(2).unwrap().title(), "Dominant Female Wolf", "index follows the removal");
    }

    #[test]
    fn names_and_pins_persist_beside_the_logs() {
        let dir = tempfile::tempdir().unwrap();
        let mut logs = Logs::default();
        logs.start(0, at(0), fox());
        logs.start(0, at(5), fox());
        logs.end(0, at(9));
        logs.rename(0, "  the good one ");
        assert!(logs.toggle_pin(1));
        logs.save(dir.path()).unwrap();
        let (loaded, warnings) = Logs::load_dir(dir.path());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(loaded.items[0].title(), "the good one");
        assert_eq!(loaded.items[0].partner_title(), "Dominant Female Fox");
        assert!(loaded.items[1].pinned);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3, "two logs plus meta.toml");
    }

    #[test]
    fn full_text_search_with_excerpts() {
        let dir = tempfile::tempdir().unwrap();
        let mut logs = Logs::default();
        logs.start(0, at(0), fox());
        logs.record(0, entry(1, EntryKind::Partner("We met at the old lighthouse by the sea, remember?".into())));
        logs.save(dir.path()).unwrap();
        let (mut loaded, _) = Logs::load_dir(dir.path());
        assert!(!loaded.items[0].matches("lighthouse"), "not loaded yet");
        loaded.load_all();
        assert!(loaded.items[0].matches("LIGHTHOUSE"));
        assert_eq!(
            loaded.items[0].text_match("lighthouse").unwrap(),
            "We met at the old lighthouse by the sea, remember?"
        );
        assert!(loaded.items[0].matches("fox"), "partner still matches");
        assert!(!loaded.items[0].matches("volcano"));
    }

    #[test]
    fn same_second_chats_get_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut logs = Logs::default();
        logs.start(0, at(0), fox());
        logs.start(0, at(0), fox());
        logs.save(dir.path()).unwrap();
        assert_ne!(logs.items[0].path, logs.items[1].path);
    }
}
