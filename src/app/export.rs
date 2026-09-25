//! Import/export of profiles and settings, chat log files and traffic dumps.

use super::*;
use crate::chat_export::{self, Doc, Format};

impl App {
    pub fn export(&mut self, profile: Option<&str>, path: &str) {
        let path = config::expand_tilde(path);
        let result = self
            .config
            .export(profile)
            .map_err(anyhow::Error::from)
            .and_then(|doc| config::export_to_file(&doc, &path));
        match result {
            Ok(()) => self.toast(Level::Success, format!("Exported to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Export failed: {e:#}")),
        }
    }

    pub fn import(&mut self, path: &str, with_settings: bool) {
        let path = config::expand_tilde(path);
        match config::read_import(&path) {
            Ok(doc) => {
                let report = self.config.import(doc, with_settings);
                self.config_changed();
                if report.settings_applied {
                    let theme = self.config.settings.theme.clone();
                    self.theme =
                        effective_theme(pick_theme(&self.themes, &theme), self.config.settings.transparent_background);
                    self.traffic.set_capacity(self.config.settings.traffic.capacity);
                }
                let mut msg = match report.profiles.len() {
                    0 => "Imported no profiles".to_owned(),
                    _ => format!("Imported {}", report.profiles.join(", ")),
                };
                if report.settings_applied {
                    msg.push_str(" and settings");
                }
                self.toast(Level::Success, msg);
                for w in report.warnings {
                    self.toast(Level::Warning, w);
                }
            }
            Err(e) => self.toast(Level::Error, format!("Import failed: {e:#}")),
        }
    }

    /// Indices into `logs.items` shown in the Logs tab: newest first, filtered.
    pub fn visible_logs(&self) -> Vec<usize> {
        let mut shown = self.visible_logs_unsorted();
        // Pinned first; otherwise newest first (the stable sort keeps that order).
        shown.sort_by_key(|&i| !self.logs.items[i].pinned);
        shown
    }

    fn visible_logs_unsorted(&self) -> Vec<usize> {
        let filter = &self.logs_ui.list.filter;
        (0..self.logs.items.len()).rev().filter(|&i| self.logs.items[i].matches(filter)).collect()
    }

    pub fn selected_log(&self) -> Option<usize> {
        self.visible_logs().get(self.logs_ui.list.selected).copied()
    }

    /// Save a chat from the Logs tab; the extension picks the format (.txt, .md, .html).
    pub fn export_log(&mut self, index: usize, path: &str) {
        let path = config::expand_tilde(path);
        let result = self.logs.open(index).map(|_| ()).and_then(|()| {
            let conv = &self.logs.items[index];
            let (you, partner) = conv.names();
            let doc = Doc {
                title: format!("Chat with {}", conv.title()),
                started: Some(conv.started),
                you,
                partner,
                entries: conv.loaded_entries().unwrap_or_default(),
                images: &|text| self.preview_urls(text),
            };
            config::write_atomic(&path, &chat_export::render(&doc, Format::from_path(&path)))
        });
        match result {
            Ok(()) => self.toast(Level::Success, format!("Saved transcript to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Couldn't save transcript: {e:#}")),
        }
    }

    /// `/log <path>`: save the chat view; the extension picks the format.
    pub(super) fn save_log(&mut self, path: &str) {
        let path = config::expand_tilde(path);
        let you =
            Some(self.config.active().character.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| "You".into());
        let partner = self.partner_nick.clone().unwrap_or_else(|| "Partner".into());
        let title = match self.logs.live(self.session_id) {
            Some(conv) => format!("Chat with {}", conv.title()),
            None => "Chat".into(),
        };
        let doc = Doc {
            title,
            started: self.chat.entries.first().map(|e| e.at),
            you,
            partner,
            entries: &self.chat.entries,
            images: &|text| self.preview_urls(text),
        };
        match config::write_atomic(&path, &chat_export::render(&doc, Format::from_path(&path))) {
            Ok(()) => self.toast(Level::Success, format!("Saved transcript to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Couldn't save transcript: {e:#}")),
        }
    }

    pub fn export_traffic(&mut self, path: &str) {
        let path = config::expand_tilde(path);
        let mut buf = Vec::new();
        let result = self
            .traffic
            .export(&mut buf)
            .map_err(anyhow::Error::from)
            .and_then(|n| config::write_atomic(&path, &String::from_utf8_lossy(&buf)).map(|()| n));
        match result {
            Ok(n) => self.toast(Level::Success, format!("Wrote {n} frames to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Traffic export failed: {e:#}")),
        }
    }

    /// Where file prompts start: `~/yap-<name>`.
    pub fn default_path(&self, name: &str) -> String {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().join(name))
            .unwrap_or_else(|| PathBuf::from(name))
            .display()
            .to_string()
    }
}
