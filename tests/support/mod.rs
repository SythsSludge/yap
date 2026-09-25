//! An in-process stand-in for the YiffSpot server, following the behaviour of
//! `vendor/yiffspot/src/server/*.js` closely enough to exercise the client.
//! Matching is simplified: any two searching clients pair up.

#![allow(dead_code)]

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Default)]
struct Client {
    tx: Option<mpsc::UnboundedSender<Message>>,
    partner: Option<usize>,
    previous: Option<usize>,
    searching: bool,
    preferences: Option<Value>,
    blocks: Vec<usize>,
}

#[derive(Default)]
pub struct State {
    clients: HashMap<usize, Client>,
    next_id: usize,
    /// Every frame received, as (client id, parsed JSON).
    pub received: Vec<(usize, Value)>,
}

impl State {
    fn send(&self, id: usize, kind: &str, data: Value) {
        if let Some(tx) = self.clients.get(&id).and_then(|c| c.tx.as_ref()) {
            let _ = tx.send(Message::text(json!({ "type": kind, "data": data }).to_string()));
        }
    }

    fn broadcast_count(&self) {
        let n = self.clients.len();
        for id in self.clients.keys() {
            self.send(*id, "update_user_count", json!(n));
        }
    }

    fn unpair(&mut self, id: usize) -> Option<usize> {
        let partner = self.clients.get_mut(&id)?.partner.take()?;
        if let Some(p) = self.clients.get_mut(&partner) {
            p.partner = None;
            p.previous = Some(id);
        }
        self.clients.get_mut(&id)?.previous = Some(partner);
        Some(partner)
    }

    fn info(&self, id: usize, with_language: bool) -> Value {
        let prefs = &self.clients[&id].preferences.as_ref().expect("searching clients have prefs");
        let kinks: Vec<&str> = prefs["kinks"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
        let mut info = json!({
            "gender": prefs["user"]["gender"],
            "species": prefs["user"]["species"],
            "role": prefs["user"]["role"],
            "kinks": kinks.join(", "),
        });
        if with_language {
            info["language"] = prefs["user"]["language"].clone();
        }
        info
    }

    fn handle(&mut self, id: usize, frame: Value) {
        self.received.push((id, frame.clone()));
        let kind = frame["type"].as_str().unwrap_or_default();
        match kind {
            "find_partner" => {
                if let Some(old) = self.unpair(id) {
                    self.send(old, "partner_left", json!(true));
                }
                let c = self.clients.get_mut(&id).unwrap();
                c.preferences = Some(frame["data"].clone());
                c.searching = true;
                self.send(id, "partner_pending", json!(true));
                let blocked = |a: &Client, b: usize| a.blocks.contains(&b);
                let candidate = self
                    .clients
                    .iter()
                    .filter(|(other, c)| **other != id && c.searching && c.partner.is_none())
                    .find(|(other, c)| !blocked(c, id) && !blocked(&self.clients[&id], **other))
                    .map(|(other, _)| *other);
                if let Some(other) = candidate {
                    for (a, b) in [(id, other), (other, id)] {
                        let c = self.clients.get_mut(&a).unwrap();
                        c.partner = Some(b);
                        c.searching = false;
                    }
                    // Only the searcher learns the partner's language, like the real server.
                    self.send(id, "partner_connected", self.info(other, true));
                    self.send(other, "partner_connected", self.info(id, false));
                }
            }
            "send_message" => {
                if let Some(p) = self.clients[&id].partner {
                    self.send(p, "receive_message", frame["data"].clone());
                }
            }
            "typing" => {
                if let Some(p) = self.clients[&id].partner {
                    self.send(p, "partner_typing", frame["data"].clone());
                }
            }
            "disconnect" => {
                if let Some(p) = self.unpair(id) {
                    self.send(p, "partner_left", json!(true));
                    self.send(id, "client_disconnect", json!(true));
                }
            }
            "block_partner" => {
                let target = self.clients[&id].partner.or(self.clients[&id].previous);
                if let Some(p) = target.filter(|p| self.clients.contains_key(p)) {
                    self.unpair(id);
                    self.clients.get_mut(&id).unwrap().blocks.push(p);
                    self.send(p, "partner_left", json!(true));
                    self.send(id, "partner_blocked", json!(true));
                }
            }
            _ => {}
        }
    }

    fn disconnect(&mut self, id: usize) {
        if let Some(p) = self.clients.get(&id).and_then(|c| c.partner) {
            if let Some(pc) = self.clients.get_mut(&p) {
                pc.partner = None;
            }
            self.send(p, "partner_disconnected", json!(true));
        }
        self.clients.remove(&id);
        self.broadcast_count();
    }

    pub fn connected(&self) -> usize {
        self.clients.len()
    }

    pub fn paired(&self) -> bool {
        self.clients.values().any(|c| c.partner.is_some())
    }

    pub fn searching(&self) -> usize {
        self.clients.values().filter(|c| c.searching).count()
    }

    pub fn frames_of_type(&self, kind: &str) -> Vec<(usize, Value)> {
        self.received.iter().filter(|(_, v)| v["type"] == kind).cloned().collect()
    }

    /// Close a client's socket from the server side.
    pub fn kick(&mut self, id: usize) {
        if let Some(tx) = self.clients.get(&id).and_then(|c| c.tx.as_ref()) {
            let _ = tx.send(Message::Close(None));
        }
    }

    pub fn ids(&self) -> Vec<usize> {
        let mut ids: Vec<_> = self.clients.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
}

pub struct Mock {
    pub addr: SocketAddr,
    pub state: Arc<Mutex<State>>,
}

impl Mock {
    pub async fn start() -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = shared.clone();
                tokio::spawn(async move {
                    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else { return };
                    let (mut sink, mut stream) = ws.split();
                    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
                    let id = {
                        let mut s = state.lock().unwrap();
                        let id = s.next_id;
                        s.next_id += 1;
                        s.clients.insert(id, Client { tx: Some(tx), ..Client::default() });
                        s.send(id, "connection_success", json!(format!("token-{id}")));
                        s.broadcast_count();
                        id
                    };
                    let writer = tokio::spawn(async move {
                        while let Some(msg) = rx.recv().await {
                            let close = matches!(msg, Message::Close(_));
                            if sink.send(msg).await.is_err() || close {
                                break;
                            }
                        }
                    });
                    while let Some(Ok(msg)) = stream.next().await {
                        match msg {
                            Message::Text(text) => {
                                if let Ok(v) = serde_json::from_str::<Value>(text.as_str()) {
                                    state.lock().unwrap().handle(id, v);
                                }
                            }
                            Message::Close(_) => break,
                            _ => {}
                        }
                    }
                    state.lock().unwrap().disconnect(id);
                    writer.abort();
                });
            }
        });
        Mock { addr, state }
    }

    pub fn url(&self) -> String {
        format!("ws://{}/", self.addr)
    }

    /// Poll until `check` passes, failing the test after a few seconds.
    pub async fn wait_for(&self, what: &str, check: impl Fn(&State) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if check(&self.state.lock().unwrap()) {
                return;
            }
            assert!(tokio::time::Instant::now() < deadline, "timed out waiting for: {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

pub fn prefs(gender: &str, species: &str) -> yap::protocol::WirePreferences {
    use yap::prefs::{Field, Preferences};
    let mut p = Preferences::default();
    p.toggle(Field::Gender, gender);
    p.toggle(Field::Species, species);
    p.toggle(Field::Role, "Switch");
    p.toggle(Field::PartnerRole, "Switch");
    p.toggle(Field::Kinks, "Biting");
    p.to_wire(true).unwrap()
}
