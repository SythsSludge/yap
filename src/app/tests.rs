use super::modal::{Confirm, Modal};
use super::*;
use super::{EditorPurpose, Hit, ListId, PaletteItem, Shelf, ViewerButton};
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
    assert_eq!(wire.kinks, vec!["any", "Biting", "Musk"], "any/all combines with picks");
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
    let loaded =
        crate::images::prepare(&h.picker.clone(), img, vec![1, 2, 3], ratatui::layout::Size::new(4, 4)).unwrap();
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
    assert_eq!(h.config.active().preferences.kinks, vec!["any", "3+ Penetration", "Age Differences"]);
    // Any / All is just another option in the list and can be switched off.
    h.press(KeyCode::Home);
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
    h.press(KeyCode::F(6));
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
    h.press(KeyCode::F(7));
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

#[test]
fn each_partner_gets_their_own_log() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("first chat".into()));
    h.server(ServerMessage::PartnerLeft);
    h.server(ServerMessage::PartnerPending);
    let mut h = h.partnered();
    h.server(ServerMessage::ReceiveMessage("second chat".into()));

    assert_eq!(h.logs.items.len(), 2);
    let first = h.logs.items[0].loaded_entries().unwrap();
    assert!(first.iter().any(|e| e.kind == EntryKind::Partner("first chat".into())));
    assert!(first.iter().any(|e| e.kind == EntryKind::System("Your yiffing partner has left.".into())));
    assert!(
        !first.iter().any(|e| matches!(&e.kind, EntryKind::System(t) if t.starts_with("We are looking"))),
        "search noise isn't logged"
    );
    assert!(!h.logs.items[0].is_live());
    assert!(h.logs.items[1].is_live());
    // The continuous view still shows everything.
    assert!(h.chat.entries.iter().any(|e| e.kind == EntryKind::Partner("first chat".into())));
}

#[test]
fn split_view_starts_fresh_for_each_partner() {
    let mut h = harness().online().with_prefs();
    h.config.settings.split_chats = true;
    let mut h = h.partnered();
    h.server(ServerMessage::ReceiveMessage("old".into()));
    h.server(ServerMessage::PartnerLeft);
    let h = h.partnered();
    assert!(!h.chat.entries.iter().any(|e| e.kind == EntryKind::Partner("old".into())));
    assert!(matches!(h.chat.entries[0].kind, EntryKind::System(ref t) if t.starts_with("You have been connected")));
    assert_eq!(h.logs.items.len(), 2, "the old chat is still in the logs");
}

#[test]
fn logs_only_hit_disk_when_enabled() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("secret".into()));
    h.flush();
    assert!(!h.paths.logs_dir.exists(), "logging is off by default");

    h.press(KeyCode::F(7));
    let rows = settings::rows(&h.config.settings);
    h.settings_ui.selected = rows.iter().position(|r| *r == settings::Row::SaveLogs).unwrap();
    h.press(KeyCode::Enter);
    h.flush();
    let files: Vec<_> = std::fs::read_dir(&h.paths.logs_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    let (mut loaded, _) = crate::logs::Logs::load_dir(&h.paths.logs_dir);
    assert!(loaded.open(0).unwrap().iter().any(|e| e.kind == EntryKind::Partner("secret".into())));
}

#[test]
fn logs_tab_opens_exports_and_deletes() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("hello".into()));
    h.server(ServerMessage::PartnerLeft);
    h.press(KeyCode::F(5));
    assert_eq!(h.tab, Tab::Logs);
    h.press(KeyCode::Enter);
    assert!(h.logs_ui.reading);
    h.press(KeyCode::Esc);
    let path = h._dir.path().join("chat.txt");
    h.press(KeyCode::Char('e'));
    h.ctrl('u');
    h.type_str(&path.display().to_string());
    h.press(KeyCode::Enter);
    assert!(std::fs::read_to_string(&path).unwrap().contains("Partner: hello"));
    h.press(KeyCode::Char('d'));
    h.press(KeyCode::Char('y'));
    assert!(h.logs.items.is_empty());
}

#[test]
fn drawer_tags_edit_and_filter() {
    let mut h = harness();
    h.run_command(Command::Save { url: "https://a.com/".into(), label: "alpha #ref".into() });
    h.run_command(Command::Save { url: "https://b.com/".into(), label: "beta".into() });
    h.press(KeyCode::F(4));
    h.press(KeyCode::Char('t'));
    h.type_str("outfits, ref");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.items[0].tags, vec!["outfits", "ref"]);

    h.press(KeyCode::Char(']'));
    assert_eq!(h.drawer_ui.tag.as_deref(), Some("outfits"));
    assert_eq!(h.drawer_items(), vec![0]);
    h.press(KeyCode::Char(']'));
    assert_eq!(h.drawer_ui.tag.as_deref(), Some("ref"));
    assert_eq!(h.drawer_items().len(), 2);
    h.press(KeyCode::Char(']'));
    assert_eq!(h.drawer_ui.tag, None, "wraps back to all");
    h.press(KeyCode::Char('['));
    assert_eq!(h.drawer_ui.tag.as_deref(), Some("ref"));
    h.press(KeyCode::Esc);
    assert_eq!(h.drawer_ui.tag, None);
}

#[test]
fn transparent_background_survives_theme_changes() {
    let mut h = harness();
    h.press(KeyCode::F(7));
    let rows = settings::rows(&h.config.settings);
    h.settings_ui.selected = rows.iter().position(|r| *r == settings::Row::Transparent).unwrap();
    h.press(KeyCode::Enter);
    assert_eq!(h.theme.bg, ratatui::style::Color::Reset);
    h.set_theme("nord", true);
    assert_eq!(h.theme.bg, ratatui::style::Color::Reset);
    assert_ne!(h.theme.surface, ratatui::style::Color::Reset, "popups keep their own background");
    h.press(KeyCode::Enter);
    assert_ne!(h.theme.bg, ratatui::style::Color::Reset);
}

fn select_row(h: &mut Harness, row: settings::Row) {
    h.press(KeyCode::F(7));
    let rows = settings::rows(&h.config.settings);
    h.settings_ui.selected = rows.iter().position(|r| *r == row).unwrap();
}

fn alt(h: &mut Harness, c: char) {
    h.on_terminal(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)));
}

#[test]
fn rebinding_a_key_in_settings() {
    use crate::keymap::Action;
    let mut h = harness().online().with_prefs();
    select_row(&mut h, settings::Row::Key(Action::Find));
    h.press(KeyCode::Enter);
    assert!(matches!(h.modal, Some(Modal::CaptureKey { action: Action::Find })));
    // Plain letters are for typing; the popup stays open and says why.
    h.press(KeyCode::Char('g'));
    assert!(matches!(h.modal, Some(Modal::CaptureKey { .. })));
    assert!(h.last_toast().unwrap().contains("needed for typing"));
    alt(&mut h, 'f');
    assert!(h.modal.is_none());
    assert_eq!(h.last_toast(), Some("Find a partner: alt+f"));

    // The old key does nothing now; the new one searches.
    h.press(KeyCode::F(2));
    h.ctrl('f');
    assert!(find_payload(&h.sent()).is_none());
    alt(&mut h, 'f');
    assert!(find_payload(&h.sent()).is_some());

    // Saved as an override only, and reloaded on restart.
    assert_eq!(h.config.settings.keys.len(), 1);
    h.flush();
    let (saved, warnings) = Config::load(&h.paths.config_file).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(crate::keymap::Keymap::build(&saved.settings.keys).0, h.keymap);
}

#[test]
fn rebinding_steals_reset_and_unbind() {
    use crate::keymap::Action;
    let mut h = harness().online().with_prefs().partnered();
    select_row(&mut h, settings::Row::Key(Action::Leave));
    h.press(KeyCode::Enter);
    h.ctrl('b');
    assert_eq!(h.last_toast(), Some("Leave partner / stop searching: ctrl+b (taken from block partner)"));
    assert!(h.keymap.keys(Action::Block).is_empty());

    // Backspace restores the default (ctrl+d); x unbinds.
    h.press(KeyCode::Backspace);
    assert_eq!(h.keymap.hint(Action::Leave).as_deref(), Some("^D"));
    h.press(KeyCode::Char('x'));
    assert!(h.keymap.keys(Action::Leave).is_empty());
    h.press(KeyCode::F(2));
    h.ctrl('d');
    assert!(h.modal.is_none(), "unbound: ctrl+d does nothing");

    select_row(&mut h, settings::Row::ResetKeys);
    h.press(KeyCode::Enter);
    assert_eq!(h.keymap, crate::keymap::Keymap::default());
    assert!(h.config.settings.keys.is_empty());
}

#[test]
fn chat_scroll_keys_are_rebindable_but_only_act_in_chat() {
    use crate::keymap::{Action, Chord};
    let mut h = harness();
    h.keymap.bind(Action::ScrollPageUp, Chord::parse("alt+k").unwrap());
    h.chat.last_total = 100;
    h.chat.last_height = 20;
    alt(&mut h, 'k');
    assert!(!h.chat.is_following());
    h.press(KeyCode::Esc);
    h.press(KeyCode::PageUp);
    assert!(h.chat.is_following(), "pgup was moved to alt+k");
    // In other tabs the same key moves lists instead of the hidden chat.
    h.press(KeyCode::F(7));
    let before = h.settings_ui.selected;
    alt(&mut h, 'k');
    assert!(h.chat.is_following());
    assert_eq!(h.settings_ui.selected, before);
}

#[test]
fn bad_key_config_warns_instead_of_failing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[settings.keys]\nfnid = \"alt+f\"\nquit = \"q\"\nfind = \"alt+f\"\n").unwrap();
    let (config, warnings) = Config::load(&path).unwrap();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    let map = crate::keymap::Keymap::build(&config.settings.keys).0;
    assert_eq!(map.hint(crate::keymap::Action::Find).as_deref(), Some("alt+f"));
}

#[test]
fn hints_follow_bindings() {
    use crate::keymap::{Action, Chord};
    let mut h = harness();
    h.keymap.bind(Action::Find, Chord::parse("alt+f").unwrap());
    h.keymap.unbind(Action::Block);
    let hints = crate::ui::hint_pairs(&h);
    assert!(hints.contains(&("alt+f".to_owned(), "find")));
    assert!(!hints.iter().any(|(_, what)| *what == "block"));
}

// ----- sessions ---------------------------------------------------------------------

#[test]
fn second_chat_has_its_own_socket_and_state() {
    let mut h = harness().online().with_prefs().partnered();
    h.type_str("draft for chat one");
    h.take_effects();
    h.run_command(Command::NewChat);
    assert_eq!(h.session_count(), 2);
    let effects = h.take_tagged_effects();
    assert!(matches!(effects.as_slice(), [(1, Effect::Connect(_))]), "{effects:?}");
    assert!(h.input.is_empty(), "each chat keeps its own draft");
    assert!(!h.has_partner());

    // Chat 1 comes online and searches; its frames are tagged with its id.
    h.on_net_for(1, NetEvent::Open);
    h.ctrl('f');
    assert!(
        h.take_tagged_effects()
            .iter()
            .any(|(id, e)| *id == 1 && matches!(e, Effect::Send(ClientMessage::FindPartner(_))))
    );

    // Meanwhile chat 0's partner writes: it lands in chat 0, counted as unseen.
    h.on_net_for(0, NetEvent::Message(ServerMessage::ReceiveMessage("you there?".into())));
    assert!(!h.chat.entries.iter().any(|e| e.kind == EntryKind::Partner("you there?".into())));
    let chat0 = h.sessions().into_iter().find(|s| s.id == 0).unwrap();
    assert_eq!(chat0.unseen, 1);
    assert_eq!(chat0.label, "female fox");

    h.switch_session(0);
    assert_eq!(h.input.text(), "draft for chat one");
    assert_eq!(h.last_entry(), &EntryKind::Partner("you there?".into()));
    h.mark_seen();
    assert_eq!(h.unseen, 0);
}

#[test]
fn background_alerts_become_toasts_and_quit_checks_every_chat() {
    let mut h = harness().online().with_prefs().partnered();
    h.new_session();
    h.take_effects();
    h.on_net_for(0, NetEvent::Message(ServerMessage::PartnerLeft));
    assert_eq!(h.last_toast(), Some("Chat 1: Partner Left"));
    assert_eq!(h.tab, Tab::Chat);

    let mut h = harness().online().with_prefs().partnered();
    h.new_session();
    h.ctrl('q');
    assert!(matches!(h.modal, Some(Modal::Confirm { action: Confirm::Quit, .. })), "chat 1 still has a partner");
}

#[test]
fn closing_a_chat_closes_its_socket() {
    let mut h = harness().online().with_prefs();
    h.new_session();
    h.take_effects();
    assert_eq!(h.session_number(), 2);
    h.run_command(Command::CloseChat);
    assert_eq!(h.take_tagged_effects(), vec![(1, Effect::CloseSocket)]);
    assert_eq!(h.session_count(), 1);
    assert_eq!(h.session_id, 0);
    h.run_command(Command::CloseChat);
    assert_eq!(h.session_count(), 1, "the last chat stays");
    // Events for the closed chat are ignored.
    h.on_net_for(1, NetEvent::Message(ServerMessage::ReceiveMessage("ghost".into())));
    assert!(!h.chat.entries.iter().any(|e| e.kind == EntryKind::Partner("ghost".into())));
}

#[test]
fn cycling_chats_wraps() {
    let mut h = harness();
    h.new_session();
    h.new_session();
    assert_eq!(h.session_number(), 3);
    h.cycle_session(1);
    assert_eq!(h.session_number(), 1);
    h.cycle_session(-1);
    assert_eq!(h.session_number(), 3);
    h.run_command(Command::SwitchChat(2));
    assert_eq!(h.session_number(), 2);
}

// ----- next, requeue, skip ----------------------------------------------------------

#[test]
fn next_skips_without_asking() {
    let mut h = harness().online().with_prefs().partnered();
    h.ctrl('n');
    assert!(h.modal.is_none());
    assert!(find_payload(&h.sent()).is_some());
    assert_eq!(h.partner, PartnerState::Searching);
}

#[test]
fn auto_requeue_after_partner_leaves() {
    let mut h = harness().online().with_prefs();
    h.config.settings.auto_requeue = true;
    let mut h = h.partnered();
    h.server(ServerMessage::PartnerLeft);
    assert!(matches!(h.last_entry(), EntryKind::System(t) if t.starts_with("Searching again in 3s")));
    h.on_tick();
    assert!(h.sent().is_empty());
    h.now += Duration::from_secs(3);
    h.on_tick();
    assert!(find_payload(&h.sent()).is_some());

    // Esc in the chat cancels a pending requeue.
    let mut h = h.partnered();
    h.server(ServerMessage::PartnerDisconnected);
    h.press(KeyCode::Esc);
    h.now += Duration::from_secs(10);
    h.on_tick();
    assert!(h.sent().is_empty());
    // Leaving on purpose doesn't requeue.
    let mut h = h.partnered();
    h.server(ServerMessage::ClientDisconnect);
    h.now += Duration::from_secs(10);
    h.on_tick();
    assert!(h.sent().is_empty());
}

fn connected_to(h: &mut Harness, kinks: &str, language: Option<&str>) {
    h.server(ServerMessage::PartnerConnected(PartnerInfo {
        gender: "Male".into(),
        species: "Cat".into(),
        kinks: kinks.into(),
        role: "Dominant".into(),
        language: language.map(Into::into),
    }));
}

#[test]
fn limits_skip_partners_before_the_chat_starts() {
    let mut h = harness().online().with_prefs();
    h.config.active_mut().preferences.toggle(Field::Limits, "Scat");
    h.ctrl('f');
    h.take_effects();
    connected_to(&mut h, "Biting, Scat", None);
    assert!(find_payload(&h.sent()).is_some(), "searched again");
    assert!(!h.has_partner());
    assert!(
        matches!(h.last_entry(), EntryKind::System(t) if t == "Skipped a Dominant Male Cat: they're into Scat, one of your limits.")
    );
    assert!(h.logs.items.is_empty(), "skipped partners don't get a log");

    connected_to(&mut h, "Biting", None);
    assert!(h.has_partner());
    assert!(h.sent().is_empty());
}

#[test]
fn shared_kink_and_language_rules() {
    let mut h = harness().online().with_prefs();
    h.config.settings.skip.min_shared_kinks = 2;
    h.config.settings.skip.language_mismatch = true;
    h.config.active_mut().preferences.kinks = vec!["Biting".into(), "Musk".into()];
    h.config.active_mut().preferences.language = "English".into();
    let reason = |h: &Harness, kinks: &str, lang: Option<&str>| {
        h.skip_reason(&PartnerInfo {
            gender: "M".into(),
            species: "S".into(),
            kinks: kinks.into(),
            role: "R".into(),
            language: lang.map(Into::into),
        })
    };
    assert_eq!(reason(&h, "Biting, Tickling", None).as_deref(), Some("only 1 shared kink"));
    assert_eq!(reason(&h, "Biting, Musk", None), None);
    assert_eq!(reason(&h, "any", None), None, "partners open to anything pass");
    assert_eq!(reason(&h, "Biting, Musk", Some("French")).as_deref(), Some("they chose French"));
    assert_eq!(reason(&h, "Biting, Musk", Some("any")), None);
    h.config.settings.skip.enabled = false;
    assert_eq!(reason(&h, "Tickling", Some("French")), None);
}

#[test]
fn skipping_stops_after_too_many_in_a_row() {
    let mut h = harness().online().with_prefs();
    h.config.settings.skip.max_in_a_row = 2;
    h.config.active_mut().preferences.toggle(Field::Limits, "Scat");
    h.ctrl('f');
    for _ in 0..2 {
        connected_to(&mut h, "Scat", None);
        assert!(!h.has_partner());
    }
    connected_to(&mut h, "Scat", None);
    assert!(h.has_partner(), "the third one stays so we don't spin forever");
    assert!(
        h.chat
            .entries
            .iter()
            .any(|e| matches!(&e.kind, EntryKind::System(t) if t.starts_with("Auto-skipped 2 partners")))
    );
}

// ----- snippets, editor, drawer export ----------------------------------------------

#[test]
fn snippets_insert_with_placeholders() {
    let mut h = harness().online().with_prefs().partnered();
    h.type_str("/snip-add intro Hi {partner_species}! I'm a {species}.");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.snippets[0].name, "intro");
    h.take_effects();
    h.type_str("/snip intro");
    h.press(KeyCode::Enter);
    assert_eq!(h.input.text(), "Hi Fox! I'm a Wolf.");
    assert!(h.sent().contains(&ClientMessage::Typing(true)));

    h.input.clear();
    h.ctrl('g');
    h.type_str("int");
    h.press(KeyCode::Enter);
    assert_eq!(h.input.text(), "Hi Fox! I'm a Wolf.");

    h.run_command(Command::Snip(Some("nope".into())));
    assert!(h.last_toast().unwrap().contains("No snippet called `nope`"));
}

#[test]
fn snippet_shelf_add_and_delete() {
    let mut h = harness();
    h.press(KeyCode::F(4));
    h.press(KeyCode::Char('s'));
    assert_eq!(h.drawer_ui.shelf, Shelf::Snippets);
    h.press(KeyCode::Char('a'));
    h.type_str("bye");
    h.press(KeyCode::Enter);
    h.type_str("Thanks, take care!");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.snippets[0].text, "Thanks, take care!");
    // An empty text opens the editor instead.
    h.press(KeyCode::Char('a'));
    h.type_str("long");
    h.press(KeyCode::Enter);
    h.press(KeyCode::Enter);
    assert!(
        h.take_effects().iter().any(|e| matches!(e, Effect::OpenEditor { purpose: EditorPurpose::Snippet(_), .. }))
    );
    h.on_editor(EditorPurpose::Snippet(1), Ok("Line one\nline two\n".into()));
    assert_eq!(h.drawer.snippets[1].text, "Line one\nline two");
    h.drawer_ui.snippets.selected = 0;
    h.press(KeyCode::Char('d'));
    h.press(KeyCode::Char('y'));
    assert_eq!(h.drawer.snippets.len(), 1);
}

#[test]
fn editor_round_trip_joins_paragraphs() {
    let mut h = harness().online().with_prefs().partnered();
    h.config.settings.editor = "my-editor --wait".into();
    h.input.set("first / second");
    h.ctrl('x');
    let effects = h.take_effects();
    let Some(Effect::OpenEditor { command, text, purpose }) = effects.first() else { panic!("{effects:?}") };
    assert_eq!(command, "my-editor --wait");
    assert_eq!(text, "first\n\nsecond\n");
    assert_eq!(*purpose, EditorPurpose::Message);

    h.on_editor(EditorPurpose::Message, Ok("A longer\npost.\n\nNew paragraph.\n".into()));
    assert_eq!(h.input.text(), "A longer post. / New paragraph.");
    h.on_editor(EditorPurpose::Message, Err("editor crashed".into()));
    assert_eq!(h.last_toast(), Some("Editor: editor crashed"));
}

#[test]
fn drawer_export_and_import() {
    let mut h = harness();
    h.run_command(Command::Save { url: "https://a.com/".into(), label: "a #ref".into() });
    h.run_command(Command::SnipAdd { name: "hi".into(), text: "hello".into() });
    let path = h._dir.path().join("drawer.json");
    h.run_command(Command::DrawerExport(path.display().to_string()));
    let mut other = harness();
    other.run_command(Command::DrawerImport(path.display().to_string()));
    assert_eq!(other.last_toast(), Some("Imported 1 links and 1 snippets"));
    assert_eq!(other.drawer.items[0].tags, vec!["ref"]);
}

// ----- logs, notifications ----------------------------------------------------------

#[test]
fn logs_pin_rename_and_search() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("let's meet at the lighthouse".into()));
    h.server(ServerMessage::PartnerLeft);
    let mut h = h.partnered();
    h.server(ServerMessage::PartnerLeft);
    h.press(KeyCode::F(5));
    // Newest first: select the older chat and pin it; it moves to the top.
    h.logs_ui.list.selected = 1;
    h.press(KeyCode::Char('p'));
    assert_eq!(h.visible_logs()[0], 0);
    assert_eq!(h.logs_ui.list.selected, 0, "selection follows the pinned chat");
    h.press(KeyCode::Char('r'));
    h.type_str("beach one");
    h.press(KeyCode::Enter);
    assert_eq!(h.logs.items[0].title(), "beach one");
    h.press(KeyCode::Char('/'));
    h.type_str("lighthouse");
    h.press(KeyCode::Enter);
    assert_eq!(h.visible_logs(), vec![0]);
}

#[test]
fn keywords_notify_even_when_messages_dont() {
    let mut h = harness().online().with_prefs().partnered();
    h.config.settings.notify.keywords = vec!["Ember".into()];
    h.set_focused(false);
    h.server(ServerMessage::ReceiveMessage("so how are you".into()));
    assert!(!h.take_effects().contains(&Effect::Bell));
    h.server(ServerMessage::ReceiveMessage("*nuzzles ember*".into()));
    assert!(h.take_effects().contains(&Effect::Bell));
    assert_eq!(h.window_title(), "Mentioned you · yap");
}

#[test]
fn sound_uses_the_configured_command() {
    let mut h = harness().online().with_prefs();
    h.config.settings.notify.sound = true;
    h.config.settings.notify.sound_command = "play ding.wav".into();
    h.set_focused(false);
    let mut h = h.partnered();
    h.server(ServerMessage::PartnerLeft);
    assert!(h.take_effects().contains(&Effect::PlaySound("play ding.wav".into())));
}

// ----- mouse ------------------------------------------------------------------------

fn frame(h: &mut Harness) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 30)).unwrap();
    terminal.draw(|f| crate::ui::draw(f, &mut h.app)).unwrap();
}

fn click_on(h: &mut Harness, target: &Hit) {
    let (rect, _) = h.hits.borrow().iter().find(|(_, hit)| hit == target).cloned().unwrap_or_else(|| {
        panic!(
            "no hitbox for {target:?}; have {:?}",
            h.hits.borrow().iter().map(|(_, h)| h.clone()).collect::<Vec<_>>()
        )
    });
    h.on_terminal(Event::Mouse(ratatui::crossterm::event::MouseEvent {
        kind: ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    }));
    frame(h);
}

#[test]
fn clicking_tabs_rows_and_double_clicks() {
    let mut h = harness();
    frame(&mut h);
    click_on(&mut h, &Hit::Tab(Tab::Settings));
    assert_eq!(h.tab, Tab::Settings);
    let row = settings::rows(&h.config.settings).iter().position(|r| *r == settings::Row::Timestamps).unwrap();
    let target = Hit::Row { list: ListId::Settings, index: row };
    click_on(&mut h, &target);
    assert_eq!(h.settings_ui.selected, row);
    assert!(h.config.settings.timestamps, "a single click only selects");
    click_on(&mut h, &target);
    assert!(!h.config.settings.timestamps, "a double click toggles");

    // Option checkboxes toggle on every click.
    click_on(&mut h, &Hit::Tab(Tab::Preferences));
    h.prefs_ui.field = Field::ALL.iter().position(|f| *f == Field::Kinks).unwrap();
    frame(&mut h);
    click_on(&mut h, &Hit::Row { list: ListId::PrefsOptions, index: 1 });
    assert_eq!(h.config.active().preferences.kinks, vec!["any", "3+ Penetration"]);
}

#[test]
fn clicking_links_images_and_viewer_buttons() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("gallery: https://example.com/g".into()));
    h.take_effects();
    frame(&mut h);
    let entry = h.chat.entries.len() - 1;
    click_on(&mut h, &Hit::Message { entry, links: vec!["https://example.com/g".into()] });
    assert_eq!(h.take_effects(), vec![Effect::OpenUrl("https://example.com/g".into())]);

    // A loaded inline image opens the viewer; its buttons work.
    let url = "https://i.imgur.com/pic.png".to_string();
    let png = {
        let img: image::RgbaImage = image::ImageBuffer::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let loaded = crate::images::prepare(
        &h.picker.clone(),
        crate::images::decode(&png).unwrap(),
        png.clone(),
        ratatui::layout::Size::new(4, 2),
    )
    .unwrap();
    h.server(ServerMessage::ReceiveMessage(url.clone()));
    h.on_image(url.clone(), Ok(loaded));
    frame(&mut h);
    click_on(&mut h, &Hit::Image(url.clone()));
    assert!(h.viewer.is_some());
    click_on(&mut h, &Hit::Viewer(ViewerButton::SaveImage));
    assert!(h.last_toast().unwrap().starts_with("Saved"));
    assert_eq!(std::fs::read(h.paths.downloads_dir.join("pic.png")).unwrap(), png);
    click_on(&mut h, &Hit::Viewer(ViewerButton::Close));
    assert!(h.viewer.is_none());
}

#[test]
fn clicking_sessions_and_new_chat() {
    let mut h = harness();
    h.new_session();
    frame(&mut h);
    click_on(&mut h, &Hit::Session(0));
    assert_eq!(h.session_id, 0);
    click_on(&mut h, &Hit::NewSession);
    assert_eq!(h.session_count(), 3);
}

// ----- roleplay names, select, search, palette, stats -------------------------------

#[test]
fn character_names_and_nicknames() {
    let mut h = harness().online().with_prefs();
    h.run_command(Command::Name(Some("Ember".into())));
    assert_eq!(h.config.active().character, "Ember");
    assert_eq!(h.my_label(), "Ember");
    h.run_command(Command::Nick(Some("Rook".into())));
    assert_eq!(h.last_toast(), Some("Nicknames are for the partner you're chatting with."));

    let mut h = h.partnered();
    h.run_command(Command::Nick(Some("Rook".into())));
    assert_eq!(h.partner_label(), "Rook");
    assert_eq!(h.sessions()[0].label, "Rook");
    assert_eq!(h.logs.live(0).unwrap().names(), ("Ember".into(), "Rook".into()));
    h.run_command(Command::SnipAdd { name: "hi".into(), text: "{me} waves at {partner}".into() });
    h.run_command(Command::Snip(Some("hi".into())));
    assert_eq!(h.input.text(), "Ember waves at Rook");

    // A new partner starts without the old nickname.
    let h = h.partnered();
    assert_eq!(h.partner_label(), "partner");
}

#[test]
fn select_mode_quotes_copies_and_saves() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("first".into()));
    h.server(ServerMessage::ReceiveMessage("look https://e621.net/posts/9 *grins*".into()));
    h.type_str("draft");
    h.on_terminal(Event::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)));
    let last = h.chat.entries.len() - 1;
    assert_eq!(h.chat.mode, chat::ChatMode::Select(last));
    h.press(KeyCode::Up);
    assert!(
        matches!(h.chat.mode, chat::ChatMode::Select(i) if h.chat.entries[i].kind == EntryKind::Partner("first".into()))
    );
    h.press(KeyCode::Char('y'));
    assert!(h.take_effects().contains(&Effect::Copy("first".into())));
    h.press(KeyCode::Down);
    h.press(KeyCode::Char('s'));
    assert!(matches!(&h.modal, Some(Modal::Prompt(p)) if p.editor.text().contains("e621.net")));
    h.press(KeyCode::Esc);
    h.press(KeyCode::Enter);
    assert_eq!(h.chat.mode, chat::ChatMode::Normal);
    assert!(h.input.text().starts_with("> \"look https://e621.net/posts/9 *grins*\" draft"), "{}", h.input.text());
}

#[test]
fn select_mode_saves_message_as_snippet() {
    let mut h = harness().online().with_prefs().partnered();
    h.type_str("my go-to intro line");
    h.press(KeyCode::Enter);
    h.run_command(Command::Select);
    h.press(KeyCode::Char('n'));
    h.type_str("intro");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.snippet("intro").unwrap().text, "my go-to intro line");
}

#[test]
fn searching_the_chat() {
    let mut h = harness().online().with_prefs().partnered();
    for text in ["the old lighthouse", "something else", "LIGHTHOUSE again"] {
        h.server(ServerMessage::ReceiveMessage(text.into()));
    }
    h.run_command(Command::Search(None));
    h.type_str("lighthouse");
    let chat::ChatMode::Search(s) = &h.chat.mode else { panic!() };
    assert_eq!(s.hits.len(), 2);
    assert_eq!(s.current, 1, "starts at the newest hit");
    assert!(s.editing);
    h.press(KeyCode::Enter);
    h.press(KeyCode::Char('n'));
    let chat::ChatMode::Search(s) = &h.chat.mode else { panic!() };
    assert_eq!(h.chat.entries[s.current_entry().unwrap()].kind, EntryKind::Partner("the old lighthouse".into()));
    // Enter selects the hit for quoting.
    h.press(KeyCode::Enter);
    assert!(matches!(h.chat.mode, chat::ChatMode::Select(_)));
    h.press(KeyCode::Esc);
    h.run_command(Command::Search(Some("volcano".into())));
    assert!(h.last_toast().unwrap().contains("Nothing in this chat matches"));
    h.press(KeyCode::Esc);
    assert_eq!(h.chat.mode, chat::ChatMode::Normal);
    // Typing works normally again.
    h.type_str("hi");
    assert_eq!(h.input.text(), "hi");
}

#[test]
fn clicking_a_plain_message_selects_it() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("no links here".into()));
    frame(&mut h);
    let entry = h.chat.entries.len() - 1;
    click_on(&mut h, &Hit::Message { entry, links: vec![] });
    assert_eq!(h.chat.mode, chat::ChatMode::Select(entry));
}

#[test]
fn palette_finds_and_runs_everything() {
    let mut h = harness().online().with_prefs();
    h.run_command(Command::SnipAdd { name: "intro".into(), text: "hello".into() });
    h.ctrl('k');
    assert!(matches!(h.modal, Some(Modal::Palette { .. })));
    h.type_str("theme nord");
    let top = h.palette_matches("theme nord");
    assert_eq!(top[0].item, PaletteItem::Theme("nord".into()));
    h.press(KeyCode::Enter);
    assert_eq!(h.config.settings.theme, "nord");

    assert_eq!(h.palette_matches("snippet intro")[0].item, PaletteItem::Snippet(0));
    assert!(matches!(h.palette_matches("transparent")[0].item, PaletteItem::Setting(settings::Row::Transparent)));
    assert_eq!(h.palette_matches("find a partner")[0].item, PaletteItem::Action(crate::keymap::Action::Find));

    // Commands that take arguments are started in the message box.
    h.run_palette_item(PaletteItem::Command("/nick [name]"));
    assert!(h.last_toast().unwrap().contains("Nicknames"), "no-arg form runs directly");
    h.run_palette_item(PaletteItem::Command("/snip-add <name> <text>"));
    assert_eq!(h.input.text(), "/snip-add ");
    assert!(h.palette_matches("zzzzqqq").is_empty());
}

#[test]
fn stats_count_the_session_and_persist() {
    let mut h = harness().online().with_prefs();
    h.config.active_mut().preferences.toggle(Field::Limits, "Scat");
    h.ctrl('f');
    connected_to(&mut h, "Scat", None);
    let mut h = h.partnered();
    h.server(ServerMessage::ReceiveMessage("hey".into()));
    h.type_str("hi");
    h.press(KeyCode::Enter);
    h.server(ServerMessage::PartnerLeft);
    for s in [&h.stats, &h.run_stats] {
        assert_eq!((s.partners, s.skipped, s.sent, s.received, s.chats), (1, 1, 1, 1, 1));
    }
    h.flush();
    let saved = crate::stats::Stats::load(&h.paths.stats_file).unwrap();
    assert_eq!(saved.partners, 1);
    h.run_command(Command::Stats);
    assert!(matches!(h.modal, Some(Modal::Stats)));
    h.press(KeyCode::Char('x'));
    assert!(h.modal.is_none());
}
