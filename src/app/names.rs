//! Display names: your character, and nicknames for partners. Purely cosmetic; the
//! server never sees them.

use super::*;

impl App {
    /// How you appear in the chat: your character name, else "you".
    pub fn my_label(&self) -> String {
        match self.config.active().character.trim() {
            "" => "you".into(),
            name => name.to_owned(),
        }
    }

    /// How the partner appears: their nickname, else "partner".
    pub fn partner_label(&self) -> String {
        self.partner_nick.clone().unwrap_or_else(|| "partner".into())
    }

    pub fn set_character(&mut self, name: &str) {
        let name = crate::text::sanitize(name.trim()).replace('\n', " ");
        let profile = self.config.active_profile.clone();
        self.config.active_mut().character = name.clone();
        if let Some(s) = &mut self.speller {
            s.learn(&name);
        }
        self.config_changed();
        if name.is_empty() {
            self.toast(Level::Info, format!("Profile `{profile}` no longer has a character name."));
        } else {
            self.toast(Level::Success, format!("You're {name} in profile `{profile}`."));
        }
    }

    pub fn set_nick(&mut self, nick: &str) {
        if !self.has_partner() {
            return self.toast(Level::Info, "Nicknames are for the partner you're chatting with.");
        }
        let nick = crate::text::sanitize(nick.trim()).replace('\n', " ");
        self.partner_nick = (!nick.is_empty()).then_some(nick.clone());
        if let Some(s) = &mut self.speller {
            s.learn(&nick);
        }
        self.logs.set_nick(self.session_id, self.partner_nick.clone());
        if nick.is_empty() {
            self.toast(Level::Info, "Nickname cleared.");
        }
    }
}
