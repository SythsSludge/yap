//! Moving between partners quickly: one-key next, automatic re-queueing, and
//! client-side skip rules the server doesn't know about.

use super::*;
use crate::history::SkipRule;

impl App {
    /// Skip straight to someone new. Unlike Find, this never asks: that's the point.
    pub fn next_partner(&mut self) {
        self.requeue_at = None;
        if let Err(invalid) = self.config.active().preferences.validate() {
            return self.show_invalid(invalid);
        }
        if !self.is_online() {
            return self.toast(Level::Error, "You're not connected to the server.");
        }
        self.find_partner();
    }

    /// After a partner leaves, search again after a short delay if enabled.
    pub(super) fn schedule_requeue(&mut self) {
        if !self.config.settings.auto_requeue {
            return;
        }
        let delay = self.config.settings.requeue_delay_secs;
        self.requeue_at = Some(self.now + Duration::from_secs(delay));
        if delay > 0 {
            self.system(format!("Searching again in {delay}s. Esc cancels."));
        }
    }

    /// Returns true if a pending re-queue was cancelled.
    pub fn cancel_requeue(&mut self) -> bool {
        if self.requeue_at.take().is_some() {
            self.system("Stayed put: not searching again.");
            true
        } else {
            false
        }
    }

    pub(super) fn tick_requeue(&mut self) {
        if let Some(at) = self.requeue_at
            && self.now >= at
        {
            self.requeue_at = None;
            if self.partner == PartnerState::None && self.is_online() {
                self.request_find();
            }
        }
    }

    /// Why this partner breaks the user's skip rules, if they do.
    pub fn skip_reason(&self, info: &PartnerInfo) -> Option<(SkipRule, String)> {
        let rules = &self.config.settings.skip;
        if !rules.enabled {
            return None;
        }
        let prefs = &self.config.active().preferences;
        let theirs = info.kink_list();
        if let Some(limit) = theirs.iter().find(|k| prefs.limits.iter().any(|l| l == *k)) {
            return Some((SkipRule::Limit, format!("they're into {limit}, one of your limits")));
        }
        let open_ended = theirs.contains(&ANY) || prefs.kinks.iter().all(|k| k == ANY);
        if rules.min_shared_kinks > 0 && !open_ended {
            let shared = theirs.iter().filter(|k| prefs.kinks.iter().any(|m| m == *k)).count();
            if shared < rules.min_shared_kinks as usize {
                let noun = if shared == 1 { "kink" } else { "kinks" };
                return Some((SkipRule::SharedKinks, format!("only {shared} shared {noun}")));
            }
        }
        if rules.language_mismatch
            && let Some(lang) = &info.language
            && lang != ANY
            && prefs.language != ANY
            && *lang != prefs.language
        {
            return Some((SkipRule::Language, format!("they chose {lang}")));
        }
        None
    }

    /// Apply the skip rules to a new match. Returns true if they were skipped.
    pub(super) fn auto_skip(&mut self, info: &PartnerInfo) -> bool {
        let Some((rule, reason)) = self.skip_reason(info) else {
            self.skips = 0;
            return false;
        };
        let max = self.config.settings.skip.max_in_a_row;
        if self.skips >= max {
            self.system(format!(
                "Auto-skipped {max} partners in a row, so this one stays ({reason}). Loosen your rules or press {} to skip.",
                self.keymap.label(crate::keymap::Action::Next)
            ));
            self.skips = 0;
            return false;
        }
        self.skips += 1;
        self.count(|s| s.skipped += 1);
        let mut record = crate::history::Record::new(Local::now(), info, Outcome::Skipped);
        record.skip = Some(rule);
        record.profile = self.config.active_profile.clone();
        self.remember(record);
        let who = format!("{} {} {}", info.role, info.gender, info.species);
        self.system(format!("Skipped a {who}: {reason}."));
        self.buddy_event(crate::buddy::Event::Skipped);
        // Still waiting from the server's point of view, so just ask again.
        self.partner = PartnerState::Searching;
        self.find_partner();
        true
    }
}
