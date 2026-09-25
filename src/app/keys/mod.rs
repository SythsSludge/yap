//! Keyboard, mouse and paste handling.

mod modal;
mod prefs;
mod tabs;

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

fn sanitize_file_name(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
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
            Action::Palette => self.open_palette(),
            Action::Find => self.request_find(),
            Action::Next => self.next_partner(),
            Action::Snippets => self.open_snippet_picker(),
            Action::Editor => self.compose_in_editor(),
            Action::SelectMessage => self.select_message(None),
            Action::SearchChat => self.start_search(None),
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
        if self.transcript_key(key) {
            return;
        }
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
}
