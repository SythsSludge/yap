//! Several chats at once. Each session is its own socket, and so its own anonymous
//! user as far as the server is concerned (just like two browser tabs).
//!
//! The visible session's state lives directly on [`App`] so the rest of the code can
//! simply say `self.chat` or `self.partner`. Other sessions are parked in
//! `App::others`, and swapped in whenever one of their events needs handling.

use super::*;

/// Per-session state. The field list is matched exhaustively in [`App::exchange`] and
/// [`App::new`], so adding a field here without handling it there won't compile.
#[derive(Debug)]
pub struct Session {
    pub id: u64,
    pub status: ConnStatus,
    pub token: Option<String>,
    pub reconnect_attempt: u32,
    pub closing: bool,
    pub partner: PartnerState,
    pub can_block_previous: bool,
    pub chat: Chat,
    pub input: LineEditor,
    pub typing_sent: bool,
    pub last_edit: Option<Instant>,
    /// Partner messages that arrived while this chat wasn't on screen.
    pub unseen: usize,
    /// When to automatically search again after a partner leaves.
    pub requeue_at: Option<Instant>,
    /// Consecutive partners skipped by the auto-skip rules.
    pub skips: u32,
}

impl Session {
    pub fn new(id: u64) -> Self {
        Session {
            id,
            status: ConnStatus::Idle,
            token: None,
            reconnect_attempt: 0,
            closing: false,
            partner: PartnerState::None,
            can_block_previous: false,
            chat: Chat::default(),
            input: LineEditor::new(),
            typing_sent: false,
            last_edit: None,
            unseen: 0,
            requeue_at: None,
            skips: 0,
        }
    }
}

/// What the session strip needs to show for one session.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub id: u64,
    /// 1-based position, as shown to the user.
    pub number: usize,
    pub active: bool,
    pub label: String,
    pub unseen: usize,
    pub online: bool,
}

fn describe(status: &ConnStatus, partner: &PartnerState) -> String {
    match (status, partner) {
        (_, PartnerState::Connected(info)) => format!("{} {}", info.gender, info.species).to_lowercase(),
        (_, PartnerState::Searching) => "searching".into(),
        (ConnStatus::Online, PartnerState::None) => "idle".into(),
        (ConnStatus::Connecting, _) => "connecting".into(),
        _ => "offline".into(),
    }
}

impl App {
    /// Swap the visible session's state with `other`.
    pub(super) fn exchange(&mut self, other: &mut Session) {
        let Session {
            id,
            status,
            token,
            reconnect_attempt,
            closing,
            partner,
            can_block_previous,
            chat,
            input,
            typing_sent,
            last_edit,
            unseen,
            requeue_at,
            skips,
        } = other;
        std::mem::swap(&mut self.session_id, id);
        std::mem::swap(&mut self.status, status);
        std::mem::swap(&mut self.token, token);
        std::mem::swap(&mut self.reconnect_attempt, reconnect_attempt);
        std::mem::swap(&mut self.closing, closing);
        std::mem::swap(&mut self.partner, partner);
        std::mem::swap(&mut self.can_block_previous, can_block_previous);
        std::mem::swap(&mut self.chat, chat);
        std::mem::swap(&mut self.input, input);
        std::mem::swap(&mut self.typing_sent, typing_sent);
        std::mem::swap(&mut self.last_edit, last_edit);
        std::mem::swap(&mut self.unseen, unseen);
        std::mem::swap(&mut self.requeue_at, requeue_at);
        std::mem::swap(&mut self.skips, skips);
    }

    /// Run `f` with session `id` swapped in as the current one. Returns `None` if there's
    /// no such session (e.g. an event for a chat that was just closed).
    pub fn with_session<R>(&mut self, id: u64, f: impl FnOnce(&mut App) -> R) -> Option<R> {
        if id == self.session_id {
            return Some(f(self));
        }
        let index = self.others.iter().position(|s| s.id == id)?;
        let mut parked = self.others.remove(index);
        self.exchange(&mut parked);
        // `parked` now holds the visible session; keep it where counts can see it.
        self.displaced = Some(parked);
        self.background = true;
        let result = f(self);
        self.background = false;
        let mut parked = self.displaced.take().expect("set above");
        self.exchange(&mut parked);
        self.others.insert(index, parked);
        Some(result)
    }

    /// Whether the current session is on screen (as opposed to being handled in the
    /// background, or the user looking at another tab).
    pub fn chat_visible(&self) -> bool {
        !self.background && self.tab == Tab::Chat
    }

    /// Route a network event to the session whose socket produced it.
    pub fn on_net_for(&mut self, session: u64, event: NetEvent) {
        self.with_session(session, |app| app.on_net(event));
    }

    pub fn session_count(&self) -> usize {
        self.others.len() + 1 + usize::from(self.displaced.is_some())
    }

    fn all_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> =
            self.others.iter().chain(&self.displaced).map(|s| s.id).chain([self.session_id]).collect();
        ids.sort_unstable();
        ids
    }

    /// Every session, in the order they were opened.
    pub fn sessions(&self) -> Vec<SessionSummary> {
        let mut all: Vec<SessionSummary> = self
            .others
            .iter()
            .map(|s| SessionSummary {
                id: s.id,
                number: 0,
                active: false,
                label: describe(&s.status, &s.partner),
                unseen: s.unseen,
                online: s.status == ConnStatus::Online,
            })
            .chain([SessionSummary {
                id: self.session_id,
                number: 0,
                active: true,
                label: describe(&self.status, &self.partner),
                unseen: self.unseen,
                online: self.is_online(),
            }])
            .collect();
        all.sort_by_key(|s| s.id);
        for (i, s) in all.iter_mut().enumerate() {
            s.number = i + 1;
        }
        all
    }

    /// The 1-based position of the visible session.
    pub fn session_number(&self) -> usize {
        self.all_ids().iter().position(|&id| id == self.session_id).map_or(1, |i| i + 1)
    }

    /// Bring session `id` on screen.
    pub fn switch_session(&mut self, id: u64) {
        if id == self.session_id {
            return;
        }
        let Some(index) = self.others.iter().position(|s| s.id == id) else { return };
        let mut target = self.others.remove(index);
        self.exchange(&mut target);
        self.others.push(target);
        self.others.sort_by_key(|s| s.id);
        self.unseen = 0;
        self.tab = Tab::Chat;
    }

    /// Step to the next (`1`) or previous (`-1`) session, wrapping around.
    pub fn cycle_session(&mut self, delta: isize) {
        let ids = self.all_ids();
        if ids.len() < 2 {
            return self.toast(Level::Info, "Only one chat open. New chat: /new");
        }
        let pos = ids.iter().position(|&id| id == self.session_id).unwrap_or(0) as isize;
        let next = ids[(pos + delta).rem_euclid(ids.len() as isize) as usize];
        self.switch_session(next);
    }

    /// Open another chat with its own connection and switch to it.
    pub fn new_session(&mut self) {
        let mut fresh = Session::new(self.next_session);
        self.next_session += 1;
        self.exchange(&mut fresh);
        self.others.push(fresh);
        self.others.sort_by_key(|s| s.id);
        self.tab = Tab::Chat;
        self.connect();
        self.toast(Level::Info, format!("Opened chat {}.", self.session_number()));
    }

    /// Close the visible chat (asking first if a partner is still there).
    pub fn request_close_session(&mut self) {
        if self.others.is_empty() {
            return self.toast(Level::Info, "This is the only chat. Quit instead?");
        }
        if self.has_partner() {
            self.confirm_or("You're still chatting in this tab. Close it?", Confirm::CloseSession);
        } else {
            self.close_session();
        }
    }

    pub(super) fn close_session(&mut self) {
        if self.others.is_empty() {
            return;
        }
        self.closing = true;
        self.effect(Effect::CloseSocket);
        self.logs.end(self.session_id, Local::now());
        let number = self.session_number();
        // Bring the nearest other session forward; the closed one is dropped.
        let index = self.others.iter().rposition(|s| s.id < self.session_id).unwrap_or(0);
        let mut next = self.others.remove(index);
        self.exchange(&mut next);
        drop(next);
        self.unseen = 0;
        self.toast(Level::Info, format!("Closed chat {number}."));
    }

    /// Whether any session has a partner (used before quitting).
    pub fn any_partner(&self) -> bool {
        self.has_partner() || self.others.iter().any(|s| matches!(s.partner, PartnerState::Connected(_)))
    }

    /// Run per-session timers for every session.
    pub(super) fn tick_sessions(&mut self) {
        for id in self.all_ids() {
            self.with_session(id, App::tick_session);
        }
    }

    /// Clear the unseen count once the chat is actually on screen.
    pub fn mark_seen(&mut self) {
        if self.tab == Tab::Chat {
            self.unseen = 0;
        }
    }
}
