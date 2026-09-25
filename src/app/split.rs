//! Split view: two chats side by side, or a chat beside the traffic log.

use super::*;

/// What the right-hand pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Session(u64),
    Traffic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    /// The chat on the left.
    pub left: u64,
    pub right: Pane,
}

impl Split {
    pub fn shows(&self, session: u64) -> bool {
        self.left == session || self.right == Pane::Session(session)
    }
}

impl App {
    /// alt+v: off → another chat beside this one → the traffic log beside it → off.
    pub fn toggle_split(&mut self) {
        self.tab = Tab::Chat;
        self.split = match self.split {
            None => {
                let other = self.others.iter().map(|s| s.id).find(|&id| id != self.session_id);
                let right = other.map_or(Pane::Traffic, Pane::Session);
                Some(Split { left: self.session_id, right })
            }
            Some(Split { right: Pane::Session(_), left }) => {
                if self.session_id != left {
                    self.switch_session(left);
                }
                Some(Split { left, right: Pane::Traffic })
            }
            Some(Split { right: Pane::Traffic, .. }) => None,
        };
        let note = match self.split {
            None => "Split view off.".to_owned(),
            Some(Split { right: Pane::Traffic, .. }) => "Traffic log beside this chat.".to_owned(),
            Some(Split { right: Pane::Session(_), .. }) => {
                let key = self.keymap.hint(crate::keymap::Action::OtherPane).unwrap_or_else(|| "a click".into());
                format!("Two chats side by side. {key} switches between them.")
            }
        };
        self.toast(Level::Info, note);
    }

    /// alt+o: type in the other chat of the split.
    pub fn focus_other_pane(&mut self) {
        match self.split {
            Some(Split { left, right: Pane::Session(right) }) => {
                let target = if self.session_id == left { right } else { left };
                self.switch_session(target);
            }
            Some(Split { right: Pane::Traffic, .. }) => {
                self.toast(Level::Info, "The traffic pane is just for watching.");
            }
            None => {}
        }
    }

    /// Whether chat `id` is on screen in the split (so it counts as seen).
    pub fn split_shows(&self, id: u64) -> bool {
        self.tab == Tab::Chat && self.split.is_some_and(|s| s.shows(id))
    }

    /// `to` is replacing `from` on screen: if `to` isn't in a pane already, it takes
    /// the pane `from` was in.
    pub(super) fn place_in_split(&mut self, from: u64, to: u64) {
        let Some(split) = &mut self.split else { return };
        if split.shows(to) {
            return;
        }
        if split.left == from {
            split.left = to;
        } else if split.right == Pane::Session(from) {
            split.right = Pane::Session(to);
        }
    }

    /// After a chat closes, keep both panes pointing at chats that exist.
    pub(super) fn fix_split(&mut self) {
        let ids = self.all_ids();
        let visible = self.session_id;
        let Some(split) = &mut self.split else { return };
        if !ids.contains(&split.left) {
            split.left = visible;
        }
        if let Pane::Session(r) = split.right
            && (!ids.contains(&r) || r == split.left)
        {
            split.right = if split.left == visible { Pane::Traffic } else { Pane::Session(visible) };
        }
        if !split.shows(visible) {
            split.left = visible;
        }
    }
}
