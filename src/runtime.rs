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
use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

type ImageResult = (String, Result<Loaded, String>);
/// A network event tagged with its session and that session's connection generation.
type Tagged = (u64, u64, NetEvent);

/// How the loop talks to the real terminal.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Write escape sequences (title, clipboard, bell, notifications) to stdout and
    /// suspend the terminal for the external editor. Off in headless tests.
    pub raw_output: bool,
    /// Re-enable mouse capture after coming back from the editor.
    pub mouse: bool,
}

struct Connection {
    handle: NetHandle,
    generation: u64,
}

struct Runtime {
    /// One socket per chat session.
    connections: HashMap<u64, Connection>,
    generation: u64,
    net_tx: mpsc::UnboundedSender<Tagged>,
    img_tx: mpsc::UnboundedSender<ImageResult>,
    options: Options,
}

pub async fn run(terminal: &mut DefaultTerminal, app: App, mouse: bool) -> Result<()> {
    run_with(terminal, app, EventStream::new, Options { raw_output: true, mouse }).await.map(drop)
}

/// The loop itself, generic over the backend and the source of terminal events so it
/// can be driven headlessly in tests. `make_events` is called again after the external
/// editor returns, because the terminal event reader has to be dropped while it runs.
/// Returns the final app state.
pub async fn run_with<B, E>(
    terminal: &mut Terminal<B>,
    mut app: App,
    mut make_events: impl FnMut() -> E,
    options: Options,
) -> Result<App>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
    E: Stream<Item = std::io::Result<Event>> + Unpin,
{
    let (img_tx, mut img_rx) = mpsc::unbounded_channel();
    let (net_tx, mut net_rx) = mpsc::unbounded_channel::<Tagged>();
    let mut rt = Runtime { connections: HashMap::new(), generation: 0, net_tx, img_tx, options };
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut title = String::new();
    let mut events = make_events();

    app.connect();
    loop {
        app.now = Instant::now();
        for (session, effect) in app.take_tagged_effects() {
            if let Effect::OpenEditor { command, text, purpose } = effect {
                // The terminal belongs to the editor until it exits.
                if options.raw_output {
                    drop(events);
                    let result = with_suspended_terminal(terminal, options.mouse, || edit_text(&command, &text));
                    events = make_events();
                    app.on_editor(purpose, result);
                } else {
                    app.on_editor(purpose, edit_text(&command, &text));
                }
                continue;
            }
            rt.execute(&mut app, session, effect);
        }
        app.flush();
        if app.quit {
            break;
        }
        let new_title = app.window_title();
        if new_title != title && options.raw_output {
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
            Some(first) = net_rx.recv() => {
                rt.deliver(&mut app, first);
                // Drain whatever else is ready so bursts don't redraw per frame.
                for _ in 0..256 {
                    match net_rx.try_recv() {
                        Ok(more) => rt.deliver(&mut app, more),
                        Err(_) => break,
                    }
                }
            },
            Some((url, result)) = img_rx.recv() => app.on_image(url, result),
            _ = tick.tick() => {
                app.now = Instant::now();
                app.on_tick();
            }
        }
    }

    app.flush();
    let closing: Vec<NetHandle> = rt.connections.drain().map(|(_, c)| c.handle).collect();
    for handle in &closing {
        handle.send(NetCommand::Close);
    }
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        for handle in closing {
            handle.join().await;
        }
    })
    .await;
    Ok(app)
}

/// Hand the terminal to another program, then take it back.
fn with_suspended_terminal<B: Backend, T>(terminal: &mut Terminal<B>, mouse: bool, f: impl FnOnce() -> T) -> T {
    use ratatui::crossterm::event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
        EnableMouseCapture,
    };
    use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
    let mut out = std::io::stdout();
    let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste, DisableFocusChange, LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let result = f();
    let _ = enable_raw_mode();
    let _ = execute!(out, EnterAlternateScreen, EnableBracketedPaste, EnableFocusChange);
    if mouse {
        let _ = execute!(out, EnableMouseCapture);
    }
    let _ = terminal.clear();
    result
}

/// Open `text` in an editor and return what was saved. The temp file is private
/// (0600) and removed afterwards, since it may hold a private message.
pub fn edit_text(command: &str, text: &str) -> Result<String, String> {
    let file = tempfile::Builder::new().prefix("yap-").suffix(".txt").tempfile().map_err(|e| e.to_string())?;
    std::fs::write(file.path(), text).map_err(|e| e.to_string())?;
    // Run through the shell so commands like `code --wait` work.
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{command} \"$1\""))
        .arg("yap-editor")
        .arg(file.path())
        .status()
        .map_err(|e| format!("couldn't start `{command}`: {e}"))?;
    if !status.success() {
        return Err(format!("`{command}` exited with {status}"));
    }
    std::fs::read_to_string(file.path()).map_err(|e| e.to_string())
}

impl Runtime {
    /// Pass a network event on, unless it's from a socket that has since been replaced.
    fn deliver(&mut self, app: &mut App, (session, generation, event): Tagged) {
        let current = self.connections.get(&session).is_some_and(|c| c.generation == generation);
        if current {
            if matches!(event, NetEvent::Closed { .. }) {
                self.connections.remove(&session);
            }
            app.on_net_for(session, event);
        }
    }

    fn execute(&mut self, app: &mut App, session: u64, effect: Effect) {
        match effect {
            Effect::Connect(url) => {
                if let Some(old) = self.connections.remove(&session) {
                    old.handle.close();
                }
                match net::websocket_url(&url) {
                    Ok(url) => {
                        self.generation += 1;
                        let generation = self.generation;
                        let (tx, mut rx) = mpsc::unbounded_channel();
                        let shared = self.net_tx.clone();
                        tokio::spawn(async move {
                            while let Some(event) = rx.recv().await {
                                if shared.send((session, generation, event)).is_err() {
                                    break;
                                }
                            }
                        });
                        let handle = net::spawn(NetConfig::new(url), tx);
                        self.connections.insert(session, Connection { handle, generation });
                    }
                    Err(e) => {
                        app.with_session(session, |app| app.connection_error(e));
                    }
                }
            }
            Effect::CloseSocket => {
                if let Some(old) = self.connections.remove(&session) {
                    old.handle.close();
                }
            }
            Effect::Send(msg) => self.send(app, session, NetCommand::Send(msg)),
            Effect::SendRaw(text) => self.send(app, session, NetCommand::SendRaw(text)),
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
            Effect::PlaySound(command) => {
                if self.options.raw_output {
                    let _ = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(command)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
            Effect::OpenEditor { .. } => unreachable!("handled by the loop"),
        }
    }

    fn write_raw(&self, seq: &str) {
        if self.options.raw_output {
            let mut out = std::io::stdout();
            let _ = out.write_all(seq.as_bytes());
            let _ = out.flush();
        }
    }

    fn send(&self, app: &mut App, session: u64, cmd: NetCommand) {
        if !self.connections.get(&session).is_some_and(|c| c.handle.send(cmd)) {
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
