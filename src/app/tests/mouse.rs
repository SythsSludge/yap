//! Clicking.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn clicking_tabs_rows_and_double_clicks() {
    let mut h = harness();
    frame(&mut h);
    click_on(&mut h, &Hit::Tab(Tab::Settings));
    assert_eq!(h.tab, Tab::Settings);
    let row = settings::rows(&h.config.settings).iter().position(|r| *r == settings::Row::Timestamps).unwrap();
    let target = Hit::Row { list: ListId::Settings, index: row };
    click_on(&mut h, &target);
    assert_eq!(h.settings_ui.selected, row);
    assert!(h.config.settings.timestamps, "a single click only selects");
    click_on(&mut h, &target);
    assert!(!h.config.settings.timestamps, "a double click toggles");

    // Option checkboxes toggle on every click.
    click_on(&mut h, &Hit::Tab(Tab::Preferences));
    h.prefs_ui.field = Field::ALL.iter().position(|f| *f == Field::Kinks).unwrap();
    frame(&mut h);
    click_on(&mut h, &Hit::Row { list: ListId::PrefsOptions, index: 1 });
    assert_eq!(h.config.active().preferences.kinks, vec!["any", "3+ Penetration"]);
}

#[test]
fn clicking_links_images_and_viewer_buttons() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("gallery: https://example.com/g".into()));
    h.take_effects();
    frame(&mut h);
    let entry = h.chat.entries.len() - 1;
    click_on(&mut h, &Hit::Message { entry, links: vec!["https://example.com/g".into()] });
    assert_eq!(h.take_effects(), vec![Effect::OpenUrl("https://example.com/g".into())]);

    // A loaded inline image opens the viewer; its buttons work.
    let url = "https://i.imgur.com/pic.png".to_string();
    let png = {
        let img: image::RgbaImage = image::ImageBuffer::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let loaded = crate::images::prepare(
        &h.picker.clone(),
        crate::images::decode(&png).unwrap(),
        png.clone(),
        ratatui::layout::Size::new(4, 2),
    )
    .unwrap();
    h.server(ServerMessage::ReceiveMessage(url.clone()));
    h.on_image(url.clone(), Ok(loaded));
    frame(&mut h);
    click_on(&mut h, &Hit::Image(url.clone()));
    assert!(h.viewer.is_some());
    click_on(&mut h, &Hit::Viewer(ViewerButton::SaveImage));
    assert!(h.last_toast().unwrap().starts_with("Saved"));
    assert_eq!(std::fs::read(h.paths.downloads_dir.join("pic.png")).unwrap(), png);
    click_on(&mut h, &Hit::Viewer(ViewerButton::Close));
    assert!(h.viewer.is_none());
}

#[test]
fn clicking_sessions_and_new_chat() {
    let mut h = harness();
    h.new_session();
    frame(&mut h);
    click_on(&mut h, &Hit::Session(0));
    assert_eq!(h.session_id, 0);
    click_on(&mut h, &Hit::NewSession);
    assert_eq!(h.session_count(), 3);
}

#[test]
fn clicking_a_plain_message_selects_it() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("no links here".into()));
    frame(&mut h);
    let entry = h.chat.entries.len() - 1;
    click_on(&mut h, &Hit::Message { entry, links: vec![] });
    assert_eq!(h.chat.mode, chat::ChatMode::Select(entry));
}
