//! Names, message selection and chat search.

use super::*;
use pretty_assertions::assert_eq;

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
