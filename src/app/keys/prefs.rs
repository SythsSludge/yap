//! Keys for the Preferences tab: profiles, fields and options.

use super::*;

impl App {
    pub(super) fn prefs_key(&mut self, key: KeyEvent) {
        let ui = &mut self.prefs_ui;
        match key.code {
            KeyCode::Tab => {
                ui.pane = match ui.pane {
                    PrefsPane::Profiles => PrefsPane::Fields,
                    PrefsPane::Fields => PrefsPane::Options,
                    PrefsPane::Options => PrefsPane::Profiles,
                };
                return;
            }
            KeyCode::BackTab => {
                ui.pane = match ui.pane {
                    PrefsPane::Profiles => PrefsPane::Options,
                    PrefsPane::Fields => PrefsPane::Profiles,
                    PrefsPane::Options => PrefsPane::Fields,
                };
                return;
            }
            _ => {}
        }
        match self.prefs_ui.pane {
            PrefsPane::Profiles => self.profiles_key(key),
            PrefsPane::Fields => self.fields_key(key),
            PrefsPane::Options => self.options_key(key),
        }
    }

    fn selected_profile_name(&self) -> String {
        let i = self.prefs_ui.profile.min(self.config.profiles.len() - 1);
        self.config.profiles[i].name.clone()
    }

    fn profiles_key(&mut self, key: KeyEvent) {
        let len = self.config.profiles.len();
        if navigate(&mut self.prefs_ui.profile, len, &key, 5) {
            return;
        }
        let name = self.selected_profile_name();
        match key.code {
            KeyCode::Enter => {
                self.switch_profile(&name);
                self.prefs_ui.pane = PrefsPane::Fields;
            }
            KeyCode::Right | KeyCode::Char('l') => self.prefs_ui.pane = PrefsPane::Fields,
            KeyCode::Char('n') => self.open_prompt("New profile name", "", PromptAction::NewProfile),
            KeyCode::Char('m') => {
                let current =
                    self.config.profiles[self.prefs_ui.profile.min(self.config.profiles.len() - 1)].character.clone();
                self.open_prompt(
                    &format!("Character name for `{name}` (empty: \"you\")"),
                    &current,
                    PromptAction::CharacterName(name),
                );
            }
            KeyCode::Char('c') => {
                let suggestion = self.config.unique_profile_name(&format!("{name} copy"));
                self.open_prompt(&format!("Copy `{name}` as"), &suggestion, PromptAction::CloneProfile(name));
            }
            KeyCode::Char('r') => {
                self.open_prompt(&format!("Rename `{name}` to"), &name.clone(), PromptAction::RenameProfile(name))
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                self.modal = Some(Modal::Confirm {
                    text: format!("Delete profile `{name}`?"),
                    action: Confirm::DeleteProfile(name),
                });
            }
            KeyCode::Char('e') => {
                let path = self.default_path(&format!("yap-{}.toml", sanitize_file_name(&name)));
                self.open_prompt(
                    &format!("Export `{name}` to (.toml or .json)"),
                    &path,
                    PromptAction::ExportProfile(name),
                );
            }
            KeyCode::Char('E') => self.activate_setting(Row::ExportAll),
            KeyCode::Char('i') => self.activate_setting(Row::ImportProfiles),
            KeyCode::Char('I') => self.activate_setting(Row::ImportAll),
            _ => {}
        }
    }

    fn current_field(&self) -> Field {
        Field::ALL[self.prefs_ui.field.min(Field::ALL.len() - 1)]
    }

    fn fields_key(&mut self, key: KeyEvent) {
        if navigate(&mut self.prefs_ui.field, Field::ALL.len(), &key, 8) {
            return;
        }
        let field = self.current_field();
        match key.code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
                self.prefs_ui.filter.clear();
                // Start on the first selected option so single choices are easy to change.
                let prefs = &self.config.active().preferences;
                self.prefs_ui.option =
                    prefs_options(field, "").iter().position(|o| prefs.is_selected(field, o)).unwrap_or(0);
                self.prefs_ui.pane = PrefsPane::Options;
            }
            KeyCode::Left | KeyCode::Char('h') => self.prefs_ui.pane = PrefsPane::Profiles,
            KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                self.config.active_mut().preferences.clear(field);
                self.config_changed();
            }
            _ => {}
        }
    }

    fn options_key(&mut self, key: KeyEvent) {
        let field = self.current_field();
        let options = prefs_options(field, &self.prefs_ui.filter);
        let ui = &mut self.prefs_ui;
        match key.code {
            KeyCode::Up => step(&mut ui.option, options.len(), -1),
            KeyCode::Down => step(&mut ui.option, options.len(), 1),
            KeyCode::PageUp => step(&mut ui.option, options.len(), -10),
            KeyCode::PageDown => step(&mut ui.option, options.len(), 10),
            KeyCode::Home => ui.option = 0,
            KeyCode::End => ui.option = options.len().saturating_sub(1),
            KeyCode::Esc | KeyCode::Left if !ui.filter.is_empty() && key.code == KeyCode::Esc => {
                ui.filter.clear();
                ui.option = 0;
            }
            KeyCode::Esc | KeyCode::Left => ui.pane = PrefsPane::Fields,
            KeyCode::Backspace => {
                ui.filter.pop();
                ui.option = 0;
            }
            // Space toggles unless it's part of a filter like "snow leopard".
            KeyCode::Enter | KeyCode::Char(' ') if key.code == KeyCode::Enter || ui.filter.is_empty() => {
                let Some(&value) = options.get(ui.option) else { return };
                self.config.active_mut().preferences.toggle(field, value);
                self.config_changed();
                if !field.is_multi() {
                    self.prefs_ui.pane = PrefsPane::Fields;
                    self.prefs_ui.filter.clear();
                    step(&mut self.prefs_ui.field, Field::ALL.len(), 1);
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                ui.filter.push(c);
                ui.option = 0;
            }
            _ => {}
        }
    }
}
