//! The Settings tab and rebindable keys.

use super::*;
use pretty_assertions::assert_eq;

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
