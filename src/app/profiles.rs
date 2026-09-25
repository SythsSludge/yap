//! Switching profiles and themes.

use super::*;

impl App {
    pub fn switch_profile(&mut self, name: &str) {
        match self.config.set_active(name) {
            Ok(()) => {
                self.prefs_ui.profile = self.config.profiles.iter().position(|p| p.name == name).unwrap_or(0);
                self.config_changed();
                let place = if self.session_count() > 1 { " in this chat" } else { "" };
                self.toast(Level::Success, format!("Using profile `{name}`{place}."));
            }
            Err(e) => self.toast(Level::Error, e.to_string()),
        }
    }

    /// First start: say hello and offer to set up a profile.
    pub fn welcome(&mut self) {
        self.modal = Some(Modal::Welcome);
    }

    /// From the welcome: straight to the first preference that still needs choosing.
    pub(super) fn start_setup(&mut self) {
        self.tab = Tab::Preferences;
        self.prefs_ui.pane = PrefsPane::Fields;
        if let Err(invalid) = self.config.active().preferences.validate() {
            self.prefs_ui.field = Field::ALL.iter().position(|f| *f == invalid.field()).unwrap_or(0);
        }
    }

    pub fn open_profile_picker(&mut self) {
        let selected = self.config.profiles.iter().position(|p| p.name == self.config.active_profile).unwrap_or(0);
        self.modal = Some(Modal::Profiles { selected });
    }

    pub fn open_theme_picker(&mut self) {
        let selected = self.themes.iter().position(|t| t.name == self.theme.name).unwrap_or(0);
        self.modal = Some(Modal::Themes { selected, original: self.theme.name.clone() });
    }

    /// Re-derive the displayed theme after a display setting (e.g. transparency) changes.
    pub fn refresh_theme(&mut self) {
        let name = self.theme.name.clone();
        self.theme = effective_theme(pick_theme(&self.themes, &name), self.config.settings.transparent_background);
    }

    /// Switch theme; `persist` is false while previewing in the picker.
    pub fn set_theme(&mut self, name: &str, persist: bool) {
        match self.themes.iter().find(|t| t.name == name) {
            Some(t) => {
                self.theme = effective_theme(t.clone(), self.config.settings.transparent_background);
                if persist {
                    self.config.settings.theme = name.to_owned();
                    self.config_changed();
                }
            }
            None => {
                let names: Vec<_> = self.themes.iter().map(|t| t.name.as_str()).collect();
                self.toast(Level::Error, format!("No theme `{name}`. Try: {}", names.join(", ")));
            }
        }
    }
}
