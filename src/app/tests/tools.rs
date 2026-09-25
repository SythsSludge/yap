//! The traffic viewer, command palette and stats.

use super::*;
use pretty_assertions::assert_eq;

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

#[test]
fn kinks_popup_opens_from_command_key_and_sidebar() {
    let mut h = harness().online().with_prefs();
    h.type_str("/kinks");
    h.press(KeyCode::Enter);
    assert!(matches!(h.modal, Some(Modal::Kinks { scroll: 0 })), "works without a partner too");
    h.press(KeyCode::Down);
    assert!(matches!(h.modal, Some(Modal::Kinks { scroll: 1 })));
    h.press(KeyCode::Esc);
    assert!(h.modal.is_none());

    let mut h = h.partnered();
    let groups = h.kink_groups().unwrap();
    assert_eq!(groups.shared, ["Musk", "Biting"]);
    assert_eq!(groups.theirs, ["Tickling"]);
    alt(&mut h, 'k');
    assert!(matches!(h.modal, Some(Modal::Kinks { .. })));
    h.press(KeyCode::Esc);
    frame(&mut h);
    click_on(&mut h, &Hit::Kinks);
    assert!(matches!(h.modal, Some(Modal::Kinks { .. })));
}

#[test]
fn history_records_how_each_partner_went() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("hi".into()));
    h.type_str("hey");
    h.press(KeyCode::Enter);
    h.server(ServerMessage::PartnerLeft);
    let r = h.history.records.last().unwrap().clone();
    assert_eq!(r.outcome, crate::history::Outcome::TheyLeft);
    assert_eq!((r.sent, r.received, r.shared_kinks), (1, 1, 2));
    assert_eq!(r.partner(), "Dominant Female Fox");
    assert_eq!(r.profile, "default");

    // Re-rolling counts as you leaving; an auto-skip says which rule.
    let mut h = h.partnered();
    h.ctrl('n');
    assert_eq!(h.history.records.last().unwrap().outcome, crate::history::Outcome::YouLeft);
    h.config.active_mut().preferences.toggle(Field::Limits, "Scat");
    connected_to(&mut h, "Scat", None);
    let r = h.history.records.last().unwrap();
    assert_eq!((r.outcome, r.skip), (crate::history::Outcome::Skipped, Some(crate::history::SkipRule::Limit)));
    assert_eq!(h.history.records.len(), 3);

    // Saved on flush, and the popup opens from /history or h in /stats.
    h.flush();
    assert_eq!(std::fs::read_to_string(&h.paths.history_file).unwrap().lines().count(), 3);
    h.run_command(Command::Stats);
    h.press(KeyCode::Char('h'));
    assert!(matches!(h.modal, Some(Modal::History { scroll: 0 })));
    h.press(KeyCode::Char('x'));
    h.press(KeyCode::Char('y'));
    assert!(h.history.records.is_empty());
    assert!(!h.paths.history_file.exists());

    // Turned off, nothing is kept.
    h.config.settings.keep_history = false;
    let mut h = h.partnered();
    h.server(ServerMessage::PartnerLeft);
    assert!(h.history.records.is_empty());
}

#[test]
fn activity_records_searches_and_online_counts() {
    let mut h = harness().online().with_prefs();
    assert_eq!(h.activity.online.iter().map(|b| b[0]).sum::<u64>(), 1, "the count from connecting");
    h.ctrl('f');
    h.now += Duration::from_secs(40);
    let h = h.partnered();
    let searches: Vec<[u64; 2]> = h.activity.searches.iter().copied().filter(|b| b[0] > 0).collect();
    assert_eq!(searches, [[1, 40]]);
    let mut h = h;
    h.flush();
    assert!(h.paths.activity_file.exists());
}

#[test]
fn buddy_reacts_and_can_be_changed() {
    use crate::buddy::Mood;
    let mut h = harness().online().with_prefs();
    assert_eq!(h.buddy().unwrap().name, "fox");
    assert_eq!(h.buddy_mood(), Mood::Idle);
    h.ctrl('f');
    assert_eq!(h.buddy_mood(), Mood::Searching);
    let mut h = h.partnered();
    assert_eq!(h.buddy_mood(), Mood::Excited, "a match");
    assert!(h.buddy_speech().is_some());
    h.now += Duration::from_secs(10);
    assert_eq!(h.buddy_mood(), Mood::Idle, "reactions wear off");
    assert_eq!(h.buddy_speech(), None);

    h.config.active_mut().character = "Ember".into();
    h.server(ServerMessage::ReceiveMessage("hey Ember!".into()));
    assert_eq!(h.buddy_mood(), Mood::Surprised, "your name came up");
    h.now += Duration::from_secs(10);
    h.server(ServerMessage::ReceiveMessage("love that <3".into()));
    assert_eq!(h.buddy_mood(), Mood::Love);
    h.now += Duration::from_secs(10);
    h.server(ServerMessage::PartnerTyping(true));
    h.now += Duration::from_secs(10);
    assert_eq!(h.buddy_mood(), Mood::Curious, "still typing");
    h.server(ServerMessage::PartnerLeft);
    assert_eq!(h.buddy_mood(), Mood::Sad);

    // Background chats don't move it.
    h.now += Duration::from_secs(20);
    h.tab = Tab::Drawer;
    h.server(ServerMessage::ReceiveMessage("psst".into()));
    assert_eq!(h.buddy_mood(), Mood::Idle);

    h.run_command(Command::Buddy(Some("cat".into())));
    assert_eq!(h.buddy().unwrap().name, "cat");
    h.run_command(Command::Buddy(Some("off".into())));
    assert!(h.buddy().is_none());
    assert!(h.buddy_frame().is_empty());
}

#[test]
fn custom_buddies_load_from_the_config_folder() {
    let mut h = harness();
    let dir = h.paths.config_file.parent().unwrap().join("buddies");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bun.toml"), "extends = \"cat\"\n[moods]\nidle = ['''\n (\\_/)\n ( •.•)''']\n").unwrap();
    assert!(h.load_buddies().is_empty());
    h.run_command(Command::Buddy(Some("bun".into())));
    h.now += Duration::from_secs(10);
    assert_eq!(h.buddy_frame(), [" (\\_/)", " ( •.•)"]);
}
