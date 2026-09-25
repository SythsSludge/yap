//! Application state and behaviour, independent of the terminal and network.
//!
//! Inputs arrive through `on_*` methods; side effects (network sends, fetches, opening
//! URLs...) are queued as [`Effect`]s for the runtime to execute. That keeps nearly
//! everything here testable without a terminal or socket.

pub mod chat;
mod keys;
pub mod modal;
pub mod settings;

pub use keys::prefs_options;

use crate::catalog::{ANY, MAX_MESSAGE_LEN};
use crate::commands::{self, Command, Parsed};
use crate::config::{self, Config, Paths};
use crate::drawer::Drawer;
use crate::images::{ImageState, Loaded, Viewer};
use crate::input::LineEditor;
use crate::links::{Trust, check_trust, find_links, looks_like_image, normalize_domain};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Chat,
    Preferences,
    Drawer,
    Traffic,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Chat, Tab::Preferences, Tab::Drawer, Tab::Traffic, Tab::Settings];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Chat => "Chat",
            Tab::Preferences => "Preferences",
            Tab::Drawer => "Drawer",
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
    pub tab: Tab,
    pub chat_focus: ChatFocus,
    pub drawer_panel: bool,
    pub modal: Option<Modal>,
    pub viewer: Option<Viewer>,
    pub toasts: Vec<Toast>,
    pub prefs_ui: PrefsUi,
    pub drawer_ui: ListUi,
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
    effects: Vec<Effect>,
    pub quit: bool,
    dirty_config: bool,
    dirty_drawer: bool,
}

impl App {
    pub fn new(paths: Paths, config: Config, drawer: Drawer, themes: Vec<Theme>, picker: Picker) -> Self {
        let theme = pick_theme(&themes, &config.settings.theme);
        let traffic = TrafficLog::new(config.settings.traffic.capacity);
        let profile = config.profiles.iter().position(|p| p.name == config.active_profile).unwrap_or(0);
        App {
            paths,
            themes,
            theme,
            drawer,
            status: ConnStatus::Idle,
            token: None,
            users_online: None,
            reconnect_attempt: 0,
            closing: false,
            partner: PartnerState::None,
            can_block_previous: false,
            chat: Chat::default(),
            input: LineEditor::new(),
            typing_sent: false,
            last_edit: None,
            tab: Tab::Chat,
            chat_focus: ChatFocus::Input,
            drawer_panel: false,
            modal: None,
            viewer: None,
            toasts: Vec::new(),
            prefs_ui: PrefsUi { pane: PrefsPane::Fields, profile, field: 0, option: 0, filter: String::new() },
            drawer_ui: ListUi::default(),
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
            quit: false,
            dirty_config: false,
            dirty_drawer: false,
            config,
        }
    }

    // ----- plumbing -------------------------------------------------------------

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    fn effect(&mut self, e: Effect) {
        self.effects.push(e);
    }

    pub fn toast(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        self.toasts.retain(|t| t.text != text);
        self.toasts.push(Toast { text, level, expires: self.now + TOAST_TTL });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn system(&mut self, text: impl Into<String>) {
        self.chat.push(Local::now(), EntryKind::System(text.into()));
    }

    pub fn config_changed(&mut self) {
        self.dirty_config = true;
    }

    pub fn drawer_changed(&mut self) {
        self.dirty_drawer = true;
    }

    /// Persist anything that changed. Called by the runtime after each batch of events.
    pub fn flush(&mut self) {
        if std::mem::take(&mut self.dirty_config)
            && let Err(e) = self.config.save(&self.paths.config_file)
        {
            self.toast(Level::Error, format!("couldn't save settings: {e:#}"));
        }
        if std::mem::take(&mut self.dirty_drawer)
            && let Err(e) = self.drawer.save(&self.paths.drawer_file)
        {
            self.toast(Level::Error, format!("couldn't save drawer: {e:#}"));
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

    // ----- connection -----------------------------------------------------------

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

    fn reset_partner(&mut self) {
        self.partner = PartnerState::None;
        self.chat.partner_typing = false;
        self.typing_sent = false;
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
                self.chat.push(
                    Local::now(),
                    EntryKind::Warning(format!("Couldn't read a server frame ({error}): {preview}")),
                );
            }
            NetEvent::Closed { reason } => self.on_closed(reason),
        }
    }

    fn on_closed(&mut self, reason: String) {
        let was_partnered = self.has_partner() || self.partner == PartnerState::Searching;
        self.reset_partner();
        self.can_block_previous = false;
        self.users_online = None;
        if std::mem::take(&mut self.closing) {
            self.status = ConnStatus::Offline { reason, retry_at: None };
            return;
        }
        let retry_at = self.config.settings.auto_reconnect.then(|| self.now + self.backoff());
        if was_partnered || !matches!(self.status, ConnStatus::Offline { .. }) {
            let hint = if retry_at.is_some() { "Reconnecting…" } else { "Press Ctrl-R to reconnect." };
            self.chat.push(Local::now(), EntryKind::Warning(format!("Disconnected from the server: {reason}. {hint}")));
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
                self.request_images(&text);
                self.chat.push(Local::now(), EntryKind::Partner(text));
                if self.config.settings.notify.on_message {
                    self.alert("New Message");
                }
            }
            ServerMessage::PartnerTyping(on) => {
                self.chat.partner_typing = on && self.has_partner();
            }
            ServerMessage::PartnerConnected(info) => self.on_partner_connected(info),
            ServerMessage::PartnerPending => {
                self.partner = PartnerState::Searching;
                self.system(
                    "We are looking for a partner to match you with. \
                     Please either continue to wait, or modify your yiffing preferences.",
                );
            }
            ServerMessage::PartnerLeft => {
                self.reset_partner();
                self.can_block_previous = true;
                self.system("Your yiffing partner has left.");
                self.alert("Partner Left");
            }
            ServerMessage::PartnerDisconnected => {
                self.reset_partner();
                self.can_block_previous = false;
                self.system("Your yiffing partner has disconnected unexpectedly.");
                self.alert("Partner Disconnected");
            }
            ServerMessage::PartnerBlocked => {
                if self.has_partner() {
                    self.system("Your partner has been blocked and disconnected from you.");
                } else {
                    self.system("Your previous partner has been blocked.");
                }
                self.reset_partner();
                self.can_block_previous = false;
            }
            ServerMessage::ClientDisconnect => {
                self.reset_partner();
                self.can_block_previous = true;
                self.system("You have disconnected from your partner.");
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
                self.chat.push(
                    Local::now(),
                    EntryKind::Warning(format!(
                        "The server sent an unrecognised `{kind}` message (see the Traffic tab)."
                    )),
                );
            }
        }
    }

    fn on_partner_connected(&mut self, info: PartnerInfo) {
        let info = PartnerInfo {
            gender: crate::text::sanitize(&info.gender),
            species: crate::text::sanitize(&info.species),
            kinks: crate::text::sanitize(&info.kinks),
            role: crate::text::sanitize(&info.role),
            language: info.language.as_deref().map(crate::text::sanitize),
        };
        let mine = &self.config.active().preferences.kinks;
        let common = info
            .kink_list()
            .into_iter()
            .filter(|k| *k != ANY && mine.iter().any(|m| m == k))
            .map(str::to_owned)
            .collect();
        self.reset_partner();
        self.system("You have been connected with a yiffing partner.");
        if let Some(lang) = &info.language {
            self.system(format!("Your partner's language is {lang}"));
        }
        self.chat.push(Local::now(), EntryKind::PartnerInfo { info: info.clone(), common });
        self.partner = PartnerState::Connected(info);
        self.can_block_previous = false;
        self.tab = Tab::Chat;
        self.alert("Partner Connected");
    }

    /// Attention-grabbing, but only while the terminal is in the background, like the
    /// website only flashing its tab when hidden.
    fn alert(&mut self, what: &str) {
        if self.focused {
            return;
        }
        let notify = self.config.settings.notify.clone();
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

    // ----- user actions ---------------------------------------------------------

    fn confirm_or(&mut self, text: &str, action: Confirm) {
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

    fn show_invalid(&mut self, invalid: Invalid) {
        self.toast(Level::Error, invalid.to_string());
        self.tab = Tab::Preferences;
        self.prefs_ui.pane = PrefsPane::Fields;
        self.prefs_ui.field = Field::ALL.iter().position(|f| *f == invalid.field()).unwrap_or(0);
    }

    fn find_partner(&mut self) {
        let wire = match self.config.active().preferences.to_wire(self.config.settings.send_language) {
            Ok(w) => w,
            Err(invalid) => return self.show_invalid(invalid),
        };
        let had_partner = self.has_partner();
        if !self.send(ClientMessage::FindPartner(wire)) {
            return;
        }
        if had_partner {
            self.system("You have disconnected from your previous partner.");
        }
        self.reset_partner();
        self.can_block_previous = had_partner;
        self.partner = PartnerState::Searching;
        self.tab = Tab::Chat;
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
        if self.has_partner() {
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
                self.run_command(cmd);
            }
            Parsed::Error(e) => self.toast(Level::Error, e),
        }
    }

    fn send_chat(&mut self, text: String) {
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
        self.input.submit();
        self.request_images(&text);
        self.chat.push(Local::now(), EntryKind::You(text));
        self.chat.follow();
    }

    /// Keep the partner's "typing…" indicator honest: on when we start, off when we
    /// clear the box or go idle. (The web client sends `true` on every keystroke and
    /// never turns it off unless a message is sent.)
    pub fn input_changed(&mut self) {
        if !self.has_partner() || !self.is_online() {
            return;
        }
        if self.input.is_empty() {
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
        }
    }

    pub fn send_raw(&mut self, frame: String) {
        if self.is_online() {
            self.effect(Effect::SendRaw(frame));
        } else {
            self.toast(Level::Error, "You're not connected to the server.");
        }
    }

    // ----- profiles, themes, import/export --------------------------------------

    pub fn switch_profile(&mut self, name: &str) {
        match self.config.set_active(name) {
            Ok(()) => {
                self.prefs_ui.profile = self.config.profiles.iter().position(|p| p.name == name).unwrap_or(0);
                self.config_changed();
                self.toast(Level::Success, format!("Using profile `{name}`."));
            }
            Err(e) => self.toast(Level::Error, e.to_string()),
        }
    }

    pub fn open_profile_picker(&mut self) {
        let selected = self.config.profiles.iter().position(|p| p.name == self.config.active_profile).unwrap_or(0);
        self.modal = Some(Modal::Profiles { selected });
    }

    pub fn open_theme_picker(&mut self) {
        let selected = self.themes.iter().position(|t| t.name == self.theme.name).unwrap_or(0);
        self.modal = Some(Modal::Themes { selected, original: self.theme.name.clone() });
    }

    /// Switch theme; `persist` is false while previewing in the picker.
    pub fn set_theme(&mut self, name: &str, persist: bool) {
        match self.themes.iter().find(|t| t.name == name) {
            Some(t) => {
                self.theme = t.clone();
                if persist {
                    self.config.settings.theme = name.to_owned();
                    self.config_changed();
                }
            }
            None => {
                let names: Vec<_> = self.themes.iter().map(|t| t.name.as_str()).collect();
                self.toast(Level::Error, format!("No theme `{name}`. Try: {}", names.join(", ")));
            }
        }
    }

    pub fn export(&mut self, profile: Option<&str>, path: &str) {
        let path = config::expand_tilde(path);
        let result = self
            .config
            .export(profile)
            .map_err(anyhow::Error::from)
            .and_then(|doc| config::export_to_file(&doc, &path));
        match result {
            Ok(()) => self.toast(Level::Success, format!("Exported to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Export failed: {e:#}")),
        }
    }

    pub fn import(&mut self, path: &str, with_settings: bool) {
        let path = config::expand_tilde(path);
        match config::read_import(&path) {
            Ok(doc) => {
                let report = self.config.import(doc, with_settings);
                self.config_changed();
                if report.settings_applied {
                    let theme = self.config.settings.theme.clone();
                    self.theme = pick_theme(&self.themes, &theme);
                    self.traffic.set_capacity(self.config.settings.traffic.capacity);
                }
                let mut msg = match report.profiles.len() {
                    0 => "Imported no profiles".to_owned(),
                    _ => format!("Imported {}", report.profiles.join(", ")),
                };
                if report.settings_applied {
                    msg.push_str(" and settings");
                }
                self.toast(Level::Success, msg);
                for w in report.warnings {
                    self.toast(Level::Warning, w);
                }
            }
            Err(e) => self.toast(Level::Error, format!("Import failed: {e:#}")),
        }
    }

    fn save_log(&mut self, path: &str) {
        let path = config::expand_tilde(path);
        match config::write_atomic(&path, &self.chat.transcript()) {
            Ok(()) => self.toast(Level::Success, format!("Saved transcript to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Couldn't save transcript: {e:#}")),
        }
    }

    pub fn export_traffic(&mut self, path: &str) {
        let path = config::expand_tilde(path);
        let mut buf = Vec::new();
        let result = self
            .traffic
            .export(&mut buf)
            .map_err(anyhow::Error::from)
            .and_then(|n| config::write_atomic(&path, &String::from_utf8_lossy(&buf)).map(|()| n));
        match result {
            Ok(n) => self.toast(Level::Success, format!("Wrote {n} frames to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Traffic export failed: {e:#}")),
        }
    }

    /// Where file prompts start: `~/yap-<name>`.
    pub fn default_path(&self, name: &str) -> String {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().join(name))
            .unwrap_or_else(|| PathBuf::from(name))
            .display()
            .to_string()
    }

    // ----- drawer, links, images ------------------------------------------------

    pub fn add_to_drawer(&mut self, url: &str, label: &str) {
        match self.drawer.add(url, label, chrono::Utc::now()) {
            Ok(i) => {
                let label = self.drawer.items[i].label.clone();
                self.drawer_ui.selected = i;
                self.drawer_changed();
                self.toast(Level::Success, format!("Saved `{label}` to the drawer."));
            }
            Err(e) => self.toast(Level::Error, e.to_string()),
        }
    }

    pub fn toggle_drawer_panel(&mut self) {
        if self.tab != Tab::Chat {
            self.tab = Tab::Chat;
            self.drawer_panel = true;
        } else {
            self.drawer_panel = !self.drawer_panel;
        }
        self.chat_focus = if self.drawer_panel { ChatFocus::Drawer } else { ChatFocus::Input };
    }

    pub fn open_links(&mut self) {
        let links = self.chat.links();
        if links.is_empty() {
            self.toast(Level::Info, "No links in this chat yet.");
        } else {
            self.modal = Some(Modal::Links { links, selected: 0 });
        }
    }

    /// Put a link into the chat box, e.g. to share a ref sheet from the drawer.
    pub fn insert_into_input(&mut self, text: &str) {
        if !self.input.is_empty() && !self.input.text().ends_with(' ') {
            self.input.insert_char(' ');
        }
        self.input.insert_str(text);
        self.tab = Tab::Chat;
        self.chat_focus = ChatFocus::Input;
        self.input_changed();
    }

    pub fn open_url(&mut self, url: &str) {
        match url::Url::parse(url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => self.effect(Effect::OpenUrl(u.to_string())),
            _ => self.toast(Level::Error, "Only http(s) links can be opened."),
        }
    }

    pub fn copy(&mut self, text: &str) {
        self.effect(Effect::Copy(text.to_owned()));
        self.toast(Level::Info, "Copied to clipboard.");
    }

    pub fn trust_domain(&mut self, input: &str) {
        let Some(domain) = normalize_domain(input) else {
            return self.toast(Level::Error, format!("`{input}` isn't a domain."));
        };
        let list = &mut self.config.settings.images.trusted_domains;
        if list.contains(&domain) {
            return self.toast(Level::Info, format!("{domain} is already trusted."));
        }
        list.push(domain.clone());
        self.config_changed();
        self.toast(Level::Success, format!("Image previews enabled for {domain}."));
    }

    pub fn untrust_domain(&mut self, input: &str) {
        let domain = normalize_domain(input).unwrap_or_else(|| input.trim().to_owned());
        let list = &mut self.config.settings.images.trusted_domains;
        let before = list.len();
        list.retain(|d| *d != domain);
        if list.len() == before {
            self.toast(Level::Info, format!("{domain} wasn't trusted."));
        } else {
            self.config_changed();
            self.toast(Level::Success, format!("{domain} is no longer trusted."));
        }
    }

    /// Trusted, image-looking links in `text` that should get inline previews.
    pub fn preview_urls(&self, text: &str) -> Vec<String> {
        let s = &self.config.settings.images;
        if !s.enabled {
            return Vec::new();
        }
        find_links(text)
            .into_iter()
            .filter_map(|l| url::Url::parse(&l.url).ok().map(|u| (l.url, u)))
            .filter(|(_, u)| looks_like_image(u) && check_trust(u, &s.trusted_domains, s.https_only) == Trust::Trusted)
            .map(|(raw, _)| raw)
            .collect()
    }

    fn request_images(&mut self, text: &str) {
        if !self.config.settings.images.auto_load {
            return;
        }
        for url in self.preview_urls(text) {
            if !self.images.contains_key(&url) {
                self.images.insert(url.clone(), ImageState::Loading);
                self.effect(Effect::FetchImage { url, allow_host: None });
            }
        }
    }

    /// Show an image full-screen, fetching it if needed. Untrusted hosts need consent.
    pub fn preview(&mut self, url: &str) {
        let s = &self.config.settings.images;
        if !s.enabled {
            return self.toast(Level::Info, "Image previews are turned off in Settings.");
        }
        let Ok(parsed) = url::Url::parse(url) else {
            return self.toast(Level::Error, "That isn't a valid link.");
        };
        match check_trust(&parsed, &s.trusted_domains, s.https_only) {
            Trust::Trusted => self.open_viewer(url, None),
            Trust::UntrustedHost => {
                let host = parsed.host_str().unwrap_or("?");
                self.modal = Some(Modal::Confirm {
                    text: format!(
                        "{host} isn't a trusted image host. Loading it reveals your IP address to them. Load anyway?"
                    ),
                    action: Confirm::LoadUntrusted(url.to_owned()),
                });
            }
            Trust::InsecureScheme => self.toast(Level::Error, "Refusing to load an image over plain http."),
            Trust::NotHttp => self.toast(Level::Error, "Only http(s) images can be previewed."),
        }
    }

    fn open_viewer(&mut self, url: &str, allow_host: Option<String>) {
        let protocol = match self.images.get(url) {
            Some(ImageState::Ready(loaded)) => Some(self.picker.new_resize_protocol(loaded.image.clone())),
            Some(ImageState::Loading) => None,
            Some(ImageState::Failed(_)) | None => {
                self.images.insert(url.to_owned(), ImageState::Loading);
                self.effect(Effect::FetchImage { url: url.to_owned(), allow_host });
                None
            }
        };
        self.viewer = Some(Viewer { url: url.to_owned(), protocol });
    }

    pub fn on_image(&mut self, url: String, result: Result<Loaded, String>) {
        let state = match result {
            Ok(loaded) => {
                let loaded = Arc::new(loaded);
                if let Some(v) = self.viewer.as_mut().filter(|v| v.url == url && v.protocol.is_none()) {
                    v.protocol = Some(self.picker.new_resize_protocol(loaded.image.clone()));
                }
                ImageState::Ready(loaded)
            }
            Err(e) => {
                if self.viewer.as_ref().is_some_and(|v| v.url == url) {
                    self.viewer = None;
                    self.toast(Level::Error, format!("Couldn't load image: {e}"));
                }
                ImageState::Failed(e)
            }
        };
        self.images.insert(url, state);
    }

    /// Record a note in the traffic log (e.g. "you pressed reconnect").
    pub fn traffic_note(&mut self, text: &str) {
        self.traffic.push(Local::now(), Direction::Meta, FrameKind::Info, 0, text.into());
    }

    pub fn open_prompt(&mut self, title: &str, initial: &str, action: PromptAction) {
        self.modal =
            Some(Modal::Prompt(Prompt { title: title.into(), editor: LineEditor::with_text(initial), action }));
    }
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
