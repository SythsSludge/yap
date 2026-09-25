//! Alerts, desktop notifications, keywords and sound.

use super::*;
use pretty_assertions::assert_eq;

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
