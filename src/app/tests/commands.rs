//! Slash commands, export/import and trusted domains.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn slash_commands_run_instead_of_sending() {
    let mut h = harness().online().with_prefs().partnered();
    h.type_str("/save https://e621.net/posts/1 my ref");
    h.take_effects();
    h.press(KeyCode::Enter);
    assert!(!h.sent().iter().any(|m| matches!(m, ClientMessage::SendMessage(_))));
    assert_eq!(h.drawer.items[0].label, "my ref");
    h.flush();
    assert!(h.paths.drawer_file.exists());

    h.type_str("//me waves");
    h.press(KeyCode::Enter);
    assert!(h.sent().contains(&ClientMessage::SendMessage("/me waves".into())));

    h.type_str("/theme nord");
    h.press(KeyCode::Enter);
    assert_eq!(h.theme.name, "nord");
    assert_eq!(h.config.settings.theme, "nord");

    h.type_str("/nope");
    h.press(KeyCode::Enter);
    assert!(h.last_toast().unwrap().contains("unknown command"));
    assert_eq!(h.input.text(), "/nope", "bad commands stay editable");
}

#[test]
fn export_and_import_via_commands() {
    let mut h = harness();
    h.config.active_mut().preferences = complete_prefs();
    let path = h._dir.path().join("backup.json");
    h.run_command(Command::ExportAll(path.display().to_string()));
    assert!(h.last_toast().unwrap().starts_with("Exported"));
    h.run_command(Command::Import { path: path.display().to_string(), with_settings: false });
    assert_eq!(h.config.profiles.len(), 2);
    assert_eq!(h.config.profiles[1].name, "default (2)");
    assert_eq!(h.config.profiles[1].preferences, complete_prefs());

    h.run_command(Command::Import { path: "/definitely/missing.toml".into(), with_settings: false });
    assert!(h.last_toast().unwrap().starts_with("Import failed"));
}

#[test]
fn trust_and_untrust_domains() {
    let mut h = harness();
    h.run_command(Command::Trust("https://Cdn.Example.org/x".into()));
    assert!(h.config.settings.images.trusted_domains.contains(&"cdn.example.org".to_string()));
    h.run_command(Command::Untrust("cdn.example.org".into()));
    assert!(!h.config.settings.images.trusted_domains.contains(&"cdn.example.org".to_string()));
    h.run_command(Command::Trust("nope".into()));
    assert_eq!(h.last_toast(), Some("`nope` isn't a domain."));
}

#[test]
fn tab_completes_slash_commands() {
    let mut h = harness().online().with_prefs();
    h.drawer_panel = true;
    h.type_str("/lo");
    h.press(KeyCode::Tab);
    assert_eq!(h.input.text(), "/log ");
    assert_eq!(h.chat_focus, ChatFocus::Input, "Tab completed rather than moving to the drawer");
    h.input.clear();
    h.type_str("/sta");
    h.press(KeyCode::Tab);
    assert_eq!(h.input.text(), "/stats");
    h.press(KeyCode::Enter);
    assert!(matches!(h.modal, Some(Modal::Stats)));

    // With nothing to complete, Tab still moves to the drawer panel.
    h.modal = None;
    h.input.clear();
    h.press(KeyCode::Tab);
    assert_eq!(h.chat_focus, ChatFocus::Drawer);
}
