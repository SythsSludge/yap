//! Chat logs.

use super::*;
use pretty_assertions::assert_eq;

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
fn chats_save_as_markdown_or_html_by_extension() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("*waves* hi".into()));
    let dir = h._dir.path().to_owned();
    let save = |h: &mut Harness, name: &str| {
        h.run_command(crate::commands::Command::SaveLog(dir.join(name).display().to_string()));
        std::fs::read_to_string(dir.join(name)).unwrap()
    };
    let html = save(&mut h, "chat.html");
    assert!(html.contains("<title>Chat with Dominant Female Fox</title>"));
    assert!(html.contains("<em>*waves*</em> hi"));
    let md = save(&mut h, "chat.md");
    assert!(md.starts_with("# Chat with Dominant Female Fox"));
    assert!(md.contains("**Partner** · "), "{md}");
    let txt = save(&mut h, "chat.txt");
    assert!(txt.contains("] Partner: *waves* hi"));
}
