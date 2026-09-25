//! Preferences, profiles and themes.

use super::*;
use pretty_assertions::assert_eq;

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
