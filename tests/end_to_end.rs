//! Two complete clients (event loop, app state, rendering) chatting through the mock
//! server, driven by scripted keystrokes.

mod support;

use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui_image::picker::Picker;
use std::time::Duration;
use support::Mock;
use tokio::sync::mpsc;
use tokio_stream_shim::UnboundedReceiverStream;
use yap::app::App;
use yap::app::chat::EntryKind;
use yap::config::{Config, Paths};
use yap::drawer::Drawer;
use yap::prefs::Field;

/// Tiny local stand-in for `tokio_stream::wrappers::UnboundedReceiverStream`.
mod tokio_stream_shim {
    use futures_util::Stream;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::mpsc;

    pub struct UnboundedReceiverStream<T>(pub mpsc::UnboundedReceiver<T>);

    impl<T> Stream for UnboundedReceiverStream<T> {
        type Item = T;
        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
            self.0.poll_recv(cx)
        }
    }
}

struct Client {
    keys: mpsc::UnboundedSender<Event>,
    task: tokio::task::JoinHandle<App>,
    _dir: tempfile::TempDir,
}

impl Client {
    fn start(server: &str, gender: &str, species: &str) -> Client {
        Client::start_with(server, gender, species, |_| {})
    }

    fn start_with(server: &str, gender: &str, species: &str, configure: impl FnOnce(&mut Config)) -> Client {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        configure(&mut config);
        let p = &mut config.active_mut().preferences;
        p.toggle(Field::Gender, gender);
        p.toggle(Field::Species, species);
        p.toggle(Field::Role, "Switch");
        p.toggle(Field::PartnerRole, "Switch");
        let mut app = App::new(
            Paths::in_dir(dir.path()),
            config,
            Drawer::default(),
            yap::theme::builtins(),
            Picker::halfblocks(),
        );
        app.server_override = Some(server.to_owned());

        let (keys, rx) = mpsc::unbounded_channel();
        let events = UnboundedReceiverStream(rx).map(Ok);
        let task = tokio::spawn(async move {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            let mut events = Some(events);
            let make_events = move || events.take().expect("headless runs never recreate the event stream");
            let options = yap::runtime::Options { raw_output: false, mouse: false };
            yap::runtime::run_with(&mut terminal, app, make_events, options).await.unwrap()
        });
        Client { keys, task, _dir: dir }
    }

    fn key(&self, code: KeyCode) {
        self.keys.send(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))).unwrap();
    }

    fn alt(&self, c: char) {
        self.keys.send(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT))).unwrap();
    }

    fn ctrl_key(&self, code: KeyCode) {
        self.keys.send(Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))).unwrap();
    }

    fn ctrl(&self, c: char) {
        self.keys.send(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))).unwrap();
    }

    fn type_line(&self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c));
        }
        self.key(KeyCode::Enter);
    }

    /// Wait for the client to quit. The temp dir is returned so saved files survive
    /// long enough to be checked.
    async fn finish(self) -> (App, tempfile::TempDir) {
        let app = tokio::time::timeout(Duration::from_secs(5), self.task).await.expect("client didn't quit").unwrap();
        (app, self._dir)
    }
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

fn has_entry(app: &App, pred: impl Fn(&EntryKind) -> bool) -> bool {
    app.chat.entries.iter().any(|e| pred(&e.kind))
}

#[tokio::test]
async fn two_clients_chat_end_to_end() {
    let mock = Mock::start().await;
    let wolf = Client::start(&mock.url(), "Male", "Wolf");
    let fox = Client::start(&mock.url(), "Female", "Fox");
    mock.wait_for("both connected", |s| s.connected() == 2).await;
    settle().await;

    wolf.ctrl('f');
    mock.wait_for("wolf searching", |s| s.searching() == 1).await;
    fox.ctrl('f');
    mock.wait_for("paired", |s| s.paired()).await;
    // The server has paired them; give both event loops a moment to process it.
    settle().await;

    wolf.type_line("hello from the terminal");
    mock.wait_for("message relayed", |s| !s.frames_of_type("send_message").is_empty()).await;
    fox.type_line("/save https://e621.net/posts/1 their ref");
    fox.type_line("hi wolf!");
    mock.wait_for("reply relayed", |s| s.frames_of_type("send_message").len() == 2).await;

    // The fox leaves (confirming), then quits; the wolf sees them go.
    fox.ctrl('d');
    fox.key(KeyCode::Char('y'));
    mock.wait_for("fox left", |s| !s.frames_of_type("disconnect").is_empty()).await;
    fox.ctrl('q');
    let (fox, _fox_dir) = fox.finish().await;
    mock.wait_for("fox gone", |s| s.connected() == 1).await;
    settle().await;
    wolf.ctrl('q');
    let (wolf, _) = wolf.finish().await;

    assert!(has_entry(&fox, |k| *k == EntryKind::Partner("hello from the terminal".into())));
    assert!(has_entry(&fox, |k| *k == EntryKind::You("hi wolf!".into())));
    assert!(has_entry(&fox, |k| matches!(k, EntryKind::PartnerInfo { info, .. } if info.species == "Wolf")));
    assert!(has_entry(&fox, |k| *k == EntryKind::System("You have disconnected from your partner.".into())));
    assert_eq!(fox.drawer.items[0].label, "their ref");
    assert!(fox.paths.drawer_file.exists(), "drawer saved on the way out");

    assert!(has_entry(&wolf, |k| *k == EntryKind::Partner("hi wolf!".into())));
    assert!(has_entry(&wolf, |k| *k == EntryKind::System("Your yiffing partner has left.".into())));
    assert!(wolf.traffic.iter().any(|e| e.body.contains("send_message")));

    // The typing indicator was switched on and back off around each message.
    let typing = mock.state.lock().unwrap().frames_of_type("typing");
    assert!(typing.iter().any(|(_, v)| v["data"] == true));
    assert!(typing.iter().any(|(_, v)| v["data"] == false));
}

#[tokio::test]
async fn stop_searching_reconnects_and_leaves_queue() {
    let mock = Mock::start().await;
    let wolf = Client::start(&mock.url(), "Male", "Wolf");
    mock.wait_for("connected", |s| s.connected() == 1).await;
    // The server sees the socket before the client has processed its handshake.
    settle().await;
    wolf.ctrl('f');
    mock.wait_for("searching", |s| s.searching() == 1).await;
    wolf.ctrl('d');
    // The old socket closes (dropping us from the queue) and a fresh one opens.
    mock.wait_for("requeued as a fresh idle client", |s| s.searching() == 0 && s.connected() == 1).await;
    wolf.ctrl('q');
    let (wolf, _) = wolf.finish().await;
    assert!(has_entry(&wolf, |k| *k == EntryKind::System("Stopped searching.".into())));
}

#[tokio::test]
async fn reconnects_after_server_drops_us() {
    let mock = Mock::start().await;
    let wolf = Client::start(&mock.url(), "Male", "Wolf");
    mock.wait_for("connected", |s| s.connected() == 1).await;
    let id = mock.state.lock().unwrap().ids()[0];
    mock.state.lock().unwrap().kick(id);
    // Backoff starts at one second.
    let reconnected = |s: &support::State| s.connected() == 1 && s.ids()[0] != id;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while !reconnected(&mock.state.lock().unwrap()) {
        assert!(tokio::time::Instant::now() < deadline, "did not reconnect");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    wolf.ctrl('q');
    let (wolf, _) = wolf.finish().await;
    assert!(has_entry(&wolf, |k| matches!(k, EntryKind::Warning(t) if t.contains("Reconnecting"))));
}

#[tokio::test]
async fn one_client_two_chats_two_partners() {
    let mock = Mock::start().await;
    let wolf = Client::start(&mock.url(), "Male", "Wolf");
    let fox = Client::start(&mock.url(), "Female", "Fox");
    let cat = Client::start(&mock.url(), "Male", "Cat");
    mock.wait_for("three connected", |s| s.connected() == 3).await;
    wolf.alt('n');
    mock.wait_for("wolf's second socket", |s| s.connected() == 4).await;
    settle().await;

    // Chat 2 (on screen after alt+n) pairs with the fox.
    wolf.ctrl('f');
    mock.wait_for("wolf searching", |s| s.searching() == 1).await;
    fox.ctrl('f');
    mock.wait_for("paired once", |s| s.searching() == 0).await;
    settle().await;

    // Back to chat 1, which pairs with the cat.
    wolf.ctrl_key(KeyCode::PageUp);
    wolf.ctrl('f');
    mock.wait_for("wolf searching again", |s| s.searching() == 1).await;
    cat.ctrl('f');
    mock.wait_for("paired twice", |s| s.searching() == 0).await;
    settle().await;

    wolf.type_line("hi cat");
    wolf.ctrl_key(KeyCode::PageDown);
    wolf.type_line("hi fox");
    mock.wait_for("both messages", |s| s.frames_of_type("send_message").len() == 2).await;
    settle().await;

    wolf.ctrl('q');
    wolf.key(KeyCode::Char('y'));
    let (wolf, _) = wolf.finish().await;
    settle().await;
    fox.ctrl('q');
    cat.ctrl('q');
    let (fox, _) = fox.finish().await;
    let (cat, _) = cat.finish().await;

    assert!(has_entry(&fox, |k| *k == EntryKind::Partner("hi fox".into())));
    assert!(!has_entry(&fox, |k| *k == EntryKind::Partner("hi cat".into())));
    assert!(has_entry(&cat, |k| *k == EntryKind::Partner("hi cat".into())));
    assert!(!has_entry(&cat, |k| *k == EntryKind::Partner("hi fox".into())));
    assert_eq!(wolf.session_count(), 2);
    assert_eq!(wolf.logs.items.len(), 2, "one log per partner");
}

#[tokio::test]
async fn composing_in_an_external_editor() {
    let mock = Mock::start().await;
    // A stand-in "editor" that writes two paragraphs into the file it's given.
    let editor = r#"printf 'first line\nsame paragraph\n\nsecond paragraph\n' >"#;
    let wolf = Client::start_with(&mock.url(), "Male", "Wolf", |c| c.settings.editor = editor.into());
    let fox = Client::start(&mock.url(), "Female", "Fox");
    mock.wait_for("both connected", |s| s.connected() == 2).await;
    settle().await;
    wolf.ctrl('f');
    mock.wait_for("searching", |s| s.searching() == 1).await;
    fox.ctrl('f');
    mock.wait_for("paired", |s| s.paired()).await;
    settle().await;

    wolf.ctrl('x');
    settle().await;
    wolf.key(KeyCode::Enter);
    mock.wait_for("message sent", |s| !s.frames_of_type("send_message").is_empty()).await;
    let sent = mock.state.lock().unwrap().frames_of_type("send_message")[0].1["data"].clone();
    assert_eq!(sent, "first line same paragraph / second paragraph");

    wolf.ctrl('q');
    wolf.key(KeyCode::Char('y'));
    wolf.finish().await;
    // Let the fox hear that the wolf left, or quitting would ask for confirmation.
    settle().await;
    fox.ctrl('q');
    fox.finish().await;
}
