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

#[test]
fn outside_edits_to_config_and_themes_are_picked_up() {
    let mut h = harness();
    let paths = h.paths.clone();
    let tick = |h: &mut Harness| {
        h.now += Duration::from_secs(1);
        h.on_tick();
    };
    // yap's own saves aren't outside edits.
    h.config.settings.timestamps = false;
    h.config_changed();
    h.flush();
    tick(&mut h);
    assert!(h.toasts.is_empty());

    let mut edited = h.config.clone();
    edited.settings.theme = "nord".into();
    edited.save(&paths.config_file).unwrap();
    tick(&mut h);
    assert_eq!(h.theme.name, "nord");
    assert_eq!(h.last_toast(), Some("Reloaded config.toml."));

    // A broken file is reported and the current settings are kept.
    std::fs::write(&paths.config_file, "theme = [").unwrap();
    tick(&mut h);
    assert!(h.last_toast().unwrap().ends_with("Keeping the current settings."));
    assert_eq!(h.config.settings.theme, "nord");

    // New theme files show up without a restart.
    std::fs::create_dir_all(&paths.themes_dir).unwrap();
    std::fs::write(paths.themes_dir.join("mine.toml"), "extends = \"nord\"\n[colors]\naccent = \"#ff79c6\"\n").unwrap();
    tick(&mut h);
    assert!(h.themes.iter().any(|t| t.name == "mine"));
    assert_eq!(h.last_toast(), Some("Reloaded themes."));
}

#[test]
fn ui_tweaks_behave() {
    // Errors stay until Esc; other toasts fade.
    let mut h = harness();
    h.toast(Level::Error, "boom");
    h.toast(Level::Info, "fyi");
    h.now += Duration::from_secs(30);
    h.on_tick();
    assert_eq!(h.toasts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["boom"]);
    h.press(KeyCode::Esc);
    assert!(h.toasts.is_empty());

    // The title counts unseen messages.
    let mut h = harness().online().with_prefs().partnered();
    h.tab = Tab::Drawer;
    h.server(ServerMessage::ReceiveMessage("hi".into()));
    assert_eq!(h.window_title().rsplit(" · ").next(), Some("(1) yap"));

    // Narrow terminals toggle an overlay instead of the setting.
    h.tab = Tab::Chat;
    h.narrow = true;
    h.ctrl('s');
    assert!(h.sidebar_overlay);
    assert!(h.config.settings.show_sidebar, "the saved setting is left alone");

    // The welcome leads to the first thing to fill in.
    let mut h = harness();
    h.welcome();
    h.press(KeyCode::Enter);
    assert_eq!(h.tab, Tab::Preferences);
    assert_eq!(Field::ALL[h.prefs_ui.field], Field::Gender);
}

#[test]
fn footer_hints_are_clickable() {
    let mut h = harness().online().with_prefs();
    frame(&mut h);
    let find = h.keymap.hint(crate::keymap::Action::Find).unwrap();
    click_on(&mut h, &Hit::HintKey(find));
    assert!(find_payload(&h.sent()).is_some(), "clicking ^F find searches");

    h.tab = Tab::Drawer;
    frame(&mut h);
    click_on(&mut h, &Hit::HintKey("a".into()));
    assert!(matches!(h.modal, Some(Modal::Prompt(_))), "clicking `a add` adds");
}
