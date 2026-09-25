//! Finding, chatting with, blocking and leaving partners; the connection.

use super::*;
use pretty_assertions::assert_eq;

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
