//! Everything yap keeps, in one file, for moving to another machine: settings and
//! profiles, themes, buddies, the drawer, stats, history, activity and open tabs, and
//! (if you ask) chat logs.

use crate::config::Paths;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub yap_backup: u32,
    pub created: DateTime<Local>,
    /// `config/…` or `data/…` path → file contents.
    pub files: BTreeMap<String, String>,
}

/// The folder a backup path prefix stands for.
fn root<'a>(paths: &'a Paths, prefix: &str) -> Option<&'a Path> {
    match prefix {
        "config" => paths.config_file.parent(),
        "data" => Some(&paths.data_dir),
        _ => None,
    }
}

/// Files (not folders) directly inside `dir` with one of `exts`.
fn files_in(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)))
        .collect();
    out.sort();
    out
}

impl Backup {
    pub fn create(paths: &Paths, with_logs: bool) -> Result<Self> {
        let config_dir = paths.config_file.parent().context("config file has no folder")?;
        let mut wanted: Vec<(String, PathBuf)> = vec![("config/config.toml".into(), paths.config_file.clone())];
        for (sub, exts) in [("themes", &["toml"][..]), ("buddies", &["toml"][..])] {
            for f in files_in(&config_dir.join(sub), exts) {
                wanted.push((format!("config/{sub}/{}", f.file_name().unwrap().to_string_lossy()), f));
            }
        }
        for (name, path) in [
            ("drawer.toml", &paths.drawer_file),
            ("stats.toml", &paths.stats_file),
            ("history.jsonl", &paths.history_file),
            ("activity.json", &paths.activity_file),
            ("tabs.toml", &paths.tabs_file),
        ] {
            wanted.push((format!("data/{name}"), path.clone()));
        }
        if with_logs {
            for f in files_in(&paths.logs_dir, &["jsonl", "toml"]) {
                wanted.push((format!("data/logs/{}", f.file_name().unwrap().to_string_lossy()), f));
            }
        }
        let mut files = BTreeMap::new();
        for (name, path) in wanted {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    files.insert(name, text);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
            }
        }
        Ok(Backup { yap_backup: VERSION, created: Local::now(), files })
    }

    /// Write the backup, readable only by you (it may hold chat logs).
    pub fn write(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        let mut file = crate::logs::open_private(path)?;
        file.set_len(0)?;
        std::io::Write::write_all(&mut file, text.as_bytes())?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let backup: Backup = serde_json::from_str(&text).context("not a yap backup")?;
        if backup.yap_backup > VERSION {
            bail!("this backup is from a newer yap");
        }
        Ok(backup)
    }

    /// Put the files back. Anything it would overwrite (and that differs) is kept
    /// beside it as `<name>.before-restore`. Returns the files written.
    pub fn restore(&self, paths: &Paths) -> Result<Vec<PathBuf>> {
        let mut written = Vec::new();
        for (name, text) in &self.files {
            let (prefix, rest) = name.split_once('/').context("bad file name in backup")?;
            let rest = Path::new(rest);
            if !rest.components().all(|c| matches!(c, Component::Normal(_))) {
                bail!("refusing a file outside yap's folders: {name}");
            }
            let dest = root(paths, prefix).with_context(|| format!("unknown place in backup: {name}"))?.join(rest);
            if let Ok(existing) = std::fs::read_to_string(&dest) {
                if existing == *text {
                    continue;
                }
                let mut aside = dest.clone().into_os_string();
                aside.push(".before-restore");
                std::fs::rename(&dest, aside)?;
            }
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            if name.starts_with("data/logs/") || name == "data/history.jsonl" {
                std::io::Write::write_all(&mut crate::logs::open_private(&dest)?, text.as_bytes())?;
            } else {
                crate::config::write_atomic(&dest, text)?;
            }
            written.push(dest);
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_keeps_what_it_overwrites() {
        let from = tempfile::tempdir().unwrap();
        let paths = Paths::in_dir(from.path());
        std::fs::write(&paths.config_file, "theme = \"nord\"\n").unwrap();
        std::fs::create_dir_all(&paths.themes_dir).unwrap();
        std::fs::write(paths.themes_dir.join("mine.toml"), "extends = \"nord\"\n").unwrap();
        std::fs::write(&paths.history_file, "{}\n").unwrap();
        std::fs::create_dir_all(&paths.logs_dir).unwrap();
        std::fs::write(paths.logs_dir.join("chat.jsonl"), "{}\n").unwrap();

        let without = Backup::create(&paths, false).unwrap();
        assert!(!without.files.keys().any(|k| k.starts_with("data/logs/")));
        let backup = Backup::create(&paths, true).unwrap();
        assert!(backup.files.contains_key("config/themes/mine.toml"));
        assert!(backup.files.contains_key("data/logs/chat.jsonl"));
        let file = from.path().join("backup.json");
        backup.write(&file).unwrap();

        let to = tempfile::tempdir().unwrap();
        let other = Paths::in_dir(to.path());
        std::fs::write(&other.config_file, "theme = \"light\"\n").unwrap();
        let written = Backup::read(&file).unwrap().restore(&other).unwrap();
        assert!(written.contains(&other.config_file));
        assert_eq!(std::fs::read_to_string(&other.config_file).unwrap(), "theme = \"nord\"\n");
        let aside = to.path().join("config.toml.before-restore");
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "theme = \"light\"\n");
        assert!(other.logs_dir.join("chat.jsonl").exists());

        let mut evil = backup.clone();
        evil.files.insert("data/../../escape".into(), String::new());
        assert!(evil.restore(&other).is_err());
    }
}
