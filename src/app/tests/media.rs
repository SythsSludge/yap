//! Image previews and the viewer.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn trusted_image_links_are_fetched_automatically() {
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage(
        "ref: https://static1.e621.net/data/ab/cd/x.png and https://evil.example/y.png and https://e621.net/posts/1"
            .into(),
    ));
    let fetches: Vec<_> = h.take_effects().into_iter().filter(|e| matches!(e, Effect::FetchImage { .. })).collect();
    assert_eq!(
        fetches,
        vec![Effect::FetchImage { url: "https://static1.e621.net/data/ab/cd/x.png".into(), allow_host: None }]
    );
    // Seen again: not refetched.
    h.server(ServerMessage::ReceiveMessage("again https://static1.e621.net/data/ab/cd/x.png".into()));
    assert!(!h.take_effects().iter().any(|e| matches!(e, Effect::FetchImage { .. })));

    h.config.settings.images.enabled = false;
    h.server(ServerMessage::ReceiveMessage("https://i.imgur.com/new.png".into()));
    assert!(!h.take_effects().iter().any(|e| matches!(e, Effect::FetchImage { .. })));
}

#[test]
fn previewing_untrusted_image_needs_consent() {
    let mut h = harness();
    h.preview("https://random.host/pic.png");
    assert!(matches!(h.modal, Some(Modal::Confirm { action: Confirm::LoadUntrusted(_), .. })));
    h.press(KeyCode::Char('y'));
    assert_eq!(
        h.take_effects(),
        vec![Effect::FetchImage { url: "https://random.host/pic.png".into(), allow_host: Some("random.host".into()) }]
    );
    assert!(h.viewer.is_some());
    h.on_image("https://random.host/pic.png".into(), Err("HTTP 404".into()));
    assert!(h.viewer.is_none());
    assert_eq!(h.last_toast(), Some("Couldn't load image: HTTP 404"));
}

#[test]
fn images_arriving_fill_the_viewer() {
    let mut h = harness();
    h.preview("https://i.imgur.com/a.png");
    assert!(h.viewer.as_ref().unwrap().protocol.is_none());
    let img = image::DynamicImage::new_rgba8(8, 8);
    let loaded =
        crate::images::prepare(&h.picker.clone(), img, vec![1, 2, 3], ratatui::layout::Size::new(4, 4)).unwrap();
    h.on_image("https://i.imgur.com/a.png".into(), Ok(loaded));
    assert!(h.viewer.as_ref().unwrap().protocol.is_some());
    assert!(matches!(h.images.get("https://i.imgur.com/a.png"), Some(ImageState::Ready(_))));
    h.press(KeyCode::Esc);
    assert!(h.viewer.is_none());
}
