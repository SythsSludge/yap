//! Application state and behaviour, independent of the terminal and network.
//!
//! Inputs arrive through `on_*` methods; side effects (network sends, fetches, opening
//! URLs...) are queued as [`Effect`]s for the runtime to execute. That keeps nearly
//! everything here testable without a terminal or socket.

mod actions;
pub mod chat;
mod compose;
mod connection;
mod export;
mod keys;
mod kinks;
mod matching;
mod media;
pub mod modal;
mod mouse;
mod names;
mod palette;
mod profiles;
mod reload;
mod sessions;
pub mod settings;
mod snippets;
mod transcript;

pub use compose::{join_paragraphs, split_paragraphs};
pub use kinks::KinkGroups;
pub use mouse::{Hit, ListId, ViewerButton};
pub use palette::{PaletteEntry, PaletteItem};
pub use sessions::{Session, SessionSummary};
pub use transcript::quote;

pub use keys::prefs_options;

use crate::catalog::{ANY, MAX_MESSAGE_LEN};
use crate::commands::{self, Command, Parsed};
use crate::config::{self, Config, Paths};
use crate::drawer::Drawer;
use crate::images::{ImageState, Loaded, Viewer};
use crate::input::LineEditor;
use crate::keymap::Keymap;
use crate::links::{Trust, check_trust, find_links, looks_like_image, normalize_domain};
use crate::logs::Logs;
use crate::net::NetEvent;
use crate::prefs::{Field, Invalid};
use crate::protocol::{ClientMessage, PartnerInfo, ServerMessage};
use crate::theme::Theme;
use crate::traffic::{Direction, FrameKind, TrafficLog};
use chat::{Chat, EntryKind};
use chrono::Local;
use modal::{Confirm, Modal, Prompt, PromptAction};
use ratatui_image::picker::Picker;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Stop showing "typing" to the partner after this long without a keystroke.
pub const TYPING_IDLE: Duration = Duration::from_secs(5);
const TOAST_TTL: Duration = Duration::from_secs(6);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Messages sent this soon before a partner leaves may have been dropped.
const UNDELIVERED_WINDOW: chrono::TimeDelta = chrono::TimeDelta::seconds(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Chat,
    Preferences,
    Drawer,
    Logs,
    Traffic,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Chat, Tab::Preferences, Tab::Drawer, Tab::Logs, Tab::Traffic, Tab::Settings];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Chat => "Chat",
            Tab::Preferences => "Preferences",
            Tab::Drawer => "Drawer",
            Tab::Logs => "Logs",
            Tab::Traffic => "Traffic",
            Tab::Settings => "Settings",
        }
    }

    pub fn short_title(self) -> &'static str {
        match self {
            Tab::Preferences => "Prefs",
            Tab::Settings => "Setup",
            other => other.title(),
        }
    }
}

/// Side effects for the runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Open a fresh connection, replacing any existing one.
    Connect(String),
    /// Close the socket on purpose.
    CloseSocket,
    Send(ClientMessage),
    SendRaw(String),
    FetchImage {
        url: String,
        allow_host: Option<String>,
    },
    OpenUrl(String),
    /// Copy to the system clipboard (OSC 52).
    Copy(String),
    Notify {
        title: String,
        body: String,
    },
    Bell,
    /// Run a shell command that plays a notification sound.
    PlaySound(String),
    /// Suspend the UI and edit `text` in an external editor.
    OpenEditor {
        command: String,
        text: String,
        purpose: EditorPurpose,
    },
}

/// What text coming back from the external editor is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPurpose {
    /// The chat message box.
    Message,
    /// A drawer snippet, by index.
    Snippet(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnStatus {
    Idle,
    Connecting,
    Online,
    Offline { reason: String, retry_at: Option<Instant> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PartnerState {
    None,
    Searching,
    Connected(PartnerInfo),
}

/// When things happened with the current partner, for the sidebar's timers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    /// Matched with the current partner.
    pub partner_since: Option<Instant>,
    /// The partner's last message.
    pub last_heard: Option<Instant>,
    /// The partner started typing.
    pub typing_since: Option<Instant>,
    /// Started looking for a partner.
    pub searching_since: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub level: Level,
    pub expires: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsPane {
    Profiles,
    Fields,
    Options,
}

#[derive(Debug, Clone)]
pub struct PrefsUi {
    pub pane: PrefsPane,
    pub profile: usize,
    pub field: usize,
    pub option: usize,
    pub filter: String,
}

#[derive(Debug, Clone, Default)]
pub struct ListUi {
    pub selected: usize,
    pub filter: String,
    /// Keystrokes go to the filter while set.
    pub filtering: bool,
}

#[derive(Debug, Clone)]
pub struct TrafficUi {
    pub list: ListUi,
    /// Track the newest frame.
    pub follow: bool,
    pub pretty: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LogsUi {
    pub list: ListUi,
    /// Keys scroll the transcript rather than the list.
    pub reading: bool,
    /// Top line of the transcript view.
    pub scroll: usize,
}

/// The drawer holds two shelves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shelf {
    #[default]
    Links,
    Snippets,
}

#[derive(Debug, Clone, Default)]
pub struct DrawerUi {
    pub shelf: Shelf,
    pub list: ListUi,
    /// Only show items with this tag.
    pub tag: Option<String>,
    pub snippets: ListUi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatFocus {
    Input,
    Drawer,
}

pub struct App {
    pub paths: Paths,
    pub config: Config,
    pub themes: Vec<Theme>,
    pub theme: Theme,
    pub drawer: Drawer,
    // ----- the visible session (see `sessions.rs`) -----
    pub session_id: u64,
    pub status: ConnStatus,
    pub token: Option<String>,
    pub users_online: Option<u64>,
    reconnect_attempt: u32,
    /// The socket is being closed on purpose; don't auto-reconnect.
    closing: bool,
    pub partner: PartnerState,
    /// The server remembers our last partner, so they can still be blocked after leaving.
    pub can_block_previous: bool,
    pub chat: Chat,
    pub input: LineEditor,
    typing_sent: bool,
    last_edit: Option<Instant>,
    pub unseen: usize,
    requeue_at: Option<Instant>,
    skips: u32,
    pub partner_nick: Option<String>,
    pub clock: Clock,
    /// Sessions not on screen.
    pub others: Vec<Session>,
    next_session: u64,
    /// Set while a parked session is swapped in to handle one of its events.
    background: bool,
    /// The visible session, set aside while a parked one is swapped in.
    displaced: Option<Session>,
    // ----- global -----
    pub tab: Tab,
    pub chat_focus: ChatFocus,
    pub drawer_panel: bool,
    pub modal: Option<Modal>,
    pub viewer: Option<Viewer>,
    pub toasts: Vec<Toast>,
    pub prefs_ui: PrefsUi,
    pub drawer_ui: DrawerUi,
    pub keymap: Keymap,
    pub logs: Logs,
    /// All-time stats (persisted) and this run's.
    pub stats: crate::stats::Stats,
    pub run_stats: crate::stats::Stats,
    dirty_stats: bool,
    pub logs_ui: LogsUi,
    /// A resizable copy of the drawer's selected image, for its preview pane.
    pub drawer_preview: Option<(String, ratatui_image::protocol::StatefulProtocol)>,
    pub traffic: TrafficLog,
    pub traffic_ui: TrafficUi,
    pub settings_ui: ListUi,
    pub images: HashMap<String, ImageState>,
    pub picker: Picker,
    pub focused: bool,
    alert: Option<(String, Instant)>,
    pub now: Instant,
    /// `--server` from the command line: used for this session, never saved.
    pub server_override: Option<String>,
    /// Effects tagged with the session that produced them.
    effects: Vec<(u64, Effect)>,
    /// Clickable areas from the last frame (see `mouse.rs`).
    pub hits: std::cell::RefCell<Vec<(ratatui::layout::Rect, Hit)>>,
    last_click: Option<(Hit, Instant)>,
    pub quit: bool,
    dirty_config: bool,
    dirty_drawer: bool,
    /// Outside edits to the config and themes (see `reload.rs`).
    watch: reload::Watch,
}

impl App {
    pub fn new(paths: Paths, config: Config, drawer: Drawer, themes: Vec<Theme>, picker: Picker) -> Self {
        let theme =
            effective_theme(pick_theme(&themes, &config.settings.theme), config.settings.transparent_background);
        let traffic = TrafficLog::new(config.settings.traffic.capacity);
        let watch = reload::Watch::new(&paths);
        let profile = config.profiles.iter().position(|p| p.name == config.active_profile).unwrap_or(0);
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
            partner_nick,
            clock,
        } = Session::new(0);
        App {
            paths,
            themes,
            theme,
            drawer,
            session_id: id,
            status,
            token,
            users_online: None,
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
            partner_nick,
            clock,
            others: Vec::new(),
            next_session: 1,
            background: false,
            displaced: None,
            tab: Tab::Chat,
            chat_focus: ChatFocus::Input,
            drawer_panel: false,
            modal: None,
            viewer: None,
            toasts: Vec::new(),
            prefs_ui: PrefsUi { pane: PrefsPane::Fields, profile, field: 0, option: 0, filter: String::new() },
            drawer_ui: DrawerUi::default(),
            keymap: Keymap::build(&config.settings.keys).0,
            logs: Logs::default(),
            stats: crate::stats::Stats::starting(Local::now()),
            run_stats: crate::stats::Stats::starting(Local::now()),
            dirty_stats: false,
            logs_ui: LogsUi::default(),
            drawer_preview: None,
            traffic,
            traffic_ui: TrafficUi { list: ListUi::default(), follow: true, pretty: true },
            settings_ui: ListUi::default(),
            images: HashMap::new(),
            picker,
            focused: true,
            alert: None,
            now: Instant::now(),
            server_override: None,
            effects: Vec::new(),
            hits: Default::default(),
            last_click: None,
            quit: false,
            dirty_config: false,
            dirty_drawer: false,
            watch,
            config,
        }
    }

    // ----- plumbing -------------------------------------------------------------

    /// Pending effects, without their session tags (handy in tests).
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects).into_iter().map(|(_, e)| e).collect()
    }

    /// Pending effects with the id of the session that produced each.
    pub fn take_tagged_effects(&mut self) -> Vec<(u64, Effect)> {
        std::mem::take(&mut self.effects)
    }

    fn effect(&mut self, e: Effect) {
        self.effects.push((self.session_id, e));
    }

    pub fn toast(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        self.toasts.retain(|t| t.text != text);
        self.toasts.push(Toast { text, level, expires: self.now + TOAST_TTL });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    /// Add a line to the chat view and to the live conversation's log.
    fn push(&mut self, kind: EntryKind) {
        let at = Local::now();
        if matches!(kind, EntryKind::Partner(_)) && !self.chat_visible() {
            self.unseen += 1;
        }
        self.chat.push(at, kind.clone());
        self.logs.record(self.session_id, chat::Entry { at, kind });
    }

    fn system(&mut self, text: impl Into<String>) {
        self.push(EntryKind::System(text.into()));
    }

    /// The current partner is gone: close their conversation in the logs and count it.
    pub(super) fn end_conversation(&mut self) {
        let now = Local::now();
        if let Some(conv) = self.logs.live(self.session_id) {
            let secs = (now - conv.started).num_seconds().max(0) as u64;
            self.count(|s| s.chat_ended(secs));
        }
        self.logs.end(self.session_id, now);
    }

    /// Update both the all-time and this-run stats.
    pub(super) fn count(&mut self, f: impl Fn(&mut crate::stats::Stats)) {
        f(&mut self.stats);
        f(&mut self.run_stats);
        self.dirty_stats = true;
    }

    pub fn config_changed(&mut self) {
        self.dirty_config = true;
    }

    pub fn drawer_changed(&mut self) {
        self.dirty_drawer = true;
    }

    /// Persist anything that changed. Called by the runtime after each batch of events.
    pub fn flush(&mut self) {
        if std::mem::take(&mut self.dirty_config) {
            match self.config.save(&self.paths.config_file) {
                Ok(()) => self.config_saved(),
                Err(e) => self.toast(Level::Error, format!("couldn't save settings: {e:#}")),
            }
        }
        if std::mem::take(&mut self.dirty_drawer)
            && let Err(e) = self.drawer.save(&self.paths.drawer_file)
        {
            self.toast(Level::Error, format!("couldn't save drawer: {e:#}"));
        }
        if std::mem::take(&mut self.dirty_stats)
            && let Err(e) = self.stats.save(&self.paths.stats_file)
        {
            self.toast(Level::Error, format!("couldn't save stats: {e:#}"));
        }
        if self.config.settings.save_logs
            && let Err(e) = self.logs.save(&self.paths.logs_dir)
        {
            // Turn it off rather than retrying (and toasting) on every frame.
            self.config.settings.save_logs = false;
            self.config_changed();
            self.toast(Level::Error, format!("Chat logging turned off: {e:#}"));
        }
    }

    pub fn is_online(&self) -> bool {
        self.status == ConnStatus::Online
    }

    pub fn has_partner(&self) -> bool {
        matches!(self.partner, PartnerState::Connected(_))
    }

    fn send(&mut self, msg: ClientMessage) -> bool {
        if !self.is_online() {
            self.toast(Level::Error, "You're not connected to the server.");
            return false;
        }
        self.effect(Effect::Send(msg));
        true
    }
}

/// Apply display-only tweaks (transparency) to a theme.
fn effective_theme(mut theme: Theme, transparent: bool) -> Theme {
    if transparent {
        theme.bg = ratatui::style::Color::Reset;
    }
    theme
}

fn pick_theme(themes: &[Theme], name: &str) -> Theme {
    themes
        .iter()
        .find(|t| t.name == name)
        .or_else(|| themes.iter().find(|t| t.name == "dark"))
        .or(themes.first())
        .cloned()
        .unwrap_or_else(|| crate::theme::builtins().remove(0))
}

#[cfg(test)]
pub(crate) mod tests;
