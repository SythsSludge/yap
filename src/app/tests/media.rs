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
    // The viewer stays open with the reason, so the chat's other images are a key away.
    assert_eq!(h.viewer.as_ref().unwrap().note, Some(crate::images::ViewerNote::Failed("HTTP 404".into())));
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

#[test]
fn arrows_step_through_the_chats_images() {
    use crate::images::ViewerNote;
    let mut h = harness().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("https://i.imgur.com/one.png and https://example.com/page".into()));
    h.type_str("https://random.host/two.jpg");
    h.press(KeyCode::Enter);
    h.server(ServerMessage::ReceiveMessage("https://i.imgur.com/three.png https://i.imgur.com/one.png".into()));
    let urls: Vec<String> = h.chat_images().into_iter().map(|(u, _)| u).collect();
    assert_eq!(urls, ["https://i.imgur.com/one.png", "https://random.host/two.jpg", "https://i.imgur.com/three.png"]);

    h.preview("https://i.imgur.com/one.png");
    assert_eq!(h.viewer_position(), Some((0, 3)));
    h.press(KeyCode::Left);
    assert_eq!(h.viewer_position(), Some((0, 3)), "no wrapping past the first");

    // The untrusted one asks inside the viewer instead of fetching.
    h.take_effects();
    h.press(KeyCode::Right);
    let v = h.viewer.as_ref().unwrap();
    assert_eq!(v.url, "https://random.host/two.jpg");
    assert_eq!(v.note, Some(ViewerNote::Untrusted { host: "random.host".into() }));
    assert!(h.take_effects().is_empty());
    h.press(KeyCode::Enter);
    assert_eq!(
        h.take_effects(),
        vec![Effect::FetchImage { url: "https://random.host/two.jpg".into(), allow_host: Some("random.host".into()) }]
    );
    assert_eq!(h.viewer.as_ref().unwrap().note, None);

    h.press(KeyCode::Right);
    assert_eq!(h.viewer_position(), Some((2, 3)));
    h.press(KeyCode::Right);
    assert_eq!(h.viewer_position(), Some((2, 3)), "no wrapping past the last");
    h.press(KeyCode::Esc);
    assert!(h.viewer.is_none());
}
