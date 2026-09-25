//! Things the user does: finding, leaving and blocking partners, sending messages,
//! slash commands, and the periodic tick.

use super::*;

impl App {
    pub(super) fn confirm_or(&mut self, text: &str, action: Confirm) {
        if self.config.settings.confirm_actions {
            self.modal = Some(Modal::Confirm { text: text.into(), action });
        } else {
            self.run_confirmed(action);
        }
    }

    pub fn run_confirmed(&mut self, action: Confirm) {
        match action {
            Confirm::FindNew => self.find_partner(),
            Confirm::Leave => {
                self.send(ClientMessage::Disconnect);
            }
            Confirm::Block => {
                self.send(ClientMessage::BlockPartner);
            }
            Confirm::Quit => self.quit = true,
            Confirm::CloseSession => self.close_session(),
            Confirm::DeleteSnippet(index) => {
                if index < self.drawer.snippets.len() {
                    let s = self.drawer.snippets.remove(index);
                    self.drawer_changed();
                    self.toast(Level::Info, format!("Deleted snippet `{}`.", s.name));
                }
            }
            Confirm::DeleteProfile(name) => match self.config.delete_profile(&name) {
                Ok(()) => {
                    self.prefs_ui.profile = self.prefs_ui.profile.min(self.config.profiles.len() - 1);
                    self.config_changed();
                    self.toast(Level::Info, format!("Deleted profile `{name}`."));
                }
                Err(e) => self.toast(Level::Error, e.to_string()),
            },
            Confirm::DeleteDrawerItem(index) => {
                if let Some(item) = self.drawer.remove(index) {
                    self.drawer_changed();
                    self.toast(Level::Info, format!("Removed `{}` from the drawer.", item.label));
                }
            }
            Confirm::DeleteLog(index) => match self.logs.delete(index) {
                Ok(conv) => {
                    self.logs_ui.reading = false;
                    self.logs_ui.list.selected = self.logs_ui.list.selected.saturating_sub(1);
                    self.toast(Level::Info, format!("Deleted chat with {}.", conv.title()));
                }
                Err(e) => self.toast(Level::Error, format!("{e:#}")),
            },
            Confirm::LoadUntrusted(url) => {
                let host = url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_owned));
                self.open_viewer(&url, host);
            }
        }
    }

    /// "Find Partner": validate, confirm replacing a current partner, then search.
    pub fn request_find(&mut self) {
        if let Err(invalid) = self.config.active().preferences.validate() {
            self.show_invalid(invalid);
            return;
        }
        if !self.is_online() {
            self.toast(Level::Error, "You're not connected to the server.");
            return;
        }
        if self.has_partner() {
            self.confirm_or("Are you sure you want to find a new partner?", Confirm::FindNew);
        } else {
            self.find_partner();
        }
    }

    pub(super) fn show_invalid(&mut self, invalid: Invalid) {
        self.toast(Level::Error, invalid.to_string());
        self.tab = Tab::Preferences;
        self.prefs_ui.pane = PrefsPane::Fields;
        self.prefs_ui.field = Field::ALL.iter().position(|f| *f == invalid.field()).unwrap_or(0);
    }

    pub(super) fn find_partner(&mut self) {
        let wire = match self.config.active().preferences.to_wire(self.config.settings.send_language) {
            Ok(w) => w,
            Err(invalid) => return self.show_invalid(invalid),
        };
        let had_partner = self.has_partner();
        // Auto-skips search again mid-search; keep counting from the first search.
        let searching_since = if self.partner == PartnerState::Searching { self.clock.searching_since } else { None };
        if !self.send(ClientMessage::FindPartner(wire)) {
            return;
        }
        if had_partner {
            self.system("You have disconnected from your previous partner.");
            self.end_conversation();
        }
        self.reset_partner();
        self.can_block_previous = had_partner;
        self.partner = PartnerState::Searching;
        self.clock.searching_since = searching_since.or(Some(self.now));
        self.requeue_at = None;
        if !self.background {
            self.tab = Tab::Chat;
        }
        self.chat.follow();
    }

    pub fn request_leave(&mut self) {
        match self.partner {
            PartnerState::Connected(_) => self.confirm_or("Disconnect from your partner?", Confirm::Leave),
            // The protocol has no "cancel search"; the server keeps us queued until the
            // socket closes, so reconnecting is the only way out.
            PartnerState::Searching => {
                self.system("Stopped searching.");
                self.connect();
            }
            PartnerState::None => self.toast(Level::Info, "You're not connected to a partner."),
        }
    }

    pub fn request_block(&mut self) {
        if self.has_partner() || self.can_block_previous {
            self.confirm_or("Are you sure you want to block this partner?", Confirm::Block);
        } else {
            self.toast(Level::Info, "There's no partner to block.");
        }
    }

    pub fn request_quit(&mut self) {
        if self.any_partner() {
            self.confirm_or("You're still chatting. Quit anyway?", Confirm::Quit);
        } else {
            self.quit = true;
        }
    }

    /// Enter in the chat box: a slash command or a message.
    pub fn submit_input(&mut self) {
        match commands::parse(self.input.text()) {
            Parsed::Message(text) => self.send_chat(text),
            Parsed::Command(cmd) => {
                self.input.submit();
                self.input_changed();
                self.run_command(cmd);
            }
            Parsed::Error(e) => self.toast(Level::Error, e),
        }
    }

    fn send_chat(&mut self, text: String) {
        let text = if self.config.settings.emoji_shortcodes { crate::emoji::expand(&text) } else { text };
        // Same checks, order and messages as the web client. JS counts UTF-16 units.
        if text.is_empty() {
            return self.toast(Level::Error, "Please enter a message.");
        }
        if text.encode_utf16().count() >= MAX_MESSAGE_LEN {
            return self.toast(Level::Error, "Please shorten the length of your message.");
        }
        if !self.has_partner() {
            return self.toast(Level::Error, "You are not connected to a partner yet.");
        }
        if !self.is_online() {
            return self.toast(Level::Error, "You're not connected to the server.");
        }
        self.effect(Effect::Send(ClientMessage::Typing(false)));
        self.typing_sent = false;
        self.effect(Effect::Send(ClientMessage::SendMessage(text.clone())));
        self.count(|s| s.sent += 1);
        self.input.submit();
        self.request_images(&text);
        self.push(EntryKind::You(text));
        self.chat.follow();
    }

    /// Keep the partner's "typing…" indicator honest: on when we start, off when we
    /// clear the box or go idle. (The web client sends `true` on every keystroke and
    /// never turns it off unless a message is sent.)
    pub fn input_changed(&mut self) {
        if !self.has_partner() || !self.is_online() {
            return;
        }
        // A slash command isn't a message, so it shouldn't show as typing.
        let text = self.input.text();
        let command = text.starts_with('/') && !text.starts_with("//");
        if self.input.is_empty() || command {
            if self.typing_sent {
                self.typing_sent = false;
                self.effect(Effect::Send(ClientMessage::Typing(false)));
            }
        } else {
            self.last_edit = Some(self.now);
            if !self.typing_sent {
                self.typing_sent = true;
                self.effect(Effect::Send(ClientMessage::Typing(true)));
            }
        }
    }

    pub fn on_tick(&mut self) {
        let now = self.now;
        self.toasts.retain(|t| t.expires > now);
        self.tick_sessions();
        self.tick_reload();
    }

    /// Timers for the current session: typing idle and reconnect backoff.
    pub(super) fn tick_session(&mut self) {
        self.tick_requeue();
        let now = self.now;
        if self.typing_sent && self.last_edit.is_some_and(|t| now.duration_since(t) >= TYPING_IDLE) {
            self.typing_sent = false;
            if self.has_partner() && self.is_online() {
                self.effect(Effect::Send(ClientMessage::Typing(false)));
            }
        }
        if let ConnStatus::Offline { retry_at: Some(at), .. } = self.status
            && now >= at
        {
            self.reconnect_attempt += 1;
            self.connect();
        }
    }

    pub fn run_command(&mut self, cmd: Command) {
        match cmd {
            Command::Find => self.request_find(),
            Command::Leave => self.request_leave(),
            Command::Block => self.request_block(),
            Command::Reconnect => {
                self.reconnect_attempt = 0;
                self.connect();
            }
            Command::Clear => self.chat.clear(),
            Command::Logs => self.tab = Tab::Logs,
            Command::Help => self.modal = Some(Modal::Help { scroll: 0 }),
            Command::Quit => self.request_quit(),
            Command::Links => self.open_links(),
            Command::Drawer => self.toggle_drawer_panel(),
            Command::Profile(None) => self.open_profile_picker(),
            Command::Profile(Some(name)) => self.switch_profile(&name),
            Command::Theme(None) => self.open_theme_picker(),
            Command::Theme(Some(name)) => self.set_theme(&name, true),
            Command::Save { url, label } => self.add_to_drawer(&url, &label),
            Command::ExportAll(path) => self.export(None, &path),
            Command::ExportProfile(path) => {
                let name = self.config.active_profile.clone();
                self.export(Some(&name), &path);
            }
            Command::Import { path, with_settings } => self.import(&path, with_settings),
            Command::Trust(domain) => self.trust_domain(&domain),
            Command::Untrust(domain) => self.untrust_domain(&domain),
            Command::Raw(frame) => self.send_raw(frame),
            Command::SaveLog(path) => self.save_log(&path),
            Command::Next => self.next_partner(),
            Command::Snip(None) => self.open_snippet_picker(),
            Command::Snip(Some(name)) => self.insert_snippet_named(&name),
            Command::SnipAdd { name, text } => {
                self.add_snippet(&name, &text);
            }
            Command::Edit => self.compose_in_editor(),
            Command::Name(name) => self.set_character(name.as_deref().unwrap_or("")),
            Command::Nick(nick) => self.set_nick(nick.as_deref().unwrap_or("")),
            Command::Search(query) => self.start_search(query.as_deref()),
            Command::Select => self.select_message(None),
            Command::Stats => self.modal = Some(Modal::Stats),
            Command::Kinks => self.open_kinks(),
            Command::DrawerExport(path) => self.export_drawer(&path),
            Command::DrawerImport(path) => self.import_drawer(&path),
            Command::NewChat => self.new_session(),
            Command::CloseChat => self.request_close_session(),
            Command::SwitchChat(n) => match self.sessions().get(n - 1) {
                Some(s) => {
                    let id = s.id;
                    self.switch_session(id);
                }
                None => self.toast(Level::Error, format!("There's no chat {n}.")),
            },
        }
    }

    pub fn send_raw(&mut self, frame: String) {
        if self.is_online() {
            self.effect(Effect::SendRaw(frame));
        } else {
            self.toast(Level::Error, "You're not connected to the server.");
        }
    }
}
