use super::modal::{Confirm, Modal};
use super::*;
use crate::config::Paths;
use crate::prefs::Preferences;
use crate::protocol::WirePreferences;
use pretty_assertions::assert_eq;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

pub(crate) struct Harness {
    pub app: App,
    pub _dir: tempfile::TempDir,
}

impl std::ops::Deref for Harness {
    type Target = App;
    fn deref(&self) -> &App {
        &self.app
    }
}

impl std::ops::DerefMut for Harness {
    fn deref_mut(&mut self) -> &mut App {
        &mut self.app
    }
}

pub(crate) fn complete_prefs() -> Preferences {
    let mut p = Preferences::default();
    p.toggle(Field::Gender, "Male");
    p.toggle(Field::Species, "Wolf");
    p.toggle(Field::Role, "Switch");
    p.toggle(Field::PartnerRole, "Dominant");
    p.toggle(Field::Kinks, "Biting");
    p.toggle(Field::Kinks, "Musk");
    p
}

pub(crate) fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(
        Paths::in_dir(dir.path()),
        Config::default(),
        Drawer::default(),
        crate::theme::builtins(),
        Picker::halfblocks(),
    );
    Harness { app, _dir: dir }
}

impl Harness {
    pub fn online(mut self) -> Self {
        self.on_net(NetEvent::Open);
        self.on_net(NetEvent::Message(ServerMessage::ConnectionSuccess { token: "tok".into() }));
        self.on_net(NetEvent::Message(ServerMessage::UserCount(42)));
        self.take_effects();
        self
    }

    pub fn with_prefs(mut self) -> Self {
        self.config.active_mut().preferences = complete_prefs();
        self
    }

    pub fn partnered(mut self) -> Self {
        self.server(ServerMessage::PartnerConnected(PartnerInfo {
            gender: "Female".into(),
            species: "Fox".into(),
            kinks: "Musk, Biting, Tickling".into(),
            role: "Dominant".into(),
            language: Some("English".into()),
        }));
        self.take_effects();
        self
    }

    pub fn server(&mut self, msg: ServerMessage) {
        self.on_net(NetEvent::Message(msg));
    }

    pub fn press(&mut self, code: KeyCode) {
        self.on_terminal(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    pub fn ctrl(&mut self, c: char) {
        self.on_terminal(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)));
    }

    pub fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    pub fn sent(&mut self) -> Vec<ClientMessage> {
        self.take_effects()
            .into_iter()
            .filter_map(|e| match e {
                Effect::Send(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    pub fn last_toast(&self) -> Option<&str> {
        self.toasts.last().map(|t| t.text.as_str())
    }

    pub fn last_entry(&self) -> &EntryKind {
        &self.chat.entries.last().expect("chat is empty").kind
    }
}

fn find_payload(sent: &[ClientMessage]) -> Option<&WirePreferences> {
    sent.iter().find_map(|m| match m {
        ClientMessage::FindPartner(p) => Some(p),
        _ => None,
    })
}

#[test]
fn find_with_incomplete_preferences_points_at_the_problem() {
    let mut h = harness().online();
    h.ctrl('f');
    assert_eq!(h.last_toast(), Some("Please select your gender."));
    assert_eq!(h.tab, Tab::Preferences);
    assert_eq!(h.prefs_ui.field, 0);
    assert!(h.sent().is_empty());
}

#[test]
fn find_requires_a_connection() {
    let mut h = harness().with_prefs();
    h.ctrl('f');
    assert_eq!(h.last_toast(), Some("You're not connected to the server."));
    assert!(h.sent().is_empty());
}

#[test]
fn full_partner_lifecycle_matches_web_client() {
    let mut h = harness().online().with_prefs();
    h.ctrl('f');
    let sent = h.sent();
    let wire = find_payload(&sent).expect("find_partner sent");
    assert_eq!(wire.user.species, "Wolf");
    assert_eq!(wire.kinks, vec!["Biting", "Musk"]);
    assert_eq!(h.partner, PartnerState::Searching);

    h.server(ServerMessage::PartnerPending);
    assert!(matches!(h.last_entry(), EntryKind::System(t) if t.starts_with("We are looking for a partner")));

    let mut h = h.partnered();
    assert!(h.has_partner());
    let EntryKind::PartnerInfo { common, .. } = h.last_entry() else { panic!("expected partner info") };
    assert_eq!(common, &vec!["Musk".to_string(), "Biting".to_string()]);
    assert!(h.chat.entries.iter().any(|e| e.kind == EntryKind::System("Your partner's language is English".into())));

    h.server(ServerMessage::PartnerTyping(true));
    assert!(h.chat.partner_typing);
    h.server(ServerMessage::ReceiveMessage("hey \x1b]0;pwned\x07there".into()));
    assert!(!h.chat.partner_typing);
    assert_eq!(h.last_entry(), &EntryKind::Partner("hey ]0;pwnedthere".into()));

    h.type_str("hello!");
    h.press(KeyCode::Enter);
    let sent = h.sent();
    assert_eq!(
        sent,
        vec![ClientMessage::Typing(true), ClientMessage::Typing(false), ClientMessage::SendMessage("hello!".into())]
    );
    assert_eq!(h.last_entry(), &EntryKind::You("hello!".into()));
    assert!(h.input.is_empty());

    h.server(ServerMessage::PartnerLeft);
    assert!(!h.has_partner());
    assert_eq!(h.last_entry(), &EntryKind::System("Your yiffing partner has left.".into()));
}

#[test]
fn send_validation_matches_web_client() {
    let mut h = harness().online().with_prefs();
    h.press(KeyCode::Enter);
    assert_eq!(h.last_toast(), Some("Please enter a message."));

    h.type_str("hi");
    h.press(KeyCode::Enter);
    assert_eq!(h.last_toast(), Some("You are not connected to a partner yet."));
    assert_eq!(h.input.text(), "hi", "text is kept when sending fails");

    let mut h = h.partnered();
    h.input.set(&"x".repeat(MAX_MESSAGE_LEN));
    h.press(KeyCode::Enter);
    assert_eq!(h.last_toast(), Some("Please shorten the length of your message."));
    // 1500 emoji are 3000 UTF-16 units, which the web client also rejects.
    h.input.set(&"🦊".repeat(1500));
    h.press(KeyCode::Enter);
    assert_eq!(h.last_toast(), Some("Please shorten the length of your message."));
    h.input.set(&"x".repeat(MAX_MESSAGE_LEN - 1));
    h.take_effects();
    h.press(KeyCode::Enter);
    assert!(h.sent().iter().any(|m| matches!(m, ClientMessage::SendMessage(_))));
}

#[test]
fn typing_indicator_is_sent_once_and_cleared() {
    let mut h = harness().online().with_prefs().partnered();
    h.type_str("abc");
    assert_eq!(h.sent(), vec![ClientMessage::Typing(true)]);
    h.press(KeyCode::Backspace);
    h.press(KeyCode::Backspace);
    assert!(h.sent().is_empty());
    h.press(KeyCode::Backspace);
    assert_eq!(h.sent(), vec![ClientMessage::Typing(false)]);

    h.type_str("x");
    h.take_effects();
    h.now += TYPING_IDLE;
    h.on_tick();
    assert_eq!(h.sent(), vec![ClientMessage::Typing(false)]);
    h.type_str("y");
    assert_eq!(h.sent(), vec![ClientMessage::Typing(true)]);
}

#[test]
fn no_typing_frames_without_a_partner() {
    let mut h = harness().online();
    h.type_str("hello");
    assert!(h.sent().is_empty());
}

#[test]
fn finding_a_new_partner_asks_first() {
    let mut h = harness().online().with_prefs().partnered();
    h.ctrl('f');
    assert!(matches!(h.modal, Some(Modal::Confirm { action: Confirm::FindNew, .. })));
    h.press(KeyCode::Char('n'));
    assert!(h.modal.is_none());
    assert!(h.sent().is_empty());
    assert!(h.has_partner());

    h.ctrl('f');
    h.press(KeyCode::Char('y'));
    assert!(find_payload(&h.sent()).is_some());
    assert_eq!(h.partner, PartnerState::Searching);
    assert_eq!(h.last_entry(), &EntryKind::System("You have disconnected from your previous partner.".into()));
    assert!(h.can_block_previous);
}

#[test]
fn confirmations_can_be_turned_off() {
    let mut h = harness().online().with_prefs().partnered();
    h.config.settings.confirm_actions = false;
    h.ctrl('d');
    assert_eq!(h.sent(), vec![ClientMessage::Disconnect]);
    h.server(ServerMessage::ClientDisconnect);
    assert_eq!(h.last_entry(), &EntryKind::System("You have disconnected from your partner.".into()));
}

#[test]
fn can_block_previous_partner_only_while_server_remembers_them() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::PartnerLeft);
    h.ctrl('b');
    h.press(KeyCode::Enter);
    assert_eq!(h.sent(), vec![ClientMessage::BlockPartner]);
    h.server(ServerMessage::PartnerBlocked);
    assert_eq!(h.last_entry(), &EntryKind::System("Your previous partner has been blocked.".into()));

    // A partner who dropped off the server entirely can't be blocked.
    let mut h = h.partnered();
    h.server(ServerMessage::PartnerDisconnected);
    h.ctrl('b');
    assert!(h.modal.is_none());
    assert_eq!(h.last_toast(), Some("There's no partner to block."));
}

#[test]
fn blocking_current_partner() {
    let mut h = harness().online().with_prefs().partnered();
    h.ctrl('b');
    h.press(KeyCode::Char('y'));
    assert_eq!(h.sent(), vec![ClientMessage::BlockPartner]);
    h.server(ServerMessage::PartnerBlocked);
    assert!(!h.has_partner());
    assert_eq!(h.last_entry(), &EntryKind::System("Your partner has been blocked and disconnected from you.".into()));
}

#[test]
fn leaving_the_queue_reconnects() {
    let mut h = harness().online().with_prefs();
    h.ctrl('f');
    h.take_effects();
    h.ctrl('d');
    let effects = h.take_effects();
    assert!(matches!(effects.as_slice(), [Effect::Connect(url)] if url == crate::config::DEFAULT_SERVER));
    assert_eq!(h.status, ConnStatus::Connecting);
    assert_eq!(h.partner, PartnerState::None);
}

#[test]
fn unexpected_close_schedules_reconnect_with_backoff() {
    let mut h = harness().online().with_prefs().partnered();
    h.on_net(NetEvent::Closed { reason: "connection reset".into() });
    assert!(!h.has_partner());
    let ConnStatus::Offline { retry_at: Some(at), .. } = h.status.clone() else { panic!("{:?}", h.status) };
    assert_eq!(at, h.now + Duration::from_secs(1));
    assert!(matches!(h.last_entry(), EntryKind::Warning(t) if t.contains("Reconnecting")));

    h.on_tick();
    assert!(h.take_effects().is_empty());
    h.now = at;
    h.on_tick();
    assert!(matches!(h.take_effects().as_slice(), [Effect::Connect(_)]));

    // A second failure backs off further.
    h.on_net(NetEvent::Closed { reason: "again".into() });
    let ConnStatus::Offline { retry_at: Some(at2), .. } = h.status.clone() else { panic!() };
    assert_eq!(at2, h.now + Duration::from_secs(2));
}

#[test]
fn deliberate_close_does_not_reconnect() {
    let mut h = harness().online();
    h.config.settings.auto_reconnect = false;
    h.on_net(NetEvent::Closed { reason: "bye".into() });
    assert!(matches!(h.status, ConnStatus::Offline { retry_at: None, .. }));
    h.now += Duration::from_secs(60);
    h.on_tick();
    assert!(h.take_effects().is_empty());
}

#[test]
fn unknown_frames_surface_in_chat() {
    let mut h = harness().online();
    h.server(ServerMessage::Unknown { kind: "shiny_new".into(), data: serde_json::Value::Null });
    assert!(matches!(h.last_entry(), EntryKind::Warning(t) if t.contains("shiny_new")));
    h.on_net(NetEvent::Unparsed { raw: "{broken".into(), error: "eof".into() });
    assert!(matches!(h.last_entry(), EntryKind::Warning(t) if t.contains("{broken")));
}

#[test]
fn invalid_preferences_from_server_stops_search() {
    let mut h = harness().online().with_prefs();
    h.ctrl('f');
    h.server(ServerMessage::InvalidPreferences);
    assert_eq!(h.partner, PartnerState::None);
    assert!(h.last_toast().unwrap().contains("invalid preferences"));
}

#[test]
fn stale_typing_without_partner_is_ignored() {
    let mut h = harness().online();
    h.server(ServerMessage::PartnerTyping(true));
    assert!(!h.chat.partner_typing);
}

#[test]
fn trusted_image_links_are_fetched_automatically() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage(
        "ref: https://static1.e621.net/data/ab/cd/x.png and https://evil.example/y.png and https://e621.net/posts/1"
            .into(),
    ));
    let fetches: Vec<_> = h.take_effects().into_iter().filter(|e| matches!(e, Effect::FetchImage { .. })).collect();
    assert_eq!(
        fetches,
        vec![Effect::FetchImage { url: "https://static1.e621.net/data/ab/cd/x.png".into(), allow_host: None }]
    );
    // Seen again: not refetched.
    h.server(ServerMessage::ReceiveMessage("again https://static1.e621.net/data/ab/cd/x.png".into()));
    assert!(!h.take_effects().iter().any(|e| matches!(e, Effect::FetchImage { .. })));

    h.config.settings.images.enabled = false;
    h.server(ServerMessage::ReceiveMessage("https://i.imgur.com/new.png".into()));
    assert!(!h.take_effects().iter().any(|e| matches!(e, Effect::FetchImage { .. })));
}

#[test]
fn previewing_untrusted_image_needs_consent() {
    let mut h = harness();
    h.preview("https://random.host/pic.png");
    assert!(matches!(h.modal, Some(Modal::Confirm { action: Confirm::LoadUntrusted(_), .. })));
    h.press(KeyCode::Char('y'));
    assert_eq!(
        h.take_effects(),
        vec![Effect::FetchImage { url: "https://random.host/pic.png".into(), allow_host: Some("random.host".into()) }]
    );
    assert!(h.viewer.is_some());
    h.on_image("https://random.host/pic.png".into(), Err("HTTP 404".into()));
    assert!(h.viewer.is_none());
    assert_eq!(h.last_toast(), Some("Couldn't load image: HTTP 404"));
}

#[test]
fn images_arriving_fill_the_viewer() {
    let mut h = harness();
    h.preview("https://i.imgur.com/a.png");
    assert!(h.viewer.as_ref().unwrap().protocol.is_none());
    let img = image::DynamicImage::new_rgba8(8, 8);
    let loaded = crate::images::prepare(&h.picker.clone(), img, ratatui::layout::Size::new(4, 4)).unwrap();
    h.on_image("https://i.imgur.com/a.png".into(), Ok(loaded));
    assert!(h.viewer.as_ref().unwrap().protocol.is_some());
    assert!(matches!(h.images.get("https://i.imgur.com/a.png"), Some(ImageState::Ready(_))));
    h.press(KeyCode::Esc);
    assert!(h.viewer.is_none());
}

#[test]
fn alerts_only_fire_while_unfocused() {
    let mut h = harness().online().with_prefs();
    h = h.partnered();
    assert!(!h.take_effects().contains(&Effect::Bell));
    assert_eq!(h.window_title(), "yap");

    h.on_terminal(Event::FocusLost);
    h.server(ServerMessage::PartnerLeft);
    assert!(h.take_effects().contains(&Effect::Bell));
    assert_eq!(h.window_title(), "Partner Left · yap");
    h.now += Duration::from_millis(700);
    assert_eq!(h.window_title(), "yap");
    h.on_terminal(Event::FocusGained);
    h.now += Duration::from_millis(700);
    assert_eq!(h.window_title(), "yap");
}

#[test]
fn desktop_notifications_are_opt_in() {
    let mut h = harness().online().with_prefs();
    h.set_focused(false);
    h.server(ServerMessage::PartnerDisconnected);
    assert!(!h.take_effects().iter().any(|e| matches!(e, Effect::Notify { .. })));

    h.set_focused(false);
    h.config.settings.notify.desktop = true;
    h.server(ServerMessage::PartnerPending);
    h.server(ServerMessage::PartnerDisconnected);
    assert!(h.take_effects().contains(&Effect::Notify { title: "yap".into(), body: "Partner Disconnected".into() }));
}

#[test]
fn slash_commands_run_instead_of_sending() {
    let mut h = harness().online().with_prefs().partnered();
    h.type_str("/save https://e621.net/posts/1 my ref");
    h.take_effects();
    h.press(KeyCode::Enter);
    assert!(!h.sent().iter().any(|m| matches!(m, ClientMessage::SendMessage(_))));
    assert_eq!(h.drawer.items[0].label, "my ref");
    h.flush();
    assert!(h.paths.drawer_file.exists());

    h.type_str("//me waves");
    h.press(KeyCode::Enter);
    assert!(h.sent().contains(&ClientMessage::SendMessage("/me waves".into())));

    h.type_str("/theme nord");
    h.press(KeyCode::Enter);
    assert_eq!(h.theme.name, "nord");
    assert_eq!(h.config.settings.theme, "nord");

    h.type_str("/nope");
    h.press(KeyCode::Enter);
    assert!(h.last_toast().unwrap().contains("unknown command"));
    assert_eq!(h.input.text(), "/nope", "bad commands stay editable");
}

#[test]
fn export_and_import_via_commands() {
    let mut h = harness();
    h.config.active_mut().preferences = complete_prefs();
    let path = h._dir.path().join("backup.json");
    h.run_command(Command::ExportAll(path.display().to_string()));
    assert!(h.last_toast().unwrap().starts_with("Exported"));
    h.run_command(Command::Import { path: path.display().to_string(), with_settings: false });
    assert_eq!(h.config.profiles.len(), 2);
    assert_eq!(h.config.profiles[1].name, "default (2)");
    assert_eq!(h.config.profiles[1].preferences, complete_prefs());

    h.run_command(Command::Import { path: "/definitely/missing.toml".into(), with_settings: false });
    assert!(h.last_toast().unwrap().starts_with("Import failed"));
}

#[test]
fn trust_and_untrust_domains() {
    let mut h = harness();
    h.run_command(Command::Trust("https://Cdn.Example.org/x".into()));
    assert!(h.config.settings.images.trusted_domains.contains(&"cdn.example.org".to_string()));
    h.run_command(Command::Untrust("cdn.example.org".into()));
    assert!(!h.config.settings.images.trusted_domains.contains(&"cdn.example.org".to_string()));
    h.run_command(Command::Trust("nope".into()));
    assert_eq!(h.last_toast(), Some("`nope` isn't a domain."));
}

#[test]
fn preference_editing_with_keys() {
    let mut h = harness();
    h.press(KeyCode::F(3));
    assert_eq!(h.tab, Tab::Preferences);
    // Field list starts at "Your gender".
    h.press(KeyCode::Enter);
    assert_eq!(h.prefs_ui.pane, PrefsPane::Options);
    h.type_str("fem");
    h.press(KeyCode::Enter);
    assert_eq!(h.config.active().preferences.gender.as_deref(), Some("Female"));
    // Single choice returns to the field list and advances to species.
    assert_eq!(h.prefs_ui.pane, PrefsPane::Fields);
    assert_eq!(h.prefs_ui.field, 1);
    h.press(KeyCode::Enter);
    h.type_str("snow leo");
    h.press(KeyCode::Enter);
    assert_eq!(h.config.active().preferences.species.as_deref(), Some("Snow Leopard"));

    // Kinks: multi-select stays open; space toggles when not filtering.
    h.prefs_ui.field = Field::ALL.iter().position(|f| *f == Field::Kinks).unwrap();
    h.press(KeyCode::Enter);
    h.press(KeyCode::Down);
    h.press(KeyCode::Char(' '));
    h.press(KeyCode::Down);
    h.press(KeyCode::Char(' '));
    assert_eq!(h.config.active().preferences.kinks, vec!["3+ Penetration", "Age Differences"]);
    assert_eq!(h.prefs_ui.pane, PrefsPane::Options);
    h.press(KeyCode::Esc);
    h.press(KeyCode::Char('x'));
    assert_eq!(h.config.active().preferences.kinks, vec!["any"]);
    h.flush();
    let (saved, _) = Config::load(&h.paths.config_file).unwrap();
    assert_eq!(saved.active().preferences.species.as_deref(), Some("Snow Leopard"));
}

#[test]
fn profile_management_with_keys() {
    let mut h = harness();
    h.press(KeyCode::F(3));
    h.prefs_ui.pane = PrefsPane::Profiles;
    h.press(KeyCode::Char('n'));
    h.type_str("switchy");
    h.press(KeyCode::Enter);
    assert_eq!(h.config.active_profile, "switchy");

    h.prefs_ui.pane = PrefsPane::Profiles;
    h.prefs_ui.profile = 1;
    h.press(KeyCode::Char('r'));
    h.ctrl('u');
    h.type_str("vixen");
    h.press(KeyCode::Enter);
    assert_eq!(h.config.active_profile, "vixen");

    h.press(KeyCode::Char('d'));
    h.press(KeyCode::Char('y'));
    assert_eq!(h.config.profiles.len(), 1);
    assert_eq!(h.config.active_profile, "default");
}

#[test]
fn theme_picker_previews_and_cancels() {
    let mut h = harness();
    h.ctrl('t');
    h.press(KeyCode::Down);
    let previewed = h.theme.name.clone();
    assert_ne!(previewed, "dark");
    assert_eq!(h.config.settings.theme, "dark", "preview isn't saved");
    h.press(KeyCode::Esc);
    assert_eq!(h.theme.name, "dark");

    h.ctrl('t');
    h.press(KeyCode::Down);
    h.press(KeyCode::Enter);
    assert_eq!(h.config.settings.theme, h.theme.name);
    assert_ne!(h.theme.name, "dark");
}

#[test]
fn drawer_add_flow_and_sharing() {
    let mut h = harness().online().with_prefs().partnered();
    h.press(KeyCode::F(4));
    h.press(KeyCode::Char('a'));
    h.ctrl('u');
    h.type_str("https://i.imgur.com/ref.png");
    h.press(KeyCode::Enter);
    let Some(Modal::Prompt(p)) = &h.modal else { panic!("expected label prompt") };
    assert_eq!(p.editor.text(), "i.imgur.com · ref.png");
    h.ctrl('u');
    h.type_str("Ref sheet");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.items[0].label, "Ref sheet");

    h.press(KeyCode::Char('i'));
    assert_eq!(h.tab, Tab::Chat);
    assert_eq!(h.input.text(), "https://i.imgur.com/ref.png");
    assert!(h.sent().contains(&ClientMessage::Typing(true)));
}

#[test]
fn chat_side_drawer_inserts_links() {
    let mut h = harness();
    h.drawer.add("https://e621.net/posts/1", "ref", chrono::Utc::now()).unwrap();
    h.type_str("look:");
    h.ctrl('e');
    assert_eq!(h.chat_focus, ChatFocus::Drawer);
    h.press(KeyCode::Enter);
    assert_eq!(h.input.text(), "look: https://e621.net/posts/1");
    assert_eq!(h.chat_focus, ChatFocus::Input);
}

#[test]
fn link_picker_saves_and_trusts() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("see https://cdn.furry.art/x.png".into()));
    h.ctrl('o');
    assert!(matches!(h.modal, Some(Modal::Links { .. })));
    h.press(KeyCode::Char('t'));
    assert!(h.config.settings.images.trusted_domains.contains(&"cdn.furry.art".to_string()));
    h.ctrl('o');
    h.press(KeyCode::Char('s'));
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.items[0].url, "https://cdn.furry.art/x.png");
}

#[test]
fn quitting_mid_chat_asks() {
    let mut h = harness().online().with_prefs().partnered();
    h.ctrl('c');
    assert!(!h.quit);
    h.press(KeyCode::Char('y'));
    assert!(h.quit);
    let mut h = harness();
    h.ctrl('q');
    assert!(h.quit);
}

#[test]
fn traffic_is_recorded_and_navigable() {
    let mut h = harness();
    for (i, body) in
        [r#"{"type":"ping","data":true}"#, r#"{"type":"update_user_count","data":1}"#, "x"].iter().enumerate()
    {
        h.on_net(NetEvent::Traffic(crate::net::TrafficRecord {
            at: Local::now(),
            dir: if i == 0 { Direction::Out } else { Direction::In },
            kind: FrameKind::Text,
            size: body.len(),
            body: (*body).into(),
        }));
    }
    h.press(KeyCode::F(5));
    assert_eq!(h.traffic_len(), 2, "heartbeat hidden by default");
    assert_eq!(h.traffic_selected(), Some(1));
    h.press(KeyCode::Up);
    assert!(!h.traffic_ui.follow);
    assert_eq!(h.traffic_selected(), Some(0));
    h.press(KeyCode::Char('h'));
    assert_eq!(h.traffic_len(), 3);
    h.press(KeyCode::Char('/'));
    h.type_str("count");
    h.press(KeyCode::Enter);
    assert_eq!(h.traffic_len(), 1);
    let path = h._dir.path().join("t.jsonl");
    h.export_traffic(&path.display().to_string());
    assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 3);
}

#[test]
fn raw_frames_need_a_connection() {
    let mut h = harness();
    h.run_command(Command::Raw("{}".into()));
    assert!(h.take_effects().is_empty());
    let mut h = harness().online();
    h.run_command(Command::Raw(r#"{"type":"typing","data":true}"#.into()));
    assert_eq!(h.take_effects(), vec![Effect::SendRaw(r#"{"type":"typing","data":true}"#.into())]);
}

#[test]
fn settings_rows_toggle_and_persist() {
    let mut h = harness();
    h.press(KeyCode::F(6));
    let rows = settings::rows(&h.config.settings);
    h.settings_ui.selected = rows.iter().position(|r| *r == settings::Row::Timestamps).unwrap();
    h.press(KeyCode::Enter);
    assert!(!h.config.settings.timestamps);
    h.settings_ui.selected = rows.iter().position(|r| *r == settings::Row::MaxRows).unwrap();
    h.press(KeyCode::Right);
    assert_eq!(h.config.settings.images.max_rows, 13);
    h.settings_ui.selected = rows.iter().position(|r| matches!(r, settings::Row::Domain(0))).unwrap();
    let first = h.config.settings.images.trusted_domains[0].clone();
    h.press(KeyCode::Char('d'));
    assert!(!h.config.settings.images.trusted_domains.contains(&first));
    h.flush();
    let (saved, _) = Config::load(&h.paths.config_file).unwrap();
    assert_eq!(saved.settings, h.config.settings);
}

#[test]
fn server_url_prompt_validates_and_reconnects() {
    let mut h = harness().online();
    h.open_prompt("Server URL", "", modal::PromptAction::ServerUrl);
    h.type_str("https://localhost:8000");
    h.press(KeyCode::Enter);
    assert_eq!(h.config.settings.server_url, "wss://localhost:8000/");
    assert_eq!(h.take_effects(), vec![Effect::Connect("wss://localhost:8000/".into())]);

    h.open_prompt("Server URL", "", modal::PromptAction::ServerUrl);
    h.type_str("gopher://x");
    h.press(KeyCode::Enter);
    assert!(h.last_toast().unwrap().contains("unsupported scheme"));
}
