//! Keyboard, mouse and paste handling.

use super::modal::{Confirm, Modal, PromptAction};
use super::settings::{self, Row};
use super::{App, ChatFocus, Level, ListUi, PrefsPane, Tab};
use crate::catalog::ANY;
use crate::drawer::{parse_link, suggest_label};
use crate::input::LineEditor;
use crate::prefs::{Field, Preferences};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};

/// Options offered for a preference field under a filter. "Any / All" comes first
/// where the field allows it.
pub fn prefs_options(field: Field, filter: &str) -> Vec<&'static str> {
    let f = filter.to_lowercase();
    let mut out = Vec::new();
    if field.allows_any() && (f.is_empty() || "any / all".contains(&f)) {
        out.push(ANY);
    }
    out.extend(field.catalog().options().iter().copied().filter(|o| o.to_lowercase().contains(&f)));
    out
}

/// Move a list selection, clamped to `len`.
fn step(selected: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        *selected = 0;
    } else {
        *selected = selected.saturating_add_signed(delta).min(len - 1);
    }
}

/// Shared list navigation. Returns true if the key was a navigation key.
fn navigate(selected: &mut usize, len: usize, key: &KeyEvent, page: usize) -> bool {
    let page = page.max(1) as isize;
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => step(selected, len, -1),
        KeyCode::Down | KeyCode::Char('j') => step(selected, len, 1),
        KeyCode::PageUp => step(selected, len, -page),
        KeyCode::PageDown => step(selected, len, page),
        KeyCode::Home | KeyCode::Char('g') => *selected = 0,
        KeyCode::End | KeyCode::Char('G') => *selected = len.saturating_sub(1),
        _ => return false,
    }
    true
}

/// Edit a filter string in place. Returns true when filtering should stop.
fn edit_filter(list: &mut ListUi, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            list.filter.clear();
            list.selected = 0;
            true
        }
        KeyCode::Enter | KeyCode::Down | KeyCode::Up => true,
        KeyCode::Backspace => {
            list.filter.pop();
            list.selected = 0;
            false
        }
        KeyCode::Char(c) => {
            list.filter.push(c);
            list.selected = 0;
            false
        }
        _ => false,
    }
}

/// Shared editing keys for a single-line editor. Returns true if handled.
fn edit_line(editor: &mut LineEditor, key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Char('w') if ctrl => editor.delete_word_back(),
        KeyCode::Char('u') if ctrl => editor.delete_to_start(),
        KeyCode::Backspace if ctrl || alt => editor.delete_word_back(),
        KeyCode::Char(c) if !ctrl && !alt => editor.insert_char(c),
        KeyCode::Backspace => editor.backspace(),
        KeyCode::Delete => editor.delete(),
        KeyCode::Left if ctrl || alt => editor.word_left(),
        KeyCode::Right if ctrl || alt => editor.word_right(),
        KeyCode::Left => editor.left(),
        KeyCode::Right => editor.right(),
        KeyCode::Home => editor.home(),
        KeyCode::End => editor.end(),
        _ => return false,
    }
    true
}

impl App {
    pub fn on_terminal(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) => self.on_paste(&text),
            Event::Mouse(mouse) => self.on_mouse(mouse),
            Event::FocusGained => self.set_focused(true),
            Event::FocusLost => self.set_focused(false),
            _ => {}
        }
    }

    fn on_paste(&mut self, text: &str) {
        if let Some(Modal::Prompt(p)) = &mut self.modal {
            p.editor.insert_str(text);
        } else if self.modal.is_none() && self.tab == Tab::Chat {
            self.chat_focus = ChatFocus::Input;
            self.input.insert_str(text);
            self.input_changed();
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        let delta: isize = match mouse.kind {
            MouseEventKind::ScrollUp => -3,
            MouseEventKind::ScrollDown => 3,
            _ => return,
        };
        if self.modal.is_some() || self.viewer.is_some() {
            return;
        }
        match self.tab {
            Tab::Chat if delta < 0 => self.chat.scroll_up(3),
            Tab::Chat => self.chat.scroll_down(3),
            Tab::Traffic => {
                let len = self.traffic_len();
                self.traffic_ui.follow = false;
                step(&mut self.traffic_ui.list.selected, len, delta.signum());
            }
            Tab::Drawer => {
                let len = self.drawer.filtered(&self.drawer_ui.filter).len();
                step(&mut self.drawer_ui.selected, len, delta.signum());
            }
            Tab::Settings => {
                let len = settings::rows(&self.config.settings).len();
                step(&mut self.settings_ui.selected, len, delta.signum());
            }
            Tab::Preferences => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if self.viewer.is_some() {
            return self.viewer_key(key);
        }
        if ctrl && key.code == KeyCode::Char('c') {
            if self.modal.take().is_some() {
                return self.restore_theme_if_previewing();
            }
            return self.request_quit();
        }
        if let Some(modal) = self.modal.take() {
            return self.modal_key(modal, key);
        }
        if self.global_key(key) {
            return;
        }
        match self.tab {
            Tab::Chat => self.chat_key(key),
            Tab::Preferences => self.prefs_key(key),
            Tab::Drawer => self.drawer_key(key),
            Tab::Traffic => self.traffic_key(key),
            Tab::Settings => self.settings_key(key),
        }
    }

    fn restore_theme_if_previewing(&mut self) {
        let original = self.config.settings.theme.clone();
        if self.theme.name != original {
            self.set_theme(&original, false);
        }
    }

    fn global_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let tab = match key.code {
            KeyCode::F(n @ 2..=6) => Some(Tab::ALL[n as usize - 2]),
            KeyCode::Char(c @ '1'..='5') if alt => Some(Tab::ALL[c as usize - '1' as usize]),
            _ => None,
        };
        if let Some(tab) = tab {
            self.tab = tab;
            return true;
        }
        match key.code {
            KeyCode::F(1) => self.modal = Some(Modal::Help { scroll: 0 }),
            KeyCode::Char('f') if ctrl => self.request_find(),
            KeyCode::Char('d') if ctrl => self.request_leave(),
            KeyCode::Char('b') if ctrl => self.request_block(),
            KeyCode::Char('r') if ctrl => {
                self.traffic_note("manual reconnect");
                self.run_command(crate::commands::Command::Reconnect);
            }
            KeyCode::Char('p') if ctrl => self.open_profile_picker(),
            KeyCode::Char('t') if ctrl => self.open_theme_picker(),
            KeyCode::Char('o') if ctrl => self.open_links(),
            KeyCode::Char('e') if ctrl => self.toggle_drawer_panel(),
            KeyCode::Char('s') if ctrl => {
                self.config.settings.show_sidebar ^= true;
                self.config_changed();
            }
            KeyCode::Char('q') if ctrl => self.request_quit(),
            _ => return false,
        }
        true
    }

    // ----- chat -----------------------------------------------------------------

    fn chat_key(&mut self, key: KeyEvent) {
        if self.chat_focus == ChatFocus::Drawer && self.drawer_panel {
            return self.chat_drawer_key(key);
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => return self.submit_input(),
            KeyCode::PageUp => return self.chat.scroll_up(self.chat.page()),
            KeyCode::PageDown => return self.chat.scroll_down(self.chat.page()),
            KeyCode::Up if shift || ctrl => return self.chat.scroll_up(1),
            KeyCode::Down if shift || ctrl => return self.chat.scroll_down(1),
            KeyCode::End if ctrl => return self.chat.follow(),
            KeyCode::Up => {
                self.input.history_prev();
                return;
            }
            KeyCode::Down => {
                self.input.history_next();
                return;
            }
            KeyCode::Esc => return self.chat.follow(),
            KeyCode::Tab if self.drawer_panel => {
                self.chat_focus = ChatFocus::Drawer;
                return;
            }
            _ => {}
        }
        let before = self.input.text().to_owned();
        edit_line(&mut self.input, &key);
        if self.input.text() != before {
            self.input_changed();
        }
    }

    fn chat_drawer_key(&mut self, key: KeyEvent) {
        let items = self.drawer.filtered("");
        if navigate(&mut self.drawer_ui.selected, items.len(), &key, 5) {
            return;
        }
        let selected = items.get(self.drawer_ui.selected).map(|&i| self.drawer.items[i].url.clone());
        match (key.code, selected) {
            (KeyCode::Enter | KeyCode::Char('i'), Some(url)) => self.insert_into_input(&url),
            (KeyCode::Char('o'), Some(url)) => self.open_url(&url),
            (KeyCode::Char('y'), Some(url)) => self.copy(&url),
            (KeyCode::Char('p'), Some(url)) => self.preview(&url),
            (KeyCode::Esc | KeyCode::Tab, _) => self.chat_focus = ChatFocus::Input,
            _ => {}
        }
    }

    // ----- preferences ----------------------------------------------------------

    fn prefs_key(&mut self, key: KeyEvent) {
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

    // ----- drawer ---------------------------------------------------------------

    /// The drawer item index under the selection, respecting the filter.
    pub fn selected_drawer_item(&self) -> Option<usize> {
        self.drawer.filtered(&self.drawer_ui.filter).get(self.drawer_ui.selected).copied()
    }

    fn drawer_key(&mut self, key: KeyEvent) {
        if self.drawer_ui.filtering {
            if edit_filter(&mut self.drawer_ui, &key) {
                self.drawer_ui.filtering = false;
            }
            return;
        }
        let len = self.drawer.filtered(&self.drawer_ui.filter).len();
        if navigate(&mut self.drawer_ui.selected, len, &key, 10) {
            return;
        }
        if key.code == KeyCode::Char('a') {
            return self.open_prompt("Link to save", "https://", PromptAction::DrawerAddUrl);
        }
        if key.code == KeyCode::Char('/') {
            self.drawer_ui.filtering = true;
            return;
        }
        if key.code == KeyCode::Esc {
            self.drawer_ui.filter.clear();
            return;
        }
        let Some(index) = self.selected_drawer_item() else { return };
        let item = self.drawer.items[index].clone();
        match key.code {
            KeyCode::Enter | KeyCode::Char('o') => self.open_url(&item.url),
            KeyCode::Char('y') => self.copy(&item.url),
            KeyCode::Char('i') => self.insert_into_input(&item.url),
            KeyCode::Char('p') => self.preview(&item.url),
            KeyCode::Char('e') | KeyCode::Char('r') => {
                self.open_prompt("Label", &item.label, PromptAction::DrawerEditLabel(index));
            }
            KeyCode::Char('n') => self.open_prompt("Note", &item.note, PromptAction::DrawerEditNote(index)),
            KeyCode::Char('d') | KeyCode::Delete => {
                self.modal = Some(Modal::Confirm {
                    text: format!("Remove `{}` from the drawer?", item.label),
                    action: Confirm::DeleteDrawerItem(index),
                });
            }
            KeyCode::Char(c @ ('J' | 'K')) if self.drawer_ui.filter.is_empty() => {
                self.drawer_ui.selected = self.drawer.shift(index, if c == 'J' { 1 } else { -1 });
                self.drawer_changed();
            }
            _ => {}
        }
    }

    // ----- traffic --------------------------------------------------------------

    pub fn traffic_len(&self) -> usize {
        self.traffic.visible(&self.traffic_ui.list.filter, self.config.settings.traffic.hide_heartbeat).count()
    }

    /// The selected traffic row, accounting for follow mode.
    pub fn traffic_selected(&self) -> Option<usize> {
        let len = self.traffic_len();
        if len == 0 {
            None
        } else if self.traffic_ui.follow {
            Some(len - 1)
        } else {
            Some(self.traffic_ui.list.selected.min(len - 1))
        }
    }

    fn traffic_key(&mut self, key: KeyEvent) {
        if self.traffic_ui.list.filtering {
            if edit_filter(&mut self.traffic_ui.list, &key) {
                self.traffic_ui.list.filtering = false;
            }
            return;
        }
        let len = self.traffic_len();
        if self.traffic_ui.follow {
            self.traffic_ui.list.selected = len.saturating_sub(1);
        }
        let mut selected = self.traffic_ui.list.selected;
        if navigate(&mut selected, len, &key, 10) {
            self.traffic_ui.list.selected = selected;
            self.traffic_ui.follow = selected + 1 >= len;
            return;
        }
        match key.code {
            KeyCode::Char('f') => self.traffic_ui.follow ^= true,
            KeyCode::Char('p') => self.traffic_ui.pretty ^= true,
            KeyCode::Char('h') => {
                self.config.settings.traffic.hide_heartbeat ^= true;
                self.config_changed();
            }
            KeyCode::Char('/') => self.traffic_ui.list.filtering = true,
            KeyCode::Esc => self.traffic_ui.list.filter.clear(),
            KeyCode::Char('c') => {
                self.traffic.clear();
                self.traffic_ui.follow = true;
            }
            KeyCode::Char('e') => {
                let path =
                    self.default_path(&format!("yap-traffic-{}.jsonl", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                self.open_prompt("Export traffic (JSON Lines) to", &path, PromptAction::TrafficExport);
            }
            KeyCode::Char('s') => {
                self.open_prompt("Send raw frame", r#"{"type":"","data":true}"#, PromptAction::RawFrame);
            }
            KeyCode::Char('y') => {
                let body = self.traffic_selected().and_then(|i| {
                    self.traffic
                        .visible(&self.traffic_ui.list.filter, self.config.settings.traffic.hide_heartbeat)
                        .nth(i)
                        .map(|e| e.body.clone())
                });
                if let Some(body) = body {
                    self.copy(&body);
                }
            }
            _ => {}
        }
    }

    // ----- settings -------------------------------------------------------------

    fn settings_key(&mut self, key: KeyEvent) {
        let rows = settings::rows(&self.config.settings);
        let mut selected = self.settings_ui.selected;
        // h/l adjust values here, so only arrows and j/k navigate.
        if !matches!(key.code, KeyCode::Char('g' | 'G')) && navigate(&mut selected, rows.len(), &key, 10) {
            self.settings_ui.selected = selected;
            return;
        }
        let Some(&row) = rows.get(self.settings_ui.selected) else { return };
        match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_setting(row),
            KeyCode::Left | KeyCode::Char('h') => self.adjust_setting(row, -1),
            KeyCode::Right | KeyCode::Char('l') => self.adjust_setting(row, 1),
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Row::Domain(i) = row {
                    self.remove_domain(i);
                    let len = settings::rows(&self.config.settings).len();
                    self.settings_ui.selected = self.settings_ui.selected.min(len - 1);
                }
            }
            _ => {}
        }
    }

    // ----- modals ---------------------------------------------------------------

    fn viewer_key(&mut self, key: KeyEvent) {
        let url = self.viewer.as_ref().map(|v| v.url.clone()).unwrap_or_default();
        match key.code {
            KeyCode::Char('o') => self.open_url(&url),
            KeyCode::Char('y') => self.copy(&url),
            KeyCode::Char('s') => {
                self.viewer = None;
                let label = suggest_label(&url);
                self.open_prompt("Label", &label, PromptAction::DrawerAddLabel { url });
            }
            _ => self.viewer = None,
        }
    }

    /// Handle a key for the (already taken) modal; put it back if it stays open.
    fn modal_key(&mut self, modal: Modal, key: KeyEvent) {
        let keep = match modal {
            Modal::Confirm { text, action } => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                    self.run_confirmed(action);
                    None
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => None,
                _ => Some(Modal::Confirm { text, action }),
            },
            Modal::Prompt(mut prompt) => match key.code {
                KeyCode::Enter => {
                    let text = prompt.editor.text().trim().to_owned();
                    self.submit_prompt(prompt.action, text);
                    return;
                }
                KeyCode::Esc => None,
                _ => {
                    edit_line(&mut prompt.editor, &key);
                    Some(Modal::Prompt(prompt))
                }
            },
            Modal::Help { scroll } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => Some(Modal::Help { scroll: scroll.saturating_sub(1) }),
                KeyCode::Down | KeyCode::Char('j') => Some(Modal::Help { scroll: scroll.saturating_add(1) }),
                KeyCode::PageUp => Some(Modal::Help { scroll: scroll.saturating_sub(10) }),
                KeyCode::PageDown => Some(Modal::Help { scroll: scroll.saturating_add(10) }),
                _ => None,
            },
            Modal::Links { links, mut selected } => {
                if navigate(&mut selected, links.len(), &key, 5) {
                    Some(Modal::Links { links, selected })
                } else {
                    let url = links[selected.min(links.len() - 1)].url.clone();
                    match key.code {
                        KeyCode::Enter | KeyCode::Char('o') => self.open_url(&url),
                        KeyCode::Char('y') => self.copy(&url),
                        KeyCode::Char('i') => self.insert_into_input(&url),
                        KeyCode::Char('p') => self.preview(&url),
                        KeyCode::Char('s') => {
                            let label = suggest_label(&url);
                            self.open_prompt("Label for the drawer", &label, PromptAction::DrawerAddLabel { url });
                        }
                        KeyCode::Char('t') => {
                            if let Some(host) = url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_owned))
                            {
                                self.trust_domain(&host);
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('q') => {}
                        _ => return self.modal = Some(Modal::Links { links, selected }),
                    }
                    // Actions that open their own popup have already set `self.modal`.
                    return;
                }
            }
            Modal::Profiles { mut selected } => {
                let len = self.config.profiles.len();
                if navigate(&mut selected, len, &key, 5) {
                    Some(Modal::Profiles { selected })
                } else {
                    if key.code == KeyCode::Enter {
                        let name = self.config.profiles[selected.min(len - 1)].name.clone();
                        self.switch_profile(&name);
                    }
                    None
                }
            }
            Modal::Themes { mut selected, original } => {
                if navigate(&mut selected, self.themes.len(), &key, 5) {
                    let name = self.themes[selected].name.clone();
                    self.set_theme(&name, false);
                    Some(Modal::Themes { selected, original })
                } else {
                    match key.code {
                        KeyCode::Enter => {
                            let name = self.theme.name.clone();
                            self.set_theme(&name, true);
                        }
                        _ => self.set_theme(&original, false),
                    }
                    None
                }
            }
        };
        if self.modal.is_none() {
            self.modal = keep;
        }
    }

    fn submit_prompt(&mut self, action: PromptAction, text: String) {
        match action {
            PromptAction::NewProfile => match self.config.create_profile(&text, Preferences::default()) {
                Ok(name) => {
                    self.switch_profile(&name);
                    self.prefs_ui.pane = PrefsPane::Fields;
                }
                Err(e) => self.toast(Level::Error, e.to_string()),
            },
            PromptAction::CloneProfile(from) => {
                let prefs = self.config.profiles.iter().find(|p| p.name == from).map(|p| p.preferences.clone());
                match self.config.create_profile(&text, prefs.unwrap_or_default()) {
                    Ok(name) => self.switch_profile(&name),
                    Err(e) => self.toast(Level::Error, e.to_string()),
                }
            }
            PromptAction::RenameProfile(from) => match self.config.rename_profile(&from, &text) {
                Ok(()) => self.config_changed(),
                Err(e) => self.toast(Level::Error, e.to_string()),
            },
            PromptAction::ExportProfile(name) => self.export(Some(&name), &text),
            PromptAction::ExportAll => self.export(None, &text),
            PromptAction::Import { with_settings } => self.import(&text, with_settings),
            PromptAction::DrawerAddUrl => match parse_link(&text) {
                Ok(url) => {
                    let label = suggest_label(&url);
                    self.open_prompt("Label", &label, PromptAction::DrawerAddLabel { url });
                }
                Err(e) => self.toast(Level::Error, e.to_string()),
            },
            PromptAction::DrawerAddLabel { url } => self.add_to_drawer(&url, &text),
            PromptAction::DrawerEditLabel(i) => {
                if let Some(item) = self.drawer.items.get_mut(i).filter(|_| !text.is_empty()) {
                    item.label = text;
                    self.drawer_changed();
                }
            }
            PromptAction::DrawerEditNote(i) => {
                if let Some(item) = self.drawer.items.get_mut(i) {
                    item.note = text;
                    self.drawer_changed();
                }
            }
            PromptAction::AddTrustedDomain => self.trust_domain(&text),
            PromptAction::ServerUrl => match crate::net::websocket_url(&text) {
                Ok(url) => {
                    self.config.settings.server_url = url.to_string();
                    self.config_changed();
                    self.connect();
                }
                Err(e) => self.toast(Level::Error, e),
            },
            PromptAction::RawFrame => self.send_raw(text),
            PromptAction::TrafficExport => self.export_traffic(&text),
        }
    }
}

fn sanitize_file_name(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}
