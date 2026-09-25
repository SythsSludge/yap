//! Skipping, auto-requeue and auto-skip rules.

use super::*;
use pretty_assertions::assert_eq;

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
