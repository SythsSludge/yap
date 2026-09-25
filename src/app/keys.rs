//! Keyboard, mouse and paste handling.

use super::modal::{Confirm, Modal, PromptAction};
use super::settings::{self, Row};
use super::{App, ChatFocus, Level, ListUi, PrefsPane, Shelf, Tab};
use crate::catalog::ANY;
use crate::drawer::{parse_link, suggest_label};
use crate::input::LineEditor;
use crate::keymap::{Action, Chord};
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
            MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) => {
                return self.on_click(mouse.column, mouse.row);
            }
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
                let len = self.drawer_items().len();
                step(&mut self.drawer_ui.list.selected, len, delta.signum());
            }
            Tab::Logs if self.logs_ui.reading => {
                self.logs_ui.scroll = self.logs_ui.scroll.saturating_add_signed(delta);
            }
            Tab::Logs => {
                let len = self.visible_logs().len();
                step(&mut self.logs_ui.list.selected, len, delta.signum());
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
            Tab::Logs => self.logs_key(key),
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

    /// Rebindable global shortcuts (see `keymap.rs`).
    fn global_key(&mut self, key: KeyEvent) -> bool {
        let Some(action) = self.keymap.action(&key) else { return false };
        if action.chat_only() && self.tab != Tab::Chat {
            return false;
        }
        self.run_action(action);
        true
    }

    pub fn run_action(&mut self, action: Action) {
        let page = self.chat.page();
        match action {
            Action::Help => self.modal = Some(Modal::Help { scroll: 0 }),
            Action::Find => self.request_find(),
            Action::Next => self.next_partner(),
            Action::Snippets => self.open_snippet_picker(),
            Action::Editor => self.compose_in_editor(),
            Action::Leave => self.request_leave(),
            Action::Block => self.request_block(),
            Action::Links => self.open_links(),
            Action::Drawer => self.toggle_drawer_panel(),
            Action::Profile => self.open_profile_picker(),
            Action::Theme => self.open_theme_picker(),
            Action::Sidebar => {
                self.config.settings.show_sidebar ^= true;
                self.config_changed();
            }
            Action::Reconnect => {
                self.traffic_note("manual reconnect");
                self.run_command(crate::commands::Command::Reconnect);
            }
            Action::Quit => self.request_quit(),
            Action::NewChat => self.new_session(),
            Action::CloseChat => self.request_close_session(),
            Action::NextChat => self.cycle_session(1),
            Action::PrevChat => self.cycle_session(-1),
            Action::TabChat => self.tab = Tab::Chat,
            Action::TabPreferences => self.tab = Tab::Preferences,
            Action::TabDrawer => self.tab = Tab::Drawer,
            Action::TabLogs => self.tab = Tab::Logs,
            Action::TabTraffic => self.tab = Tab::Traffic,
            Action::TabSettings => self.tab = Tab::Settings,
            Action::ScrollPageUp => self.chat.scroll_up(page),
            Action::ScrollPageDown => self.chat.scroll_down(page),
            Action::ScrollUp => self.chat.scroll_up(1),
            Action::ScrollDown => self.chat.scroll_down(1),
            Action::JumpToNewest => self.chat.follow(),
        }
    }

    // ----- chat -----------------------------------------------------------------

    fn chat_key(&mut self, key: KeyEvent) {
        if self.chat_focus == ChatFocus::Drawer && self.drawer_panel {
            return self.chat_drawer_key(key);
        }
        // Scrolling is handled by the (rebindable) global keys; Esc always jumps down.
        let modified = key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Enter => return self.submit_input(),
            KeyCode::Up | KeyCode::Down if modified => return,
            KeyCode::Up => {
                self.input.history_prev();
                return;
            }
            KeyCode::Down => {
                self.input.history_next();
                return;
            }
            KeyCode::Esc => {
                if !self.cancel_requeue() {
                    self.chat.follow();
                }
                return;
            }
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
        let items = self.drawer.filtered("", None);
        if navigate(&mut self.drawer_ui.list.selected, items.len(), &key, 5) {
            return;
        }
        let selected = items.get(self.drawer_ui.list.selected).map(|&i| self.drawer.items[i].url.clone());
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

    /// Drawer items shown in the Drawer tab, after the text filter and tag filter.
    pub fn drawer_items(&self) -> Vec<usize> {
        self.drawer.filtered(&self.drawer_ui.list.filter, self.drawer_ui.tag.as_deref())
    }

    /// The drawer item index under the selection, respecting the filters.
    pub fn selected_drawer_item(&self) -> Option<usize> {
        self.drawer_items().get(self.drawer_ui.list.selected).copied()
    }

    /// Step the tag filter through: all → each tag → all.
    fn cycle_drawer_tag(&mut self, delta: isize) {
        let tags: Vec<String> = self.drawer.tags().into_iter().map(|(t, _)| t).collect();
        let current = self.drawer_ui.tag.as_ref().and_then(|t| tags.iter().position(|x| x == t));
        // Position 0 is "all", tags follow.
        let len = tags.len() as isize + 1;
        let pos = current.map_or(0, |i| i as isize + 1);
        let next = (pos + delta).rem_euclid(len);
        self.drawer_ui.tag = (next > 0).then(|| tags[next as usize - 1].clone());
        self.drawer_ui.list.selected = 0;
    }

    fn drawer_key(&mut self, key: KeyEvent) {
        let filtering = self.drawer_ui.list.filtering || self.drawer_ui.snippets.filtering;
        if !filtering {
            match key.code {
                KeyCode::Char('s') => {
                    self.drawer_ui.shelf = match self.drawer_ui.shelf {
                        Shelf::Links => Shelf::Snippets,
                        Shelf::Snippets => Shelf::Links,
                    };
                    return;
                }
                KeyCode::Char('E') => {
                    let path = self.default_path("yap-drawer.toml");
                    return self.open_prompt(
                        "Export links and snippets to (.toml or .json)",
                        &path,
                        PromptAction::DrawerExport,
                    );
                }
                KeyCode::Char('I') => {
                    return self.open_prompt("Import links and snippets from", "", PromptAction::DrawerImport);
                }
                _ => {}
            }
        }
        if self.drawer_ui.shelf == Shelf::Snippets {
            return self.snippets_key(key);
        }
        if self.drawer_ui.list.filtering {
            if edit_filter(&mut self.drawer_ui.list, &key) {
                self.drawer_ui.list.filtering = false;
            }
            return;
        }
        let len = self.drawer_items().len();
        if navigate(&mut self.drawer_ui.list.selected, len, &key, 10) {
            return;
        }
        match key.code {
            KeyCode::Char('a') => {
                return self.open_prompt("Link to save", "https://", PromptAction::DrawerAddUrl);
            }
            KeyCode::Char('/') => {
                self.drawer_ui.list.filtering = true;
                return;
            }
            KeyCode::Char(']') | KeyCode::Tab => return self.cycle_drawer_tag(1),
            KeyCode::Char('[') | KeyCode::BackTab => return self.cycle_drawer_tag(-1),
            KeyCode::Esc => {
                self.drawer_ui.list.filter.clear();
                self.drawer_ui.tag = None;
                return;
            }
            _ => {}
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
            KeyCode::Char('t') => {
                let tags = item.tags.join(" ");
                self.open_prompt("Tags (space or comma separated)", &tags, PromptAction::DrawerEditTags(index));
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                self.modal = Some(Modal::Confirm {
                    text: format!("Remove `{}` from the drawer?", item.label),
                    action: Confirm::DeleteDrawerItem(index),
                });
            }
            KeyCode::Char(c @ ('J' | 'K')) if self.drawer_ui.list.filter.is_empty() && self.drawer_ui.tag.is_none() => {
                self.drawer_ui.list.selected = self.drawer.shift(index, if c == 'J' { 1 } else { -1 });
                self.drawer_changed();
            }
            _ => {}
        }
    }

    pub fn visible_snippets(&self) -> Vec<usize> {
        self.drawer.filtered_snippets(&self.drawer_ui.snippets.filter)
    }

    fn snippets_key(&mut self, key: KeyEvent) {
        if self.drawer_ui.snippets.filtering {
            if edit_filter(&mut self.drawer_ui.snippets, &key) {
                self.drawer_ui.snippets.filtering = false;
            }
            return;
        }
        let visible = self.visible_snippets();
        if navigate(&mut self.drawer_ui.snippets.selected, visible.len(), &key, 10) {
            return;
        }
        match key.code {
            KeyCode::Char('a') => {
                return self.open_prompt("Snippet name (used as /snip <name>)", "", PromptAction::SnippetName);
            }
            KeyCode::Char('/') => {
                self.drawer_ui.snippets.filtering = true;
                return;
            }
            KeyCode::Esc => return self.drawer_ui.snippets.filter.clear(),
            _ => {}
        }
        let Some(&index) = visible.get(self.drawer_ui.snippets.selected) else { return };
        let snippet = self.drawer.snippets[index].clone();
        match key.code {
            KeyCode::Enter | KeyCode::Char('i') => self.insert_snippet(index),
            KeyCode::Char('e') => self.edit_snippet_in_editor(index),
            KeyCode::Char('r') => self.open_prompt("Rename snippet", &snippet.name, PromptAction::SnippetRename(index)),
            KeyCode::Char('y') => self.copy(&snippet.text),
            KeyCode::Char('d') | KeyCode::Delete => {
                self.modal = Some(Modal::Confirm {
                    text: format!("Delete the snippet `{}`?", snippet.name),
                    action: Confirm::DeleteSnippet(index),
                });
            }
            _ => {}
        }
    }

    // ----- logs -----------------------------------------------------------------

    fn logs_key(&mut self, key: KeyEvent) {
        if self.logs_ui.list.filtering {
            if edit_filter(&mut self.logs_ui.list, &key) {
                self.logs_ui.list.filtering = false;
            }
            // Searching looks inside messages, so older chats need loading.
            if self.logs_ui.list.filter.chars().count() >= 2 {
                self.logs.load_all();
            }
            return;
        }
        if self.logs_ui.reading {
            let page = 10;
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.logs_ui.scroll = self.logs_ui.scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => self.logs_ui.scroll += 1,
                KeyCode::PageUp => self.logs_ui.scroll = self.logs_ui.scroll.saturating_sub(page),
                KeyCode::PageDown | KeyCode::Char(' ') => self.logs_ui.scroll += page,
                KeyCode::Home | KeyCode::Char('g') => self.logs_ui.scroll = 0,
                KeyCode::End | KeyCode::Char('G') => self.logs_ui.scroll = usize::MAX,
                KeyCode::Esc | KeyCode::Left | KeyCode::Char('h' | 'q') => self.logs_ui.reading = false,
                _ => {}
            }
            return;
        }
        let len = self.visible_logs().len();
        let before = self.logs_ui.list.selected;
        if navigate(&mut self.logs_ui.list.selected, len, &key, 10) {
            if self.logs_ui.list.selected != before {
                self.logs_ui.scroll = 0;
            }
            return;
        }
        match key.code {
            KeyCode::Char('/') => self.logs_ui.list.filtering = true,
            KeyCode::Esc => self.logs_ui.list.filter.clear(),
            _ => {}
        }
        let Some(index) = self.selected_log() else { return };
        match key.code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if let Err(e) = self.logs.open(index) {
                    return self.toast(Level::Error, format!("Couldn't open chat: {e:#}"));
                }
                self.logs_ui.reading = true;
            }
            KeyCode::Char('e') => {
                let started = self.logs.items[index].started.format("%Y%m%d-%H%M%S");
                let path = self.default_path(&format!("yap-chat-{started}.txt"));
                self.open_prompt("Save transcript to", &path, PromptAction::ExportLog(index));
            }
            KeyCode::Char('p') => {
                let pinned = self.logs.toggle_pin(index);
                self.toast(Level::Info, if pinned { "Pinned." } else { "Unpinned." });
                // Keep the same chat selected as it moves.
                self.logs_ui.list.selected = self.visible_logs().iter().position(|&i| i == index).unwrap_or(0);
            }
            KeyCode::Char('r') => {
                let name = self.logs.items[index].name.clone().unwrap_or_default();
                self.open_prompt("Name this chat (empty clears)", &name, PromptAction::LogRename(index));
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                let title = self.logs.items[index].title();
                self.modal = Some(Modal::Confirm {
                    text: format!("Delete the chat with {title}? This removes its log file too."),
                    action: Confirm::DeleteLog(index),
                });
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
            KeyCode::Char('d') | KeyCode::Delete if matches!(row, Row::Domain(_)) => {
                if let Row::Domain(i) = row {
                    self.remove_domain(i);
                    let len = settings::rows(&self.config.settings).len();
                    self.settings_ui.selected = self.settings_ui.selected.min(len - 1);
                }
            }
            KeyCode::Backspace | KeyCode::Delete if matches!(row, Row::Key(_)) => {
                if let Row::Key(action) = row {
                    self.keymap.reset(action);
                    self.keymap_changed();
                }
            }
            KeyCode::Char('x') if matches!(row, Row::Key(_)) => {
                if let Row::Key(action) = row {
                    self.keymap.unbind(action);
                    self.keymap_changed();
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
            KeyCode::Char('d') => self.save_image(&url),
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
            Modal::CaptureKey { action } => match key.code {
                KeyCode::Esc => None,
                // Bare modifier presses (kitty's enhanced keyboard mode) aren't keys yet.
                KeyCode::Modifier(_) => Some(Modal::CaptureKey { action }),
                _ => {
                    let chord = Chord::from_event(&key);
                    match chord.reserved() {
                        Some(why) => {
                            self.toast(
                                Level::Warning,
                                format!("Can't use {chord}: {why}. Try a ctrl, alt or F-key combination."),
                            );
                            Some(Modal::CaptureKey { action })
                        }
                        None => {
                            let taken = self.keymap.bind(action, chord);
                            self.keymap_changed();
                            let mut msg = format!("{}: {chord}", action.describe());
                            if let Some(prev) = taken {
                                msg.push_str(&format!(" (taken from {})", prev.describe().to_lowercase()));
                            }
                            self.toast(Level::Success, msg);
                            None
                        }
                    }
                }
            },
            Modal::Prompt(mut prompt) => match key.code {
                KeyCode::Enter => {
                    // Most prompts want trimmed text; the paragraph break keeps its spaces.
                    let text = match prompt.action {
                        PromptAction::ParagraphBreak => prompt.editor.text().to_owned(),
                        _ => prompt.editor.text().trim().to_owned(),
                    };
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
            Modal::Snippets { mut filter, mut selected } => {
                let visible = self.drawer.filtered_snippets(&filter);
                match key.code {
                    KeyCode::Esc => None,
                    KeyCode::Enter => {
                        if let Some(&i) = visible.get(selected) {
                            self.insert_snippet(i);
                        }
                        None
                    }
                    KeyCode::Up => {
                        step(&mut selected, visible.len(), -1);
                        Some(Modal::Snippets { filter, selected })
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        step(&mut selected, visible.len(), 1);
                        Some(Modal::Snippets { filter, selected })
                    }
                    KeyCode::Backspace => {
                        filter.pop();
                        Some(Modal::Snippets { filter, selected: 0 })
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        filter.push(c);
                        Some(Modal::Snippets { filter, selected: 0 })
                    }
                    _ => Some(Modal::Snippets { filter, selected }),
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
                let (label, tags) = crate::drawer::split_label_tags(&text);
                if let Some(item) = self.drawer.items.get_mut(i) {
                    if !label.is_empty() {
                        item.label = label;
                    }
                    for tag in tags {
                        if !item.tags.contains(&tag) {
                            item.tags.push(tag);
                        }
                    }
                    self.drawer_changed();
                }
            }
            PromptAction::DrawerEditNote(i) => {
                if let Some(item) = self.drawer.items.get_mut(i) {
                    item.note = text;
                    self.drawer_changed();
                }
            }
            PromptAction::DrawerEditTags(i) => {
                if let Some(item) = self.drawer.items.get_mut(i) {
                    item.tags = crate::drawer::parse_tags(&text);
                    self.drawer_changed();
                }
            }
            PromptAction::ExportLog(i) => self.export_log(i, &text),
            PromptAction::SnippetName => match crate::drawer::normalize_snippet_name(&text) {
                Some(name) if self.drawer.snippet(&name).is_some() => {
                    self.toast(Level::Error, format!("There's already a snippet called `{name}`."));
                }
                Some(name) => {
                    let title = format!("Text for `{name}` (empty opens your editor)");
                    self.open_prompt(&title, "", PromptAction::SnippetText { name });
                }
                None => self.toast(Level::Error, format!("`{text}` can't be a snippet name.")),
            },
            PromptAction::SnippetText { name } => {
                if let Some(i) = self.add_snippet(&name, &text) {
                    self.drawer_ui.shelf = Shelf::Snippets;
                    self.drawer_ui.snippets.selected = i;
                    if text.is_empty() {
                        self.edit_snippet_in_editor(i);
                    }
                }
            }
            PromptAction::SnippetRename(i) => match self.drawer.rename_snippet(i, &text) {
                Ok(()) => self.drawer_changed(),
                Err(e) => self.toast(Level::Error, e.to_string()),
            },
            PromptAction::DrawerExport => self.export_drawer(&text),
            PromptAction::DrawerImport => self.import_drawer(&text),
            PromptAction::LogRename(i) => self.logs.rename(i, &text),
            PromptAction::EditorCommand => {
                self.config.settings.editor = text;
                self.config_changed();
            }
            PromptAction::ParagraphBreak => {
                self.config.settings.paragraph_break = text;
                self.config_changed();
            }
            PromptAction::RequeueDelay => match text.parse::<u64>() {
                Ok(n @ 0..=120) => {
                    self.config.settings.requeue_delay_secs = n;
                    self.config_changed();
                }
                _ => self.toast(Level::Error, "Use a number of seconds from 0 to 120."),
            },
            PromptAction::MinSharedKinks => match text.parse::<u8>() {
                Ok(n @ 0..=20) => {
                    self.config.settings.skip.min_shared_kinks = n;
                    self.config_changed();
                }
                _ => self.toast(Level::Error, "Use a number from 0 to 20."),
            },
            PromptAction::Keywords => {
                self.config.settings.notify.keywords =
                    text.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned).collect();
                self.config_changed();
            }
            PromptAction::SoundCommand => {
                self.config.settings.notify.sound_command = text;
                self.config_changed();
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
