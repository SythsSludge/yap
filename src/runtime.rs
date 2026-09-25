//! The event loop: terminal, network and image events in; effects out.

use crate::app::{App, Effect, Level};
use crate::images::{self, FetchPolicy, Loaded};
use crate::net::{self, NetCommand, NetConfig, NetEvent, NetHandle};
use anyhow::Result;
use base64::Engine;
use futures_util::{Stream, StreamExt};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{Event, EventStream};
use ratatui::crossterm::{execute, terminal::SetTitle};
use ratatui::layout::Size;
use ratatui::{DefaultTerminal, Terminal};
use std::io::Write;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

type ImageResult = (String, Result<Loaded, String>);

struct Runtime {
    net: Option<NetHandle>,
    /// Events from the *current* connection only: replacing the receiver on reconnect
    /// means stragglers from an old socket can never confuse the new session.
    net_rx: Option<mpsc::UnboundedReceiver<NetEvent>>,
    img_tx: mpsc::UnboundedSender<ImageResult>,
    raw_output: bool,
}

async fn recv_net(rx: &mut Option<mpsc::UnboundedReceiver<NetEvent>>) -> Option<NetEvent> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

pub async fn run(terminal: &mut DefaultTerminal, app: App) -> Result<()> {
    run_with(terminal, app, EventStream::new(), true).await.map(drop)
}

/// The loop itself, generic over the backend and the source of terminal events so it
/// can be driven headlessly in tests. `raw_output` controls whether effects that write
/// escape sequences (title, clipboard, bell) touch stdout. Returns the final app state.
pub async fn run_with<B, E>(terminal: &mut Terminal<B>, mut app: App, mut events: E, raw_output: bool) -> Result<App>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
    E: Stream<Item = std::io::Result<Event>> + Unpin,
{
    let (img_tx, mut img_rx) = mpsc::unbounded_channel();
    let mut rt = Runtime { net: None, net_rx: None, img_tx, raw_output };
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut title = String::new();

    app.connect();
    loop {
        app.now = Instant::now();
        for effect in app.take_effects() {
            rt.execute(&mut app, effect);
        }
        app.flush();
        if app.quit {
            break;
        }
        let new_title = app.window_title();
        if new_title != title && raw_output {
            let _ = execute!(std::io::stdout(), SetTitle(&new_title));
            title = new_title;
        }
        terminal.draw(|f| crate::ui::draw(f, &mut app))?;

        tokio::select! {
            event = events.next() => match event {
                Some(Ok(event)) => {
                    app.now = Instant::now();
                    app.on_terminal(event);
                }
                Some(Err(e)) => return Err(e.into()),
                None => break,
            },
            event = recv_net(&mut rt.net_rx) => match event {
                Some(event) => {
                    app.on_net(event);
                    // Drain whatever else is ready so bursts don't redraw per frame.
                    if let Some(rx) = rt.net_rx.as_mut() {
                        for _ in 0..256 {
                            match rx.try_recv() {
                                Ok(ev) => app.on_net(ev),
                                Err(_) => break,
                            }
                        }
                    }
                }
                None => rt.net_rx = None,
            },
            Some((url, result)) = img_rx.recv() => app.on_image(url, result),
            _ = tick.tick() => {
                app.now = Instant::now();
                app.on_tick();
            }
        }
    }

    app.flush();
    if let Some(handle) = rt.net.take() {
        handle.send(NetCommand::Close);
        let _ = tokio::time::timeout(Duration::from_millis(500), handle.join()).await;
    }
    Ok(app)
}

impl Runtime {
    fn execute(&mut self, app: &mut App, effect: Effect) {
        match effect {
            Effect::Connect(url) => {
                if let Some(old) = self.net.take() {
                    old.close();
                }
                self.net_rx = None;
                match net::websocket_url(&url) {
                    Ok(url) => {
                        let (tx, rx) = mpsc::unbounded_channel();
                        self.net = Some(net::spawn(NetConfig::new(url), tx));
                        self.net_rx = Some(rx);
                    }
                    Err(e) => app.connection_error(e),
                }
            }
            Effect::CloseSocket => {
                if let Some(old) = self.net.take() {
                    old.close();
                }
            }
            Effect::Send(msg) => self.send(app, NetCommand::Send(msg)),
            Effect::SendRaw(text) => self.send(app, NetCommand::SendRaw(text)),
            Effect::FetchImage { url, allow_host } => {
                let s = &app.config.settings.images;
                let policy = FetchPolicy {
                    trusted: s.trusted_domains.clone(),
                    https_only: s.https_only,
                    max_bytes: s.max_bytes,
                    allow_host,
                };
                let max = Size::new(s.max_cols, s.max_rows);
                let picker = app.picker.clone();
                let tx = self.img_tx.clone();
                tokio::spawn(async move {
                    let result = match url::Url::parse(&url) {
                        Ok(parsed) => images::load(parsed, policy, picker, max).await,
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = tx.send((url, result));
                });
            }
            Effect::OpenUrl(url) => {
                if let Err(e) = open::that_detached(&url) {
                    app.toast(Level::Error, format!("Couldn't open a browser: {e}"));
                }
            }
            Effect::Copy(text) => self.write_raw(&osc52(&text)),
            Effect::Notify { title, body } => self.write_raw(&notification(&title, &body, is_kitty())),
            Effect::Bell => self.write_raw("\x07"),
        }
    }

    fn write_raw(&self, seq: &str) {
        if self.raw_output {
            let mut out = std::io::stdout();
            let _ = out.write_all(seq.as_bytes());
            let _ = out.flush();
        }
    }

    fn send(&self, app: &mut App, cmd: NetCommand) {
        if !self.net.as_ref().is_some_and(|n| n.send(cmd)) {
            app.toast(Level::Error, "You're not connected to the server.");
        }
    }
}

fn is_kitty() -> bool {
    std::env::var_os("KITTY_WINDOW_ID").is_some() || std::env::var("TERM").is_ok_and(|t| t.contains("kitty"))
}

/// OSC 52: ask the terminal to put `text` on the clipboard. Works over SSH too.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text))
}

/// A desktop notification escape: OSC 99 for kitty, OSC 777 elsewhere. Text is
/// stripped of anything that could terminate or extend the sequence.
pub fn notification(title: &str, body: &str, kitty: bool) -> String {
    let clean = |s: &str| crate::text::sanitize(s).replace(['\n', ';', '\x1b', '\x07'], " ");
    let (title, body) = (clean(title), clean(body));
    if kitty {
        format!("\x1b]99;i=yap:d=0;{title}\x1b\\\x1b]99;i=yap:d=1:p=body;{body}\x1b\\")
    } else {
        format!("\x1b]777;notify;{title};{body}\x07")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_encodes_clipboard_text() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn notifications_cannot_be_escaped() {
        let seq = notification("yap", "evil\x1b]52;c;x\x07;more", false);
        assert_eq!(seq.matches('\x1b').count(), 1, "{seq:?}");
        assert_eq!(seq.matches('\x07').count(), 1);
        assert!(seq.ends_with("evil]52 c x more\x07"), "{seq:?}");
        let seq = notification("yap", "Partner Left", true);
        assert!(seq.starts_with("\x1b]99;i=yap:d=0;yap\x1b\\"));
        assert!(seq.ends_with("p=body;Partner Left\x1b\\"));
    }
}
