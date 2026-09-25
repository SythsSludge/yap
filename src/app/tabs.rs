//! Reopening your chat tabs (and the profile each used) the next time yap starts.
//! Partners can't come back, since each tab is a fresh anonymous connection, but
//! the tabs and characters do.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tabs {
    /// Each tab's profile, in order.
    pub profiles: Vec<String>,
    /// Which tab was on screen (0-based).
    pub active: usize,
}

impl Tabs {
    pub fn load(path: &Path) -> Option<Self> {
        toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        config::write_atomic(path, &toml::to_string_pretty(self)?)
    }
}

impl App {
    pub fn tabs(&self) -> Tabs {
        let sessions = self.sessions();
        Tabs {
            active: sessions.iter().position(|s| s.active).unwrap_or(0),
            profiles: sessions.into_iter().map(|s| s.profile).collect(),
        }
    }

    /// Open the tabs from last time. Call before the event loop starts.
    pub fn restore_tabs(&mut self, tabs: Tabs) {
        if !self.config.settings.reopen_tabs || tabs.profiles.is_empty() {
            return;
        }
        let exists = |app: &App, name: &str| app.config.profiles.iter().any(|p| p.name == name);
        for (i, profile) in tabs.profiles.iter().enumerate() {
            if i > 0 {
                self.new_session();
            }
            if exists(self, profile) {
                self.config.active_profile = profile.clone();
            }
        }
        if let Some(id) = self.sessions().get(tabs.active).map(|s| s.id) {
            self.switch_session(id);
        }
        self.toasts.clear();
        if tabs.profiles.len() > 1 {
            self.toast(Level::Info, format!("Reopened {} chats.", tabs.profiles.len()));
        }
        self.saved_tabs = Some(self.tabs());
    }

    /// Write the tab list if it changed.
    pub(super) fn save_tabs(&mut self) {
        if !self.config.settings.reopen_tabs {
            return;
        }
        let tabs = self.tabs();
        if self.saved_tabs.as_ref() == Some(&tabs) {
            return;
        }
        if let Err(e) = tabs.save(&self.paths.tabs_file) {
            self.toast(Level::Error, format!("couldn't save open tabs: {e:#}"));
        }
        self.saved_tabs = Some(tabs);
    }

    /// Connect every chat that hasn't started connecting yet.
    pub fn connect_idle(&mut self) {
        let idle: Vec<u64> = self
            .others
            .iter()
            .map(|s| (s.id, &s.status))
            .chain([(self.session_id, &self.status)])
            .filter(|(_, status)| **status == ConnStatus::Idle)
            .map(|(id, _)| id)
            .collect();
        for id in idle {
            self.with_session(id, App::connect);
        }
    }
}
