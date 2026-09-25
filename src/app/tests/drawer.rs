//! The drawer: links, tags, snippets and the editor.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn drawer_add_flow_and_sharing() {
    let mut h = harness().online().with_prefs().partnered();
    h.press(KeyCode::F(4));
    h.press(KeyCode::Char('a'));
    h.ctrl('u');
    h.type_str("https://i.imgur.com/ref.png");
    h.press(KeyCode::Enter);
    let Some(Modal::Prompt(p)) = &h.modal else { panic!("expected label prompt") };
    assert_eq!(p.editor.text(), "i.imgur.com · ref.png");
    h.ctrl('u');
    h.type_str("Ref sheet");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.items[0].label, "Ref sheet");

    h.press(KeyCode::Char('i'));
    assert_eq!(h.tab, Tab::Chat);
    assert_eq!(h.input.text(), "https://i.imgur.com/ref.png");
    assert!(h.sent().contains(&ClientMessage::Typing(true)));
}

#[test]
fn chat_side_drawer_inserts_links() {
    let mut h = harness();
    h.drawer.add("https://e621.net/posts/1", "ref", chrono::Utc::now()).unwrap();
    h.type_str("look:");
    h.ctrl('e');
    assert_eq!(h.chat_focus, ChatFocus::Drawer);
    h.press(KeyCode::Enter);
    assert_eq!(h.input.text(), "look: https://e621.net/posts/1");
    assert_eq!(h.chat_focus, ChatFocus::Input);
}

#[test]
fn link_picker_saves_and_trusts() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("see https://cdn.furry.art/x.png".into()));
    h.ctrl('o');
    assert!(matches!(h.modal, Some(Modal::Links { .. })));
    h.press(KeyCode::Char('t'));
    assert!(h.config.settings.images.trusted_domains.contains(&"cdn.furry.art".to_string()));
    h.ctrl('o');
    h.press(KeyCode::Char('s'));
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.items[0].url, "https://cdn.furry.art/x.png");
}

#[test]
fn drawer_tags_edit_and_filter() {
    let mut h = harness();
    h.run_command(Command::Save { url: "https://a.com/".into(), label: "alpha #ref".into() });
    h.run_command(Command::Save { url: "https://b.com/".into(), label: "beta".into() });
    h.press(KeyCode::F(4));
    h.press(KeyCode::Char('t'));
    h.type_str("outfits, ref");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.items[0].tags, vec!["outfits", "ref"]);

    h.press(KeyCode::Char(']'));
    assert_eq!(h.drawer_ui.tag.as_deref(), Some("outfits"));
    assert_eq!(h.drawer_items(), vec![0]);
    h.press(KeyCode::Char(']'));
    assert_eq!(h.drawer_ui.tag.as_deref(), Some("ref"));
    assert_eq!(h.drawer_items().len(), 2);
    h.press(KeyCode::Char(']'));
    assert_eq!(h.drawer_ui.tag, None, "wraps back to all");
    h.press(KeyCode::Char('['));
    assert_eq!(h.drawer_ui.tag.as_deref(), Some("ref"));
    h.press(KeyCode::Esc);
    assert_eq!(h.drawer_ui.tag, None);
}

// ----- snippets, editor, drawer export ----------------------------------------------

#[test]
fn snippets_insert_with_placeholders() {
    let mut h = harness().online().with_prefs().partnered();
    h.type_str("/snip-add intro Hi {partner_species}! I'm a {species}.");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.snippets[0].name, "intro");
    h.take_effects();
    h.type_str("/snip intro");
    h.press(KeyCode::Enter);
    assert_eq!(h.input.text(), "Hi Fox! I'm a Wolf.");
    assert!(h.sent().contains(&ClientMessage::Typing(true)));

    h.input.clear();
    h.ctrl('g');
    h.type_str("int");
    h.press(KeyCode::Enter);
    assert_eq!(h.input.text(), "Hi Fox! I'm a Wolf.");

    h.run_command(Command::Snip(Some("nope".into())));
    assert!(h.last_toast().unwrap().contains("No snippet called `nope`"));
}

#[test]
fn snippet_shelf_add_and_delete() {
    let mut h = harness();
    h.press(KeyCode::F(4));
    h.press(KeyCode::Char('s'));
    assert_eq!(h.drawer_ui.shelf, Shelf::Snippets);
    h.press(KeyCode::Char('a'));
    h.type_str("bye");
    h.press(KeyCode::Enter);
    h.type_str("Thanks, take care!");
    h.press(KeyCode::Enter);
    assert_eq!(h.drawer.snippets[0].text, "Thanks, take care!");
    // An empty text opens the editor instead.
    h.press(KeyCode::Char('a'));
    h.type_str("long");
    h.press(KeyCode::Enter);
    h.press(KeyCode::Enter);
    assert!(
        h.take_effects().iter().any(|e| matches!(e, Effect::OpenEditor { purpose: EditorPurpose::Snippet(_), .. }))
    );
    h.on_editor(EditorPurpose::Snippet(1), Ok("Line one\nline two\n".into()));
    assert_eq!(h.drawer.snippets[1].text, "Line one\nline two");
    h.drawer_ui.snippets.selected = 0;
    h.press(KeyCode::Char('d'));
    h.press(KeyCode::Char('y'));
    assert_eq!(h.drawer.snippets.len(), 1);
}

#[test]
fn editor_round_trip_joins_paragraphs() {
    let mut h = harness().online().with_prefs().partnered();
    h.config.settings.editor = "my-editor --wait".into();
    h.input.set("first / second");
    h.ctrl('x');
    let effects = h.take_effects();
    let Some(Effect::OpenEditor { command, text, purpose }) = effects.first() else { panic!("{effects:?}") };
    assert_eq!(command, "my-editor --wait");
    assert_eq!(text, "first\n\nsecond\n");
    assert_eq!(*purpose, EditorPurpose::Message);

    h.on_editor(EditorPurpose::Message, Ok("A longer\npost.\n\nNew paragraph.\n".into()));
    assert_eq!(h.input.text(), "A longer post. / New paragraph.");
    h.on_editor(EditorPurpose::Message, Err("editor crashed".into()));
    assert_eq!(h.last_toast(), Some("Editor: editor crashed"));
}

#[test]
fn drawer_export_and_import() {
    let mut h = harness();
    h.run_command(Command::Save { url: "https://a.com/".into(), label: "a #ref".into() });
    h.run_command(Command::SnipAdd { name: "hi".into(), text: "hello".into() });
    let path = h._dir.path().join("drawer.json");
    h.run_command(Command::DrawerExport(path.display().to_string()));
    let mut other = harness();
    other.run_command(Command::DrawerImport(path.display().to_string()));
    assert_eq!(other.last_toast(), Some("Imported 1 links and 1 snippets"));
    assert_eq!(other.drawer.items[0].tags, vec!["ref"]);
}
