//! The connection task against a mock server.

mod support;

use std::time::Duration;
use support::{Mock, prefs};
use tokio::sync::mpsc;
use yap::net::{self, NetCommand, NetConfig, NetEvent, NetHandle};
use yap::protocol::{ClientMessage, ServerMessage};
use yap::traffic::{Direction, FrameKind};

struct Conn {
    handle: NetHandle,
    rx: mpsc::UnboundedReceiver<NetEvent>,
    log: Vec<NetEvent>,
}

impl Conn {
    fn open(url: &str, heartbeat: Duration) -> Conn {
        let mut config = NetConfig::new(net::websocket_url(url).unwrap());
        config.heartbeat = heartbeat;
        let (tx, rx) = mpsc::unbounded_channel();
        Conn { handle: net::spawn(config, tx), rx, log: Vec::new() }
    }

    fn send(&self, msg: ClientMessage) {
        assert!(self.handle.send(NetCommand::Send(msg)));
    }

    /// Wait for a protocol message matching `pred`, recording everything seen.
    async fn expect(&mut self, what: &str, pred: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
        let deadline = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                ev = self.rx.recv() => {
                    let ev = ev.unwrap_or_else(|| panic!("channel closed waiting for {what}"));
                    self.log.push(ev.clone());
                    if let NetEvent::Message(m) = ev
                        && pred(&m)
                    {
                        return m;
                    }
                }
                _ = &mut deadline => panic!("timed out waiting for {what}; saw {:#?}", self.log),
            }
        }
    }

    async fn closed(&mut self) -> String {
        let deadline = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                ev = self.rx.recv() => match ev {
                    Some(NetEvent::Closed { reason }) => return reason,
                    Some(ev) => self.log.push(ev),
                    None => panic!("channel closed without a Closed event"),
                },
                _ = &mut deadline => panic!("timed out waiting for close"),
            }
        }
    }
}

const SLOW: Duration = Duration::from_secs(3600);

#[tokio::test]
async fn two_clients_pair_chat_and_part() {
    let mock = Mock::start().await;
    let mut a = Conn::open(&mock.url(), SLOW);
    let mut b = Conn::open(&mock.url(), SLOW);
    a.expect("a connected", |m| matches!(m, ServerMessage::ConnectionSuccess { .. })).await;
    b.expect("b connected", |m| matches!(m, ServerMessage::ConnectionSuccess { .. })).await;
    a.expect("count 2", |m| *m == ServerMessage::UserCount(2)).await;

    a.send(ClientMessage::FindPartner(prefs("Male", "Wolf")));
    a.expect("pending", |m| *m == ServerMessage::PartnerPending).await;
    b.send(ClientMessage::FindPartner(prefs("Female", "Fox")));

    let ServerMessage::PartnerConnected(for_b) =
        b.expect("b matched", |m| matches!(m, ServerMessage::PartnerConnected(_))).await
    else {
        unreachable!()
    };
    let ServerMessage::PartnerConnected(for_a) =
        a.expect("a matched", |m| matches!(m, ServerMessage::PartnerConnected(_))).await
    else {
        unreachable!()
    };
    assert_eq!(for_b.species, "Wolf");
    assert_eq!(for_b.language.as_deref(), Some("any"), "the searcher gets the language");
    assert_eq!(for_a.species, "Fox");
    assert_eq!(for_a.language, None);
    assert_eq!(for_a.kink_list(), vec!["Biting"]);

    a.send(ClientMessage::Typing(true));
    b.expect("typing", |m| *m == ServerMessage::PartnerTyping(true)).await;
    a.send(ClientMessage::SendMessage("hello 🦊 <b>".into()));
    b.expect("message", |m| *m == ServerMessage::ReceiveMessage("hello 🦊 <b>".into())).await;

    b.send(ClientMessage::Disconnect);
    b.expect("ack", |m| *m == ServerMessage::ClientDisconnect).await;
    a.expect("left", |m| *m == ServerMessage::PartnerLeft).await;
}

#[tokio::test]
async fn partner_dropping_is_reported() {
    let mock = Mock::start().await;
    let mut a = Conn::open(&mock.url(), SLOW);
    let mut b = Conn::open(&mock.url(), SLOW);
    a.send(ClientMessage::FindPartner(prefs("Male", "Wolf")));
    mock.wait_for("a searching", |s| s.searching() == 1).await;
    b.send(ClientMessage::FindPartner(prefs("Male", "Cat")));
    a.expect("matched", |m| matches!(m, ServerMessage::PartnerConnected(_))).await;

    b.handle.send(NetCommand::Close);
    assert_eq!(b.closed().await, "disconnected");
    a.expect("partner gone", |m| *m == ServerMessage::PartnerDisconnected).await;
    a.expect("count 1", |m| *m == ServerMessage::UserCount(1)).await;
}

#[tokio::test]
async fn blocked_partners_are_not_rematched() {
    let mock = Mock::start().await;
    let mut a = Conn::open(&mock.url(), SLOW);
    let mut b = Conn::open(&mock.url(), SLOW);
    a.send(ClientMessage::FindPartner(prefs("Male", "Wolf")));
    mock.wait_for("a searching", |s| s.searching() == 1).await;
    b.send(ClientMessage::FindPartner(prefs("Male", "Cat")));
    a.expect("matched", |m| matches!(m, ServerMessage::PartnerConnected(_))).await;

    a.send(ClientMessage::BlockPartner);
    a.expect("blocked", |m| *m == ServerMessage::PartnerBlocked).await;
    b.expect("left", |m| *m == ServerMessage::PartnerLeft).await;

    a.send(ClientMessage::FindPartner(prefs("Male", "Wolf")));
    b.send(ClientMessage::FindPartner(prefs("Male", "Cat")));
    mock.wait_for("both searching", |s| s.searching() == 2).await;
    assert!(!mock.state.lock().unwrap().paired());
}

#[tokio::test]
async fn heartbeat_keeps_pinging() {
    let mock = Mock::start().await;
    let _a = Conn::open(&mock.url(), Duration::from_millis(40));
    mock.wait_for("three pings", |s| s.frames_of_type("ping").len() >= 3).await;
    let pings = mock.state.lock().unwrap().frames_of_type("ping");
    assert!(pings.iter().all(|(_, v)| v["data"] == true));
}

#[tokio::test]
async fn traffic_captures_both_directions_and_handshake() {
    let mock = Mock::start().await;
    let mut a = Conn::open(&mock.url(), SLOW);
    a.expect("connected", |m| matches!(m, ServerMessage::ConnectionSuccess { .. })).await;
    a.send(ClientMessage::Typing(false));
    mock.wait_for("typing frame", |s| !s.frames_of_type("typing").is_empty()).await;
    a.handle.send(NetCommand::Close);
    a.closed().await;

    let traffic: Vec<_> = a
        .log
        .iter()
        .filter_map(|e| match e {
            NetEvent::Traffic(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert!(traffic.iter().any(|t| t.dir == Direction::Meta && t.body.starts_with("handshake 101")));
    assert!(traffic.iter().any(|t| t.dir == Direction::In && t.body.contains("connection_success")));
    let out = traffic.iter().find(|t| t.dir == Direction::Out && t.kind == FrameKind::Text).unwrap();
    assert_eq!(out.body, r#"{"type":"typing","data":false}"#);
    assert_eq!(out.size, out.body.len());
    assert!(traffic.iter().any(|t| t.dir == Direction::Out && t.kind == FrameKind::Close));
}

#[tokio::test]
async fn raw_frames_are_sent_verbatim() {
    let mock = Mock::start().await;
    let mut a = Conn::open(&mock.url(), SLOW);
    a.expect("connected", |m| matches!(m, ServerMessage::ConnectionSuccess { .. })).await;
    a.handle.send(NetCommand::SendRaw(r#"{"type":"custom","data":[1,2]}"#.into()));
    mock.wait_for("custom frame", |s| !s.frames_of_type("custom").is_empty()).await;
}

#[tokio::test]
async fn server_side_close_is_reported() {
    let mock = Mock::start().await;
    let mut a = Conn::open(&mock.url(), SLOW);
    a.expect("connected", |m| matches!(m, ServerMessage::ConnectionSuccess { .. })).await;
    let id = mock.state.lock().unwrap().ids()[0];
    mock.state.lock().unwrap().kick(id);
    let reason = a.closed().await;
    assert!(reason.contains("server closed"), "{reason}");
}

#[tokio::test]
async fn unreachable_server_fails_fast() {
    // Bind then drop to get a port nothing is listening on.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut a = Conn::open(&format!("ws://127.0.0.1:{port}/"), SLOW);
    let reason = a.closed().await;
    assert!(reason.starts_with("connection failed"), "{reason}");
}
