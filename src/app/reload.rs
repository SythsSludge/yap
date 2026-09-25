//! Picking up edits to `config.toml` and the themes folder while yap runs.

use super::*;
use std::path::Path;
use std::time::SystemTime;

const CHECK_EVERY: Duration = Duration::from_secs(1);

/// What the watched files looked like when we last read or wrote them.
#[derive(Debug, Clone, Default)]
pub struct Watch {
    config: Option<SystemTime>,
    themes: Option<(usize, SystemTime)>,
    next_check: Option<Instant>,
}

impl Watch {
    pub fn new(paths: &Paths) -> Self {
        Watch { config: modified(&paths.config_file), themes: themes_stamp(&paths.themes_dir), next_check: None }
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// How many theme files there are and the newest change among them. The folder's own
/// time covers files being removed.
fn themes_stamp(dir: &Path) -> Option<(usize, SystemTime)> {
    let mut newest = modified(dir)?;
    let mut count = 0;
    for path in std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()) {
        if path.extension().is_some_and(|x| x == "toml") {
            count += 1;
            newest = newest.max(modified(&path).unwrap_or(newest));
        }
    }
    Some((count, newest))
}

impl App {
    pub(super) fn tick_reload(&mut self) {
        if self.watch.next_check.is_some_and(|at| self.now < at) {
            return;
        }
        self.watch.next_check = Some(self.now + CHECK_EVERY);
        let config = modified(&self.paths.config_file);
        if config != self.watch.config {
            self.watch.config = config;
            if config.is_some() {
                self.reload_config();
            }
        }
        let themes = themes_stamp(&self.paths.themes_dir);
        if themes != self.watch.themes {
            self.watch.themes = themes;
            self.reload_themes();
        }
    }

    /// yap's own saves aren't outside edits.
    pub(super) fn config_saved(&mut self) {
        self.watch.config = modified(&self.paths.config_file);
    }

    fn reload_config(&mut self) {
        let (config, warnings) = match Config::load(&self.paths.config_file) {
            Ok(loaded) => loaded,
            Err(e) => return self.toast(Level::Error, format!("{e:#}. Keeping the current settings.")),
        };
        if config == self.config {
            return;
        }
        let old = std::mem::replace(&mut self.config, config);
        let (keymap, key_warnings) = Keymap::build(&self.config.settings.keys);
        self.keymap = keymap;
        // Only switch theme if the file's choice changed, so `--theme` survives other edits.
        if self.config.settings.theme != old.settings.theme {
            let name = self.config.settings.theme.clone();
            self.set_theme(&name, false);
        } else {
            self.refresh_theme();
        }
        self.prefs_ui.profile = self.prefs_ui.profile.min(self.config.profiles.len() - 1);
        for w in warnings.into_iter().chain(key_warnings) {
            self.toast(Level::Warning, w);
        }
        let mut note = "Reloaded config.toml.".to_owned();
        if self.config.settings.server_url != old.settings.server_url && self.server_override.is_none() {
            let how = self.keymap.hint(crate::keymap::Action::Reconnect).unwrap_or_else(|| "/reconnect".into());
            note.push_str(&format!(" The new server is used after you reconnect ({how})."));
        }
        self.toast(Level::Info, note);
    }

    fn reload_themes(&mut self) {
        let (themes, errors) = crate::theme::load_all(Some(&self.paths.themes_dir));
        self.themes = themes;
        self.refresh_theme();
        for e in errors {
            self.toast(Level::Warning, e);
        }
        self.toast(Level::Info, "Reloaded themes.");
    }
}
