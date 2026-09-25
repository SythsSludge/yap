//! The buddy's feelings: what it's reacting to, and what it says.

use super::*;
use crate::buddy::{Buddy, Event, Mood};

/// How long a speech bubble stays up.
const SPEECH: Duration = Duration::from_secs(5);
/// A partner this quiet makes the buddy drowsy.
const DROWSY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub struct BuddyState {
    /// A reaction and when it wears off.
    pub reaction: Option<(Mood, Instant)>,
    pub speech: Option<(String, Instant)>,
    /// When the current look started, for animating from its first frame.
    pub since: Instant,
    rng: u64,
}

impl BuddyState {
    pub fn new(now: Instant) -> Self {
        // Tests get the same lines every run.
        let seed = if cfg!(test) {
            1
        } else {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
        };
        BuddyState { reaction: None, speech: None, since: now, rng: seed | 1 }
    }

    fn pick(&mut self, len: usize) -> usize {
        self.rng = self.rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.rng >> 33) as usize % len.max(1)
    }
}

impl App {
    /// The buddy in use, if one is switched on.
    pub fn buddy(&self) -> Option<&Buddy> {
        let name = self.config.settings.buddy.trim();
        self.buddies.iter().find(|b| b.name == name)
    }

    pub fn buddy_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.buddies.iter().map(|b| b.name.clone()).collect();
        names.push("off".into());
        names
    }

    /// `/buddy fox`, `/buddy off`, or `/buddy` to list them.
    pub fn set_buddy(&mut self, name: Option<&str>) {
        match name.map(str::trim) {
            None | Some("") => {
                let names = self.buddy_names().join(", ");
                let current = self.buddy().map_or("off".to_owned(), |b| b.name.clone());
                self.toast(Level::Info, format!("Buddy: {current}. Choose from {names}."));
            }
            Some("off" | "none") => {
                self.config.settings.buddy.clear();
                self.config_changed();
            }
            Some(name) if self.buddies.iter().any(|b| b.name == name) => {
                self.config.settings.buddy = name.to_owned();
                self.config_changed();
                self.buddy_event(Event::Petted);
            }
            Some(name) => {
                self.toast(Level::Error, format!("No buddy called `{name}`. Try: {}", self.buddy_names().join(", ")))
            }
        }
    }

    /// React to something that happened in the chat on screen.
    pub fn buddy_event(&mut self, event: Event) {
        if !self.chat_visible() || self.buddy().is_none() {
            return;
        }
        let (mood, secs) = event.mood();
        let now = self.now;
        if self.buddy_state.reaction.map(|(m, _)| m) != Some(mood) {
            self.buddy_state.since = now;
        }
        self.buddy_state.reaction = Some((mood, now + Duration::from_secs(secs)));
        if event.speaks() {
            let lines = self.buddy().map(|b| b.lines_for(event)).unwrap_or_default();
            if !lines.is_empty() {
                let line = lines[self.buddy_state.pick(lines.len())].clone();
                let line = crate::drawer::expand_placeholders(&line, &self.snippet_vars());
                self.buddy_state.speech = Some((line, now + SPEECH));
            }
        }
    }

    /// How the buddy looks right now: a fresh reaction, or else how things are going.
    pub fn buddy_mood(&self) -> Mood {
        if let Some((mood, until)) = self.buddy_state.reaction
            && self.now < until
        {
            return mood;
        }
        let quiet = self.clock.last_heard.or(self.clock.partner_since).map(|t| self.now.saturating_duration_since(t));
        match (&self.status, &self.partner) {
            (ConnStatus::Offline { .. }, _) => Mood::Dizzy,
            (_, PartnerState::Searching) => Mood::Searching,
            (_, PartnerState::Connected(_)) if self.chat.partner_typing => Mood::Curious,
            (_, PartnerState::Connected(_)) if quiet.is_some_and(|q| q >= DROWSY) => Mood::Sleepy,
            _ => Mood::Idle,
        }
    }

    /// The lines to draw this frame.
    pub fn buddy_frame(&self) -> Vec<String> {
        let Some(buddy) = self.buddy() else { return Vec::new() };
        let elapsed = self.now.saturating_duration_since(self.buddy_state.since).as_millis();
        buddy.frame(self.buddy_mood(), elapsed).to_vec()
    }

    pub fn buddy_speech(&self) -> Option<&str> {
        self.buddy_state.speech.as_ref().filter(|(_, until)| self.now < *until).map(|(s, _)| s.as_str())
    }

    /// Reload `~/.config/yap/buddies`.
    pub fn load_buddies(&mut self) -> Vec<String> {
        let dir = self.paths.config_file.parent().map(|d| d.join("buddies")).unwrap_or_default();
        let (buddies, errors) = crate::buddy::load_all(&dir);
        self.buddies = buddies;
        errors
    }
}
