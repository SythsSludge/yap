//! A bounded log of raw websocket traffic for the debug viewer.

use chrono::{DateTime, Local};
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    In,
    Out,
    /// Connection lifecycle notes (handshake, errors, close), not frames.
    Meta,
}

impl Direction {
    pub fn arrow(self) -> &'static str {
        match self {
            Direction::In => "←",
            Direction::Out => "→",
            Direction::Meta => "·",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameKind {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
    Info,
    Error,
}

impl FrameKind {
    pub fn label(self) -> &'static str {
        match self {
            FrameKind::Text => "TEXT",
            FrameKind::Binary => "BIN",
            FrameKind::Ping => "PING",
            FrameKind::Pong => "PONG",
            FrameKind::Close => "CLOSE",
            FrameKind::Info => "INFO",
            FrameKind::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    /// Monotonic sequence number, stable across ring-buffer eviction.
    pub seq: u64,
    pub at: DateTime<Local>,
    pub dir: Direction,
    pub kind: FrameKind,
    /// Payload size in bytes on the wire.
    pub size: usize,
    /// Text payload, a hex dump for binary frames, or a description for meta entries.
    pub body: String,
}

impl Entry {
    /// The protocol message type for JSON text frames (`"ping"`, `"receive_message"`...).
    pub fn message_type(&self) -> Option<String> {
        if self.kind != FrameKind::Text {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(&self.body).ok()?;
        v.get("type")?.as_str().map(str::to_owned)
    }

    /// Our keepalive and the server's websocket pings: noise most of the time.
    pub fn is_heartbeat(&self) -> bool {
        matches!(self.kind, FrameKind::Ping | FrameKind::Pong) || self.message_type().as_deref() == Some("ping")
    }

    /// The body pretty-printed if it's JSON, else as-is.
    pub fn pretty_body(&self) -> String {
        serde_json::from_str::<serde_json::Value>(&self.body)
            .ok()
            .filter(|v| v.is_object() || v.is_array())
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| self.body.clone())
    }

    pub fn matches(&self, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        let f = filter.to_lowercase();
        self.body.to_lowercase().contains(&f) || self.kind.label().to_lowercase() == f
    }
}

/// Render binary payloads as a short hex dump.
pub fn hex_preview(bytes: &[u8]) -> String {
    const MAX: usize = 256;
    let hex: Vec<String> = bytes.iter().take(MAX).map(|b| format!("{b:02x}")).collect();
    let mut s = hex.join(" ");
    if bytes.len() > MAX {
        s.push_str(&format!(" … (+{} bytes)", bytes.len() - MAX));
    }
    s
}

pub struct TrafficLog {
    entries: VecDeque<Entry>,
    capacity: usize,
    next_seq: u64,
    bytes_in: u64,
    bytes_out: u64,
    /// Optional JSONL sink (`--traffic-log`), written as entries arrive.
    sink: Option<Box<dyn Write + Send>>,
}

impl std::fmt::Debug for TrafficLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrafficLog")
            .field("len", &self.entries.len())
            .field("capacity", &self.capacity)
            .field("sink", &self.sink.is_some())
            .finish()
    }
}

impl TrafficLog {
    pub fn new(capacity: usize) -> Self {
        TrafficLog {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            next_seq: 0,
            bytes_in: 0,
            bytes_out: 0,
            sink: None,
        }
    }

    pub fn with_sink(mut self, sink: Box<dyn Write + Send>) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    pub fn push(&mut self, at: DateTime<Local>, dir: Direction, kind: FrameKind, size: usize, body: String) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        match dir {
            Direction::In => self.bytes_in += size as u64,
            Direction::Out => self.bytes_out += size as u64,
            Direction::Meta => {}
        }
        let entry = Entry { seq, at, dir, kind, size, body };
        if let Some(sink) = &mut self.sink {
            let ok = serde_json::to_writer(&mut *sink, &entry).is_ok() && writeln!(sink).is_ok();
            if !ok {
                // Don't keep failing on every frame if the disk is full or similar.
                self.sink = None;
            }
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
        seq
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn totals(&self) -> (u64, u64) {
        (self.bytes_in, self.bytes_out)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Entries passing the viewer's filters, oldest first.
    pub fn visible<'a>(&'a self, filter: &'a str, hide_heartbeat: bool) -> impl Iterator<Item = &'a Entry> + 'a {
        self.entries.iter().filter(move |e| (!hide_heartbeat || !e.is_heartbeat()) && e.matches(filter))
    }

    /// Write the retained entries as JSON Lines.
    pub fn export(&self, mut out: impl Write) -> std::io::Result<usize> {
        for e in &self.entries {
            serde_json::to_writer(&mut out, e)?;
            writeln!(out)?;
        }
        Ok(self.entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn at() -> DateTime<Local> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap().into()
    }

    fn text(log: &mut TrafficLog, dir: Direction, body: &str) {
        log.push(at(), dir, FrameKind::Text, body.len(), body.into());
    }

    #[test]
    fn ring_buffer_evicts_oldest_but_keeps_sequence() {
        let mut log = TrafficLog::new(2);
        for i in 0..5 {
            text(&mut log, Direction::In, &i.to_string());
        }
        let seqs: Vec<_> = log.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![3, 4]);
        log.set_capacity(1);
        assert_eq!(log.len(), 1);
        assert_eq!(log.iter().next().unwrap().seq, 4);
    }

    #[test]
    fn counts_bytes_by_direction() {
        let mut log = TrafficLog::new(10);
        text(&mut log, Direction::In, "12345");
        text(&mut log, Direction::Out, "123");
        log.push(at(), Direction::Meta, FrameKind::Info, 0, "connected".into());
        assert_eq!(log.totals(), (5, 3));
    }

    #[test]
    fn identifies_heartbeats_and_types() {
        let mut log = TrafficLog::new(10);
        text(&mut log, Direction::Out, r#"{"type":"ping","data":true}"#);
        log.push(at(), Direction::In, FrameKind::Ping, 0, String::new());
        text(&mut log, Direction::In, r#"{"type":"receive_message","data":"hi"}"#);
        let visible: Vec<_> = log.visible("", true).collect();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].message_type().as_deref(), Some("receive_message"));
        assert_eq!(log.visible("", false).count(), 3);
        assert_eq!(log.visible("receive", false).count(), 1);
        assert_eq!(log.visible("ping", false).count(), 2);
    }

    #[test]
    fn pretty_prints_json_only() {
        let e =
            Entry { seq: 0, at: at(), dir: Direction::In, kind: FrameKind::Text, size: 0, body: r#"{"a":1}"#.into() };
        assert_eq!(e.pretty_body(), "{\n  \"a\": 1\n}");
        let e = Entry { body: "plain".into(), ..e };
        assert_eq!(e.pretty_body(), "plain");
    }

    #[test]
    fn hex_preview_truncates() {
        assert_eq!(hex_preview(&[0, 255, 16]), "00 ff 10");
        assert!(hex_preview(&[0; 300]).ends_with("(+44 bytes)"));
    }

    #[test]
    fn export_and_sink_write_jsonl() {
        #[derive(Clone, Default)]
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().write(b)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Shared::default();
        let mut log = TrafficLog::new(10).with_sink(Box::new(sink.clone()));
        text(&mut log, Direction::Out, r#"{"type":"ping","data":true}"#);
        text(&mut log, Direction::In, "x");

        let mut out = Vec::new();
        assert_eq!(log.export(&mut out).unwrap(), 2);
        let exported = String::from_utf8(out).unwrap();
        assert_eq!(exported, String::from_utf8(sink.0.lock().unwrap().clone()).unwrap());
        let first: serde_json::Value = serde_json::from_str(exported.lines().next().unwrap()).unwrap();
        assert_eq!(first["dir"], "out");
        assert_eq!(first["kind"], "text");
        assert_eq!(first["seq"], 0);
    }
}
