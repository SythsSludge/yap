//! Keys for popups: the image viewer, confirmations, prompts and pickers.

use super::*;
use crate::images::{Viewer, ViewerNote};

impl App {
    pub(super) fn viewer_key(&mut self, key: KeyEvent) {
        let url = self.viewer.as_ref().map(|v| v.url.clone()).unwrap_or_default();
        let untrusted = matches!(self.viewer, Some(Viewer { note: Some(ViewerNote::Untrusted { .. }), .. }));
        match key.code {
            KeyCode::Left | KeyCode::Up | KeyCode::Char('h' | 'k') | KeyCode::PageUp | KeyCode::BackTab => {
                self.viewer_step(-1)
            }
            KeyCode::Right | KeyCode::Down | KeyCode::Char('l' | 'j' | ' ') | KeyCode::PageDown | KeyCode::Tab => {
                self.viewer_step(1)
            }
            KeyCode::Enter if untrusted => self.viewer_load_once(),
            KeyCode::Char('t') if untrusted => self.viewer_trust(),
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
    pub(super) fn modal_key(&mut self, modal: Modal, key: KeyEvent) {
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
            Modal::Spelling { word, start, end, suggestions, mut selected } => {
                // The suggestions, then "add to dictionary".
                let rows = suggestions.len() + 1;
                if navigate(&mut selected, rows, &key, 5) {
                    Some(Modal::Spelling { word, start, end, suggestions, selected })
                } else {
                    if key.code == KeyCode::Enter {
                        self.apply_spelling(&word, start, end, suggestions.get(selected).map(String::as_str));
                    }
                    None
                }
            }
            Modal::Stats => match key.code {
                KeyCode::Char('h') => Some(Modal::History { scroll: 0 }),
                _ => None,
            },
            Modal::History { scroll } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => Some(Modal::History { scroll: scroll.saturating_sub(1) }),
                KeyCode::Down | KeyCode::Char('j') => Some(Modal::History { scroll: scroll.saturating_add(1) }),
                KeyCode::PageUp => Some(Modal::History { scroll: scroll.saturating_sub(10) }),
                KeyCode::PageDown | KeyCode::Char(' ') => Some(Modal::History { scroll: scroll.saturating_add(10) }),
                KeyCode::Home | KeyCode::Char('g') => Some(Modal::History { scroll: 0 }),
                KeyCode::Char('x') => {
                    self.modal = Some(Modal::Confirm {
                        text: "Forget your whole partner history? Stats and chat logs aren't affected.".into(),
                        action: Confirm::ClearHistory,
                    });
                    return;
                }
                _ => None,
            },
            Modal::Kinks { scroll } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => Some(Modal::Kinks { scroll: scroll.saturating_sub(1) }),
                KeyCode::Down | KeyCode::Char('j') => Some(Modal::Kinks { scroll: scroll.saturating_add(1) }),
                KeyCode::PageUp => Some(Modal::Kinks { scroll: scroll.saturating_sub(10) }),
                KeyCode::PageDown | KeyCode::Char(' ') => Some(Modal::Kinks { scroll: scroll.saturating_add(10) }),
                KeyCode::Home | KeyCode::Char('g') => Some(Modal::Kinks { scroll: 0 }),
                _ => None,
            },
            Modal::Palette { mut query, mut selected } => {
                let len = self.palette_matches(&query).len();
                match key.code {
                    KeyCode::Esc => None,
                    KeyCode::Enter => {
                        if let Some(entry) = self.palette_matches(&query).into_iter().nth(selected) {
                            self.run_palette_item(entry.item);
                        }
                        None
                    }
                    KeyCode::Up => {
                        step(&mut selected, len, -1);
                        Some(Modal::Palette { query, selected })
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        step(&mut selected, len, 1);
                        Some(Modal::Palette { query, selected })
                    }
                    KeyCode::PageUp => {
                        step(&mut selected, len, -8);
                        Some(Modal::Palette { query, selected })
                    }
                    KeyCode::PageDown => {
                        step(&mut selected, len, 8);
                        Some(Modal::Palette { query, selected })
                    }
                    KeyCode::Backspace => {
                        query.pop();
                        Some(Modal::Palette { query, selected: 0 })
                    }
                    KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                        query.push(c);
                        Some(Modal::Palette { query, selected: 0 })
                    }
                    _ => Some(Modal::Palette { query, selected }),
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
                Ok(()) => {
                    self.sync_session_profiles(Some((&from, text.trim())));
                    self.config_changed();
                }
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
            PromptAction::SnippetFromMessage { text: body } => {
                if let Some(i) = self.add_snippet(&text, &body) {
                    self.drawer_ui.snippets.selected = i;
                }
            }
            PromptAction::SnippetRename(i) => match self.drawer.rename_snippet(i, &text) {
                Ok(()) => self.drawer_changed(),
                Err(e) => self.toast(Level::Error, e.to_string()),
            },
            PromptAction::DrawerExport => self.export_drawer(&text),
            PromptAction::DrawerImport => self.import_drawer(&text),
            PromptAction::LogRename(i) => self.logs.rename(i, &text),
            PromptAction::CharacterName(profile) => {
                if let Some(p) = self.config.profiles.iter_mut().find(|p| p.name == profile) {
                    p.character = crate::text::sanitize(&text).replace('\n', " ");
                    self.config_changed();
                }
            }
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
