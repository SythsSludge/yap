use super::draw;
use crate::app::Tab;
use crate::app::chat::EntryKind;
use crate::app::modal::{Confirm, Modal};
use crate::app::tests::{Harness, harness};
use crate::net::{NetEvent, TrafficRecord};
use crate::protocol::ServerMessage;
use crate::traffic::{Direction, FrameKind};
use chrono::Timelike;
use chrono::{DateTime, Local};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::KeyCode;

fn render(app: &mut Harness, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f, &mut app.app)).unwrap();
    terminal.backend().to_string()
}

fn render_backend(app: &mut Harness, w: u16, h: u16) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f, &mut app.app)).unwrap();
    terminal.backend().clone()
}

/// A deterministic app: no timestamps, fixed traffic times.
fn app() -> Harness {
    let mut h = harness();
    h.config.settings.timestamps = false;
    h
}

fn fixed_time() -> DateTime<Local> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap().into()
}

fn chatting() -> Harness {
    let mut h = app().online().with_prefs().partnered();
    h.server(ServerMessage::ReceiveMessage("hey there! got a ref? https://e621.net/posts/123".into()));
    h.type_str("sure, one sec");
    h.press(KeyCode::Enter);
    h.server(ServerMessage::PartnerTyping(true));
    h
}

#[test]
fn welcome_screen() {
    let mut h = app();
    insta::assert_snapshot!(render(&mut h, 100, 24));
}

#[test]
fn chat_with_partner() {
    let mut h = chatting();
    insta::assert_snapshot!(render(&mut h, 110, 26));
}

#[test]
fn compact_chat_with_drawer_panel() {
    let mut h = chatting();
    h.config.settings.chat_style = crate::config::ChatStyle::Compact;
    h.drawer.add("https://e621.net/posts/1", "Ref sheet", chrono::Utc::now()).unwrap();
    h.ctrl('e');
    insta::assert_snapshot!(render(&mut h, 110, 22));
}

#[test]
fn messages_layout() {
    let mut h = chatting();
    h.config.settings.chat_style = crate::config::ChatStyle::Sms;
    h.server(ServerMessage::ReceiveMessage(
        "a longer reply that should wrap inside its bubble on the left side".into(),
    ));
    insta::assert_snapshot!(render(&mut h, 100, 26));
}

#[test]
fn logs_screen() {
    let mut h = chatting();
    h.chat.entries.iter_mut().for_each(|e| e.at -= chrono::TimeDelta::seconds(10));
    h.server(ServerMessage::PartnerLeft);
    // The list and title show when the chat happened; pin it so the snapshot is stable.
    for conv in &mut h.logs.items {
        conv.started = fixed_time();
        conv.ended = Some(fixed_time() + chrono::Duration::minutes(12));
    }
    h.press(KeyCode::F(5));
    h.press(KeyCode::Enter);
    insta::assert_snapshot!(render(&mut h, 110, 22));
}

#[test]
fn drawer_screen_with_tags() {
    let mut h = app();
    h.run_command(crate::commands::Command::Save {
        url: "https://e621.net/posts/1".into(),
        label: "Ref sheet #ref".into(),
    });
    h.run_command(crate::commands::Command::Save {
        url: "https://example.com/gallery".into(),
        label: "Gallery #art #ref".into(),
    });
    h.drawer.items.iter_mut().for_each(|i| i.added = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap());
    h.toasts.clear();
    h.press(KeyCode::F(4));
    insta::assert_snapshot!(render(&mut h, 100, 16));
}

#[test]
fn transparent_background_paints_nothing() {
    let mut h = chatting();
    h.config.settings.transparent_background = true;
    h.refresh_theme();
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|f| draw(f, &mut h.app)).unwrap();
    let buf = terminal.backend().buffer();
    // Body cells outside bubbles, popups and chips use the terminal's own background.
    assert_eq!(buf[(50, 10)].bg, ratatui::style::Color::Reset);
    assert_eq!(buf[(0, 0)].bg, ratatui::style::Color::Reset);
}

#[test]
fn no_emoji_anywhere_in_the_ui() {
    for mut h in [chatting(), app()] {
        h.config.settings.images.auto_load = false;
        h.server(ServerMessage::ReceiveMessage("https://i.imgur.com/x.png".into()));
        for tab in Tab::ALL {
            h.tab = tab;
            let screen = render(&mut h, 120, 30);
            let emoji: Vec<char> = screen
                .chars()
                .filter(|c| matches!(*c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE0F))
                .collect();
            assert!(emoji.is_empty(), "{tab:?} shows {emoji:?}");
        }
    }
}

#[test]
fn preferences_screen() {
    let mut h = app().with_prefs();
    h.config.create_profile("switchy", Default::default()).unwrap();
    h.press(KeyCode::F(3));
    h.prefs_ui.field = 6;
    h.press(KeyCode::Enter);
    h.type_str("bit");
    insta::assert_snapshot!(render(&mut h, 110, 22));
}

#[test]
fn traffic_screen() {
    let mut h = app();
    let frames = [
        (Direction::Meta, FrameKind::Info, "connecting to wss://www.yiffspot.com/"),
        (Direction::In, FrameKind::Text, r#"{"type":"connection_success","data":"1700000000000-abc"}"#),
        (Direction::In, FrameKind::Text, r#"{"type":"update_user_count","data":1234}"#),
        (Direction::Out, FrameKind::Text, r#"{"type":"ping","data":true}"#),
        (Direction::Out, FrameKind::Text, r#"{"type":"send_message","data":"hello"}"#),
    ];
    for (dir, kind, body) in frames {
        h.on_net(NetEvent::Traffic(TrafficRecord { at: fixed_time(), dir, kind, size: body.len(), body: body.into() }));
    }
    h.press(KeyCode::F(6));
    insta::assert_snapshot!(render(&mut h, 120, 16));
}

#[test]
fn settings_screen() {
    let mut h = app();
    h.paths.config_file = "/home/you/.config/yap/config.toml".into();
    h.press(KeyCode::F(7));
    insta::assert_snapshot!(render(&mut h, 90, 20));
}

#[test]
fn confirm_modal_and_toast() {
    let mut h = chatting();
    h.toast(crate::app::Level::Error, "Please enter a message.");
    h.modal =
        Some(Modal::Confirm { text: "Are you sure you want to block this partner?".into(), action: Confirm::Block });
    let screen = render(&mut h, 100, 24);
    assert!(screen.contains("Are you sure you want to block this partner?"));
    assert!(screen.contains("y yes"));
    assert!(screen.contains("Please enter a message."));
}

#[test]
fn scrolled_up_shows_unread_badge() {
    let mut h = chatting();
    for i in 0..40 {
        h.server(ServerMessage::ReceiveMessage(format!("message {i}")));
    }
    render(&mut h, 100, 20);
    h.press(KeyCode::PageUp);
    h.server(ServerMessage::ReceiveMessage("new one".into()));
    let screen = render(&mut h, 100, 20);
    assert!(screen.contains("1 new message"), "{screen}");
    assert!(!screen.contains("new one"));
    h.press(KeyCode::Esc);
    assert!(render(&mut h, 100, 20).contains("new one"));
}

#[test]
fn common_kinks_are_highlighted() {
    let h = chatting();
    let EntryKind::PartnerInfo { common, .. } =
        &h.chat.entries.iter().find(|e| matches!(e.kind, EntryKind::PartnerInfo { .. })).unwrap().kind
    else {
        unreachable!()
    };
    assert_eq!(common.len(), 2);
    let mut h = h;
    let mut terminal = Terminal::new(TestBackend::new(110, 26)).unwrap();
    terminal.draw(|f| draw(f, &mut h.app)).unwrap();
    let buf = terminal.backend().buffer();
    let highlight = h.theme.highlight;
    let highlighted: String = buf.content().iter().filter(|c| c.fg == highlight).map(|c| c.symbol()).collect();
    assert!(highlighted.contains("Musk") && highlighted.contains("Biting"), "{highlighted}");
}

#[test]
fn every_screen_survives_tiny_terminals() {
    let modals: Vec<fn(&mut Harness)> = vec![
        |_| {},
        |h| h.modal = Some(Modal::Help { scroll: 3 }),
        |h| h.modal = Some(Modal::Confirm { text: "x".repeat(300), action: Confirm::Quit }),
        |h| h.open_prompt("A long title for a prompt", "/some/path", crate::app::modal::PromptAction::ExportAll),
        |h| h.open_links(),
        |h| h.open_profile_picker(),
        |h| h.open_theme_picker(),
        |h| h.preview("https://i.imgur.com/a.png"),
        |h| h.open_palette(),
        |h| h.modal = Some(Modal::Stats),
        |h| h.run_command(crate::commands::Command::Search(Some("hey".into()))),
        |h| h.select_message(None),
        |h| {
            for i in 0..6 {
                h.toast(crate::app::Level::Info, format!("toast number {i} with some text"));
            }
        },
    ];
    for tab in Tab::ALL {
        for (i, setup) in modals.iter().enumerate() {
            for (w, hgt) in [(1, 1), (8, 4), (20, 6), (40, 10), (61, 12)] {
                let mut h = chatting();
                h.drawer.add("https://e621.net/posts/1", "Ref", chrono::Utc::now()).unwrap();
                h.drawer_panel = true;
                h.tab = tab;
                setup(&mut h);
                h.input.set(&"long input ".repeat(40));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(&mut h, w, hgt)));
                assert!(result.is_ok(), "panicked on {tab:?} modal #{i} at {w}x{hgt}");
            }
        }
    }
}

#[test]
fn keys_section_and_capture_popup() {
    let mut h = app();
    h.keymap.bind(crate::keymap::Action::Find, crate::keymap::Chord::parse("alt+f").unwrap());
    h.press(KeyCode::F(7));
    let rows = crate::app::settings::rows(&h.config.settings);
    h.settings_ui.selected =
        rows.iter().position(|r| *r == crate::app::settings::Row::Key(crate::keymap::Action::Find)).unwrap();
    let screen = render(&mut h, 100, 30);
    assert!(screen.contains("Find a partner"), "{screen}");
    assert!(screen.contains("alt+f"));
    assert!(screen.contains("backspace default"));
    h.press(KeyCode::Enter);
    let screen = render(&mut h, 100, 30);
    assert!(screen.contains("Press the new key for find a partner"), "{screen}");
    assert!(screen.contains("currently alt+f"));
}

#[test]
fn roleplay_formatting_and_names_render() {
    let mut h = chatting();
    h.config.active_mut().character = "Ember".into();
    h.run_command(crate::commands::Command::Nick(Some("Rook".into())));
    h.server(ServerMessage::ReceiveMessage("*waves slowly* hello ((brb))".into()));
    // A new message clears the indicator; they start typing again.
    h.server(ServerMessage::PartnerTyping(true));
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|f| draw(f, &mut h.app)).unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("Ember") && screen.contains("Rook"), "{screen}");
    assert!(screen.contains("Rook is typing."));
    let buf = terminal.backend().buffer();
    let italic: String = buf
        .content()
        .iter()
        .filter(|c| c.modifier.contains(ratatui::style::Modifier::ITALIC))
        .map(|c| c.symbol())
        .collect();
    assert!(italic.contains("waves"), "{italic}");
    let dim: String = buf
        .content()
        .iter()
        .filter(|c| c.modifier.contains(ratatui::style::Modifier::DIM))
        .map(|c| c.symbol())
        .collect();
    assert!(dim.contains("brb"), "{dim}");
}

#[test]
fn palette_and_stats_popups_render() {
    let mut h = app();
    h.open_palette();
    let screen = render(&mut h, 100, 30);
    assert!(screen.contains("palette") && screen.contains("Find a partner"), "{screen}");
    h.modal = Some(Modal::Stats);
    let screen = render(&mut h, 100, 30);
    assert!(screen.contains("partners met") && screen.contains("this session"), "{screen}");
}

#[test]
fn search_bar_replaces_the_message_box() {
    let mut h = chatting();
    h.run_command(crate::commands::Command::Search(Some("ref".into())));
    let screen = render(&mut h, 110, 26);
    assert!(screen.contains("search") && screen.contains("1 of 1"), "{screen}");
}

#[test]
fn thousands_separators() {
    assert_eq!(super::thousands(0), "0");
    assert_eq!(super::thousands(999), "999");
    assert_eq!(super::thousands(1_000), "1,000");
    assert_eq!(super::thousands(1_234_567), "1,234,567");
}

#[test]
fn undelivered_messages_are_marked() {
    let mut h = chatting();
    h.type_str("you still there?");
    h.press(KeyCode::Enter);
    h.server(ServerMessage::PartnerLeft);
    let screen = render(&mut h, 110, 30);
    assert!(screen.contains("may not have arrived"), "{screen}");
}

#[test]
fn kinks_popup() {
    let mut h = chatting();
    h.toasts.clear();
    h.run_command(crate::commands::Command::Kinks);
    insta::assert_snapshot!(render(&mut h, 90, 20));
}

#[test]
fn emoji_suggestions_above_the_input() {
    let mut h = chatting();
    h.type_str("hi :wav");
    let screen = render(&mut h, 110, 30);
    assert!(screen.contains("tab"), "{screen}");
    assert!(screen.contains(":wave:"), "{screen}");
}

#[test]
fn command_hints_mark_what_tab_takes() {
    let mut h = chatting();
    h.type_str("/lo");
    let screen = render(&mut h, 110, 30);
    let tab_line = screen.lines().find(|l| l.contains("tab /log <path>")).unwrap_or_else(|| panic!("{screen}"));
    assert!(tab_line.contains("save this chat"));
    assert!(screen.contains("/logs"));
}

#[test]
fn history_popup() {
    use crate::history::{Outcome, Record, SkipRule};
    let mut h = app();
    let info = |species: &str| crate::protocol::PartnerInfo {
        gender: "Female".into(),
        species: species.into(),
        kinks: "Musk".into(),
        role: "Dominant".into(),
        language: None,
    };
    let at = fixed_time();
    h.history.push(Record {
        secs: 840,
        sent: 12,
        received: 15,
        shared_kinks: 3,
        ..Record::new(at, &info("Fox"), Outcome::TheyLeft)
    });
    h.history.push(Record { secs: 95, sent: 1, received: 0, ..Record::new(at, &info("Wolf"), Outcome::YouLeft) });
    h.history.push(Record { skip: Some(SkipRule::Language), ..Record::new(at, &info("Cat"), Outcome::Skipped) });
    h.run_command(crate::commands::Command::History);
    insta::assert_snapshot!(render(&mut h, 100, 20));
}

#[test]
fn misspelled_words_are_underlined() {
    let mut h = chatting();
    h.load_speller();
    h.type_str("helo there ");
    let error = h.theme.error;
    let backend = render_backend(&mut h, 110, 30);
    let buf = backend.buffer();
    let underlined: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .filter_map(|(x, y)| {
            let cell = &buf[(x, y)];
            let marked = cell.modifier.contains(ratatui::style::Modifier::UNDERLINED) && cell.fg == error;
            marked.then(|| cell.symbol().to_owned())
        })
        .collect();
    assert_eq!(underlined, "helo");
}

#[test]
fn image_viewer() {
    let mut h = chatting();
    h.toasts.clear();
    let url = "https://i.imgur.com/ref.png".to_string();
    h.server(ServerMessage::ReceiveMessage(format!("my ref {url} and https://random.host/b.jpg")));
    h.chat.entries.iter_mut().for_each(|e| e.at = fixed_time());
    let img: image::DynamicImage = image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 80, 40, 255])).into();
    let loaded =
        crate::images::prepare(&h.picker.clone(), img, vec![0; 2048], ratatui::layout::Size::new(4, 2)).unwrap();
    h.on_image(url.clone(), Ok(loaded));
    h.preview(&url);
    insta::assert_snapshot!("image_viewer_loaded", render(&mut h, 90, 24));
    h.press(KeyCode::Right);
    insta::assert_snapshot!("image_viewer_untrusted", render(&mut h, 90, 24));
}

#[test]
fn viewer_centres_the_picture() {
    let mut h = chatting();
    let url = "https://i.imgur.com/ref.png".to_string();
    h.server(ServerMessage::ReceiveMessage(url.clone()));
    let img: image::DynamicImage = image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 80, 40, 255])).into();
    let loaded = crate::images::prepare(&h.picker.clone(), img, vec![0; 16], ratatui::layout::Size::new(4, 2)).unwrap();
    h.on_image(url.clone(), Ok(loaded));
    h.preview(&url);
    let backend = render_backend(&mut h, 90, 24);
    let buf = backend.buffer();
    let painted: Vec<(u16, u16)> = (0..24)
        .flat_map(|y| (0..90).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let c = &buf[(x, y)];
            [c.fg, c.bg].contains(&ratatui::style::Color::Rgb(200, 80, 40))
        })
        .collect();
    assert_eq!(painted.len(), 4, "a 40×20 px image is 4×1 cells at 10×20 px per cell");
    let (x, y) = painted[0];
    assert!((41..=45).contains(&x) && (9..=12).contains(&y), "centred, not in the corner: {painted:?}");
}

#[test]
fn link_hint_letters_sit_before_links() {
    let mut h = chatting();
    render(&mut h, 110, 30);
    h.start_link_hints();
    let screen = render(&mut h, 110, 30);
    assert!(screen.contains(" a https://e621.net/posts/123"), "{screen}");
}

#[test]
fn split_view() {
    let mut h = chatting();
    h.toasts.clear();
    h.new_session();
    h.toasts.clear();
    h.toggle_split();
    h.toasts.clear();
    insta::assert_snapshot!(render(&mut h, 120, 20));
}

#[test]
fn stats_show_when_to_look() {
    let mut h = app();
    for hour in [20, 21, 22] {
        for _ in 0..3 {
            h.activity.searched(chrono::Local::now().with_hour(hour).unwrap(), 30);
        }
    }
    h.activity.online(chrono::Local::now(), 120);
    h.run_command(crate::commands::Command::Stats);
    let screen = render(&mut h, 90, 26);
    assert!(screen.contains("online, by hour"), "{screen}");
    assert!(screen.contains("quickest matches"), "{screen}");
    assert!(screen.contains("20:00 (30s)"), "{screen}");
}

#[test]
fn long_pauses_and_new_days_get_a_line() {
    let mut h = chatting();
    let now = chrono::Local::now();
    let n = h.chat.entries.len();
    for (i, e) in h.chat.entries.iter_mut().enumerate() {
        e.at = now - chrono::TimeDelta::minutes(30 * (n - i) as i64);
    }
    h.chat.entries[0].at = now - chrono::TimeDelta::days(1) - chrono::TimeDelta::hours(1);
    let screen = render(&mut h, 110, 40);
    assert!(screen.contains(" Today "), "{screen}");
    let time = h.chat.entries[n - 1].at.format("%H:%M").to_string();
    assert!(screen.contains(&format!(" {time} ")), "{screen}");
}

#[test]
fn popups_fill_only_inside_their_border() {
    use crate::config::PopupStyle;
    use ratatui::style::Color;
    let confirm = |h: &mut Harness| {
        h.modal = Some(Modal::Confirm { text: "Sure?".into(), action: Confirm::Block });
        let surface = h.theme.surface;
        let backend = render_backend(h, 80, 20);
        let buf = backend.buffer().clone();
        // The popup's top-left corner: the first cell drawn with a box/block glyph.
        let (x, y) = (0..20)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .find(|&(x, y)| matches!(buf[(x, y)].symbol(), "╭" | "▗"))
            .expect("a popup corner");
        (buf[(x, y)].clone(), buf[(x + 1, y + 1)].clone(), surface)
    };
    let mut h = app();
    let (corner, inside, surface) = confirm(&mut h);
    assert_eq!(corner.symbol(), "╭");
    assert_ne!(corner.bg, surface, "no fill on the border, so nothing pokes out past the line");
    assert_eq!(corner.bg, h.theme.bg, "it blends with the page instead");
    assert_eq!(inside.bg, surface);

    h.config.settings.popup_style = PopupStyle::Solid;
    let (corner, inside, surface) = confirm(&mut h);
    assert_eq!((corner.symbol(), corner.fg), ("▗", surface), "half blocks finish the card's edge");
    assert_eq!(inside.bg, surface);

    h.config.settings.popup_style = PopupStyle::Clear;
    let (_, inside, surface) = confirm(&mut h);
    assert_ne!(inside.bg, surface);
    assert_eq!(inside.bg, h.theme.bg, "see-through to the page");
    h.config.settings.transparent_background = true;
    h.refresh_theme();
    let (corner, inside, _) = confirm(&mut h);
    assert_eq!((corner.bg, inside.bg), (Color::Reset, Color::Reset), "and to the terminal when that's transparent");
}
