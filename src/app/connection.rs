//! The socket lifecycle and everything the server tells us.

use super::*;

impl App {
    pub fn connect(&mut self) {
        self.closing = false;
        self.status = ConnStatus::Connecting;
        self.reset_partner();
        self.can_block_previous = false;
        self.token = None;
        let url = self.server_override.clone().unwrap_or_else(|| self.config.settings.server_url.clone());
        self.effect(Effect::Connect(url));
    }

    /// The runtime couldn't even start connecting (e.g. a malformed URL). Retrying
    /// wouldn't help, so wait for the user.
    pub fn connection_error(&mut self, reason: String) {
        self.toast(Level::Error, reason.clone());
        self.status = ConnStatus::Offline { reason, retry_at: None };
    }

    pub(super) fn reset_partner(&mut self) {
        self.partner = PartnerState::None;
        self.chat.partner_typing = false;
        self.typing_sent = false;
        self.clock = Clock::default();
    }

    fn backoff(&self) -> Duration {
        let secs = 1u64 << self.reconnect_attempt.min(5);
        Duration::from_secs(secs).min(MAX_BACKOFF)
    }

    pub fn on_net(&mut self, event: NetEvent) {
        match event {
            NetEvent::Traffic(t) => {
                self.traffic.push(t.at, t.dir, t.kind, t.size, t.body);
            }
            NetEvent::Open => {
                self.status = ConnStatus::Online;
            }
            NetEvent::Message(msg) => self.on_server(msg),
            NetEvent::Unparsed { raw, error } => {
                let preview = crate::text::truncate(&crate::text::sanitize(&raw), 80);
                self.push(EntryKind::Warning(format!("Couldn't read a server frame ({error}): {preview}")));
            }
            NetEvent::Closed { reason } => self.on_closed(reason),
        }
    }

    fn on_closed(&mut self, reason: String) {
        let had_partner = self.has_partner();
        let was_partnered = had_partner || self.partner == PartnerState::Searching;
        self.reset_partner();
        self.can_block_previous = false;
        // The count is server-wide; only forget it when no socket is left to update it.
        if self.others.iter().all(|s| s.status != ConnStatus::Online) {
            self.users_online = None;
        }
        if std::mem::take(&mut self.closing) {
            if had_partner {
                self.end_conversation();
            }
            self.status = ConnStatus::Offline { reason, retry_at: None };
            return;
        }
        let retry_at = self.config.settings.auto_reconnect.then(|| self.now + self.backoff());
        if was_partnered || !matches!(self.status, ConnStatus::Offline { .. }) {
            let hint = match (retry_at, self.keymap.hint(crate::keymap::Action::Reconnect)) {
                (Some(_), _) => "Reconnecting…".to_owned(),
                (None, Some(key)) => format!("Press {key} to reconnect."),
                (None, None) => "Type /reconnect to reconnect.".to_owned(),
            };
            self.push(EntryKind::Warning(format!("Disconnected from the server: {reason}. {hint}")));
        }
        if had_partner {
            self.flag_undelivered("the connection dropped");
            self.end_conversation();
        }
        self.status = ConnStatus::Offline { reason, retry_at };
    }

    // ----- server messages ------------------------------------------------------

    fn on_server(&mut self, msg: ServerMessage) {
        match msg {
            ServerMessage::ConnectionSuccess { token } => {
                self.token = Some(token);
                self.status = ConnStatus::Online;
                self.reconnect_attempt = 0;
            }
            ServerMessage::ConnectionExists => {
                self.toast(Level::Warning, "You already have an active session.");
            }
            ServerMessage::UserCount(n) => self.users_online = Some(n),
            ServerMessage::ReceiveMessage(text) => {
                let text = crate::text::sanitize(&text);
                self.chat.partner_typing = false;
                self.clock.typing_since = None;
                self.clock.last_heard = Some(self.now);
                self.request_images(&text);
                self.count(|s| s.received += 1);
                let mentioned = !crate::text::find_keywords(&text, &self.config.settings.notify.keywords).is_empty();
                self.push(EntryKind::Partner(text));
                if mentioned {
                    self.alert("Mentioned you");
                } else if self.config.settings.notify.on_message {
                    self.alert("New Message");
                }
            }
            ServerMessage::PartnerTyping(on) => {
                self.chat.partner_typing = on && self.has_partner();
                self.clock.typing_since =
                    if self.chat.partner_typing { self.clock.typing_since.or(Some(self.now)) } else { None };
            }
            ServerMessage::PartnerConnected(info) => self.on_partner_connected(info),
            ServerMessage::PartnerPending => {
                self.partner = PartnerState::Searching;
                self.clock.searching_since.get_or_insert(self.now);
                self.system(
                    "We are looking for a partner to match you with. \
                     Please either continue to wait, or modify your yiffing preferences.",
                );
            }
            ServerMessage::PartnerLeft => {
                self.reset_partner();
                self.can_block_previous = true;
                self.system("Your yiffing partner has left.");
                self.flag_undelivered("they left");
                self.end_conversation();
                self.alert("Partner Left");
                self.schedule_requeue();
            }
            ServerMessage::PartnerDisconnected => {
                self.reset_partner();
                self.can_block_previous = false;
                self.system("Your yiffing partner has disconnected unexpectedly.");
                self.flag_undelivered("they disconnected");
                self.end_conversation();
                self.schedule_requeue();
                self.alert("Partner Disconnected");
            }
            ServerMessage::PartnerBlocked => {
                if self.has_partner() {
                    self.system("Your partner has been blocked and disconnected from you.");
                    self.end_conversation();
                    self.schedule_requeue();
                } else {
                    let kind = EntryKind::System("Your previous partner has been blocked.".into());
                    let at = Local::now();
                    self.chat.push(at, kind.clone());
                    self.logs.record_after(chat::Entry { at, kind });
                }
                self.reset_partner();
                self.can_block_previous = false;
            }
            ServerMessage::ClientDisconnect => {
                self.reset_partner();
                self.can_block_previous = true;
                self.system("You have disconnected from your partner.");
                self.end_conversation();
            }
            ServerMessage::InvalidPreferences => {
                if self.partner == PartnerState::Searching {
                    self.partner = PartnerState::None;
                }
                self.toast(
                    Level::Error,
                    "You have attempted to submit invalid preferences. Please check your preferences again.",
                );
            }
            ServerMessage::Unknown { kind, .. } => {
                self.push(EntryKind::Warning(format!(
                    "The server sent an unrecognised `{kind}` message (see the Traffic tab)."
                )));
            }
        }
    }

    /// The server silently drops messages for a partner who's already gone, so flag
    /// ours from the moments before we heard about it (`what` happened).
    fn flag_undelivered(&mut self, what: &str) {
        let now = Local::now();
        let recent: Vec<usize> = (0..self.chat.entries.len())
            .rev()
            .take_while(|&i| now - self.chat.entries[i].at <= UNDELIVERED_WINDOW)
            .filter(|&i| matches!(self.chat.entries[i].kind, EntryKind::You(_)))
            .collect();
        if recent.is_empty() {
            return;
        }
        self.chat.unsure.extend(&recent);
        let mut text = match recent.len() {
            1 => format!("Your last message may not have reached them: it was sent just as {what}."),
            n => format!("Your last {n} messages may not have reached them: they were sent just as {what}."),
        };
        if let Some(key) = self.keymap.hint(crate::keymap::Action::SelectMessage) {
            text.push_str(&format!(" {key} then y copies one."));
        }
        self.push(EntryKind::Warning(text));
    }

    fn on_partner_connected(&mut self, info: PartnerInfo) {
        let info = PartnerInfo {
            gender: crate::text::sanitize(&info.gender),
            species: crate::text::sanitize(&info.species),
            kinks: crate::text::sanitize(&info.kinks),
            role: crate::text::sanitize(&info.role),
            language: info.language.as_deref().map(crate::text::sanitize),
        };
        if self.auto_skip(&info) {
            return;
        }
        let mine = &self.config.active().preferences.kinks;
        let common = info
            .kink_list()
            .into_iter()
            .filter(|k| *k != ANY && mine.iter().any(|m| m == k))
            .map(str::to_owned)
            .collect();
        self.reset_partner();
        if self.config.settings.split_chats {
            self.chat.clear();
        }
        self.partner_nick = None;
        self.count(|s| s.partners += 1);
        let me = Some(self.config.active().character.clone()).filter(|c| !c.is_empty());
        self.logs.start(self.session_id, Local::now(), info.clone(), me);
        self.system("You have been connected with a yiffing partner.");
        if let Some(lang) = &info.language {
            self.system(format!("Your partner's language is {lang}"));
        }
        self.push(EntryKind::PartnerInfo { info: info.clone(), common });
        self.partner = PartnerState::Connected(info);
        self.clock.partner_since = Some(self.now);
        self.can_block_previous = false;
        if !self.background {
            self.tab = Tab::Chat;
        }
        self.alert("Partner Connected");
    }

    /// Attention-grabbing. While the terminal is in the background: title flash, bell,
    /// desktop notification, sound (like the website flashing its tab when hidden).
    /// While you're in the app but looking at another tab or chat: a toast.
    /// The configured sound command, or a desktop default if one is installed.
    pub fn sound_command(&self) -> Option<String> {
        let configured = self.config.settings.notify.sound_command.trim();
        if !configured.is_empty() {
            return Some(configured.to_owned());
        }
        let on_path = |bin: &str| {
            std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|dir| dir.join(bin).is_file()))
        };
        let player = ["pw-play", "paplay"].into_iter().find(|p| on_path(p))?;
        let sound = [
            "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
            "/usr/share/sounds/freedesktop/stereo/message.oga",
            "/usr/share/sounds/freedesktop/stereo/bell.oga",
        ]
        .into_iter()
        .find(|f| std::path::Path::new(f).is_file())?;
        Some(format!("{player} {sound}"))
    }

    pub(super) fn alert(&mut self, what: &str) {
        let what =
            if self.session_count() > 1 { format!("Chat {}: {what}", self.session_number()) } else { what.to_owned() };
        if self.focused {
            if !self.chat_visible() {
                self.toast(Level::Info, what);
            }
            return;
        }
        let what = what.as_str();
        let notify = self.config.settings.notify.clone();
        if notify.sound {
            match self.sound_command() {
                Some(cmd) => self.effect(Effect::PlaySound(cmd)),
                None if !notify.bell => self.effect(Effect::Bell),
                None => {}
            }
        }
        if notify.title {
            self.alert = Some((what.to_owned(), self.now));
        }
        if notify.bell {
            self.effect(Effect::Bell);
        }
        if notify.desktop {
            self.effect(Effect::Notify { title: "yap".into(), body: what.to_owned() });
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if focused {
            self.alert = None;
        }
    }

    /// The terminal title: flashes the pending alert while unfocused.
    pub fn window_title(&self) -> String {
        match &self.alert {
            Some((msg, since)) if (self.now.duration_since(*since).as_millis() / 700).is_multiple_of(2) => {
                format!("{msg} · yap")
            }
            _ => "yap".into(),
        }
    }
}
