//! The websocket connection task.
//!
//! One task per connection: it owns the socket, sends the application-level heartbeat,
//! and reports every frame (in both directions) for the traffic viewer.

use crate::protocol::{ClientMessage, ServerMessage};
use crate::traffic::{Direction, FrameKind, hex_preview};
use chrono::{DateTime, Local};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use url::Url;

/// The web client pings every 10 s; the server kills sockets silent for 30-60 s.
pub const HEARTBEAT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub struct TrafficRecord {
    pub at: DateTime<Local>,
    pub dir: Direction,
    pub kind: FrameKind,
    pub size: usize,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    /// The websocket handshake completed.
    Open,
    Message(ServerMessage),
    /// A text frame we couldn't make sense of.
    Unparsed {
        raw: String,
        error: String,
    },
    /// The connection is gone; no more events follow.
    Closed {
        reason: String,
    },
    Traffic(TrafficRecord),
}

#[derive(Debug)]
pub enum NetCommand {
    Send(ClientMessage),
    /// Send a text frame verbatim (debug console).
    SendRaw(String),
    Close,
}

#[derive(Debug, Clone)]
pub struct NetConfig {
    pub url: Url,
    pub heartbeat: Duration,
    pub user_agent: String,
}

impl NetConfig {
    pub fn new(url: Url) -> Self {
        NetConfig { url, heartbeat: HEARTBEAT, user_agent: format!("yap/{}", env!("CARGO_PKG_VERSION")) }
    }
}

/// Accept what people paste: `wss://`, `ws://`, or the site's `https://` address.
pub fn websocket_url(input: &str) -> Result<Url, String> {
    let mut url = Url::parse(input.trim()).map_err(|e| format!("invalid server URL: {e}"))?;
    let scheme = match url.scheme() {
        "wss" | "https" => "wss",
        "ws" | "http" => "ws",
        other => return Err(format!("unsupported scheme `{other}` (use wss://)")),
    };
    url.set_scheme(scheme).map_err(|()| "invalid server URL".to_owned())?;
    if url.host_str().is_none() {
        return Err("server URL has no host".into());
    }
    Ok(url)
}

/// A running connection. Dropping it aborts the task.
#[derive(Debug)]
pub struct NetHandle {
    tx: mpsc::UnboundedSender<NetCommand>,
    task: Option<JoinHandle<()>>,
}

impl NetHandle {
    pub fn send(&self, cmd: NetCommand) -> bool {
        self.tx.send(cmd).is_ok()
    }

    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Ask for a clean close (so the server drops us from its queue promptly) and let
    /// the task finish on its own.
    pub fn close(mut self) {
        let _ = self.tx.send(NetCommand::Close);
        drop(self.task.take());
    }

    /// Wait for the task to end, e.g. after `send(NetCommand::Close)` at shutdown.
    pub async fn join(mut self) {
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for NetHandle {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

pub fn spawn(config: NetConfig, events: mpsc::UnboundedSender<NetEvent>) -> NetHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let reason = run(config, rx, &events).await;
        let _ = events.send(NetEvent::Closed { reason });
    });
    NetHandle { tx, task: Some(task) }
}

struct Reporter<'a>(&'a mpsc::UnboundedSender<NetEvent>);

impl Reporter<'_> {
    fn traffic(&self, dir: Direction, kind: FrameKind, size: usize, body: impl Into<String>) {
        let _ = self.0.send(NetEvent::Traffic(TrafficRecord { at: Local::now(), dir, kind, size, body: body.into() }));
    }

    fn meta(&self, kind: FrameKind, body: impl Into<String>) {
        self.traffic(Direction::Meta, kind, 0, body);
    }

    fn event(&self, ev: NetEvent) {
        let _ = self.0.send(ev);
    }
}

fn describe_close(frame: Option<&CloseFrame>) -> String {
    match frame {
        Some(f) if f.reason.is_empty() => format!("code {}", u16::from(f.code)),
        Some(f) => format!("code {}: {}", u16::from(f.code), f.reason),
        None => "no close frame".into(),
    }
}

/// Runs until the connection ends; returns why.
async fn run(
    config: NetConfig,
    mut commands: mpsc::UnboundedReceiver<NetCommand>,
    events: &mpsc::UnboundedSender<NetEvent>,
) -> String {
    let report = Reporter(events);
    report.meta(FrameKind::Info, format!("connecting to {}", config.url));

    let mut request = match config.url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            report.meta(FrameKind::Error, e.to_string());
            return e.to_string();
        }
    };
    if let Ok(ua) = HeaderValue::from_str(&config.user_agent) {
        request.headers_mut().insert("User-Agent", ua);
    }

    let (ws, response) = match tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request)).await {
        Ok(Ok(ok)) => ok,
        Ok(Err(e)) => {
            let msg = format!("connection failed: {e}");
            report.meta(FrameKind::Error, &msg);
            return msg;
        }
        Err(_) => {
            report.meta(FrameKind::Error, "connection timed out");
            return "connection timed out".into();
        }
    };

    let headers: Vec<String> =
        response.headers().iter().map(|(k, v)| format!("{k}: {}", v.to_str().unwrap_or("<binary>"))).collect();
    report.meta(FrameKind::Info, format!("handshake {}\n{}", response.status(), headers.join("\n")));
    report.event(NetEvent::Open);

    let (mut sink, mut stream) = ws.split();
    let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + config.heartbeat, config.heartbeat);

    macro_rules! send_text {
        ($text:expr) => {{
            let text: String = $text;
            report.traffic(Direction::Out, FrameKind::Text, text.len(), text.clone());
            if let Err(e) = sink.send(Message::Text(Utf8Bytes::from(text))).await {
                let msg = format!("send failed: {e}");
                report.meta(FrameKind::Error, &msg);
                return msg;
            }
        }};
    }

    loop {
        tokio::select! {
            frame = stream.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    let text = text.as_str();
                    report.traffic(Direction::In, FrameKind::Text, text.len(), text);
                    match ServerMessage::parse(text) {
                        Ok(msg) => report.event(NetEvent::Message(msg)),
                        Err(e) => report.event(NetEvent::Unparsed { raw: text.to_owned(), error: e.to_string() }),
                    }
                }
                Some(Ok(Message::Binary(bytes))) => {
                    report.traffic(Direction::In, FrameKind::Binary, bytes.len(), hex_preview(&bytes));
                }
                Some(Ok(Message::Ping(payload))) => {
                    report.traffic(Direction::In, FrameKind::Ping, payload.len(), hex_preview(&payload));
                    // tungstenite queues the pong itself; record it for completeness.
                    report.traffic(Direction::Out, FrameKind::Pong, payload.len(), "(automatic reply)");
                }
                Some(Ok(Message::Pong(payload))) => {
                    report.traffic(Direction::In, FrameKind::Pong, payload.len(), hex_preview(&payload));
                }
                Some(Ok(Message::Close(frame))) => {
                    let why = describe_close(frame.as_ref());
                    report.traffic(Direction::In, FrameKind::Close, 0, &why);
                    return format!("server closed the connection ({why})");
                }
                Some(Ok(Message::Frame(_))) => {}
                Some(Err(e)) => {
                    let msg = format!("connection error: {e}");
                    report.meta(FrameKind::Error, &msg);
                    return msg;
                }
                None => return "connection ended".into(),
            },
            cmd = commands.recv() => match cmd {
                Some(NetCommand::Send(msg)) => send_text!(msg.to_json()),
                Some(NetCommand::SendRaw(text)) => send_text!(text),
                Some(NetCommand::Close) | None => {
                    report.traffic(Direction::Out, FrameKind::Close, 0, "code 1000");
                    let _ = sink.send(Message::Close(Some(CloseFrame { code: CloseCode::Normal, reason: "".into() }))).await;
                    return "disconnected".into();
                }
            },
            _ = heartbeat.tick() => send_text!(ClientMessage::Ping.to_json()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_url_accepts_site_addresses() {
        assert_eq!(websocket_url("https://www.yiffspot.com/").unwrap().as_str(), "wss://www.yiffspot.com/");
        assert_eq!(websocket_url("http://localhost:8000").unwrap().as_str(), "ws://localhost:8000/");
        assert_eq!(websocket_url(" wss://x.y/ ").unwrap().as_str(), "wss://x.y/");
        assert!(websocket_url("ftp://x.y").is_err());
        assert!(websocket_url("nonsense").is_err());
    }
}
