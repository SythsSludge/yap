//! Keys for the Drawer, Logs, Traffic and Settings tabs.

use super::*;

impl App {
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

    pub(super) fn drawer_key(&mut self, key: KeyEvent) {
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

    pub(super) fn logs_key(&mut self, key: KeyEvent) {
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
                self.open_prompt("Save transcript to (.txt, .md or .html)", &path, PromptAction::ExportLog(index));
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

    pub(super) fn traffic_key(&mut self, key: KeyEvent) {
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

    pub(super) fn settings_key(&mut self, key: KeyEvent) {
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
}
