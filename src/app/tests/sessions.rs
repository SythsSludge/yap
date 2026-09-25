//! Several chats at once.

use super::*;
use pretty_assertions::assert_eq;

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
