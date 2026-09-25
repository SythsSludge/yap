//! The chat transcript and its scroll position.

use crate::links::find_links;
use crate::protocol::PartnerInfo;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    You(String),
    Partner(String),
    /// Status lines, styled like the website's grey system messages.
    System(String),
    /// Problems worth showing inline (unknown server frames and the like).
    Warning(String),
    /// "Your partner is a ..." with the kinks you share highlighted.
    PartnerInfo {
        info: PartnerInfo,
        common: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub at: DateTime<Local>,
    pub kind: EntryKind,
}

impl Entry {
    /// Message text for entries that can contain links.
    pub fn message_text(&self) -> Option<&str> {
        match &self.kind {
            EntryKind::You(t) | EntryKind::Partner(t) => Some(t),
            _ => None,
        }
    }

    /// `[2026-09-25 14:03:12] You: hi` for plain-text transcripts.
    pub fn transcript_line(&self) -> String {
        let time = self.at.format("%Y-%m-%d %H:%M:%S");
        match &self.kind {
            EntryKind::You(t) => format!("[{time}] You: {t}"),
            EntryKind::Partner(t) => format!("[{time}] Partner: {t}"),
            EntryKind::System(t) | EntryKind::Warning(t) => format!("[{time}] * {t}"),
            EntryKind::PartnerInfo { info, .. } => format!("[{time}] * {}", partner_summary(info)),
        }
    }
}

/// A link found in the chat, for the link picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLink {
    pub url: String,
    pub from_partner: bool,
}

#[derive(Debug, Default)]
pub struct Chat {
    pub entries: Vec<Entry>,
    pub partner_typing: bool,
    /// `None` follows the newest message; `Some(line)` pins the view's top line.
    pub pinned_top: Option<usize>,
    /// Messages that arrived while scrolled up.
    pub unread: usize,
    /// Filled in by the renderer so scrolling knows the geometry.
    pub last_total: usize,
    pub last_height: usize,
}

impl Chat {
    pub fn push(&mut self, at: DateTime<Local>, kind: EntryKind) {
        if self.pinned_top.is_some() {
            self.unread += 1;
        }
        self.entries.push(Entry { at, kind });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.follow();
    }

    fn max_top(&self) -> usize {
        self.last_total.saturating_sub(self.last_height)
    }

    /// The top line to render, given the current layout.
    pub fn top_line(&self, total: usize, height: usize) -> usize {
        let max = total.saturating_sub(height);
        self.pinned_top.map_or(max, |t| t.min(max))
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let top = self.pinned_top.unwrap_or(self.max_top()).min(self.max_top());
        if self.max_top() == 0 {
            return;
        }
        self.pinned_top = Some(top.saturating_sub(lines));
    }

    pub fn scroll_down(&mut self, lines: usize) {
        if let Some(top) = self.pinned_top {
            let top = top + lines;
            if top >= self.max_top() {
                self.follow();
            } else {
                self.pinned_top = Some(top);
            }
        }
    }

    pub fn page(&self) -> usize {
        self.last_height.saturating_sub(2).max(1)
    }

    pub fn follow(&mut self) {
        self.pinned_top = None;
        self.unread = 0;
    }

    pub fn is_following(&self) -> bool {
        self.pinned_top.is_none()
    }

    /// Every link in the chat, newest first, without duplicates.
    pub fn links(&self) -> Vec<ChatLink> {
        let mut out: Vec<ChatLink> = Vec::new();
        for entry in self.entries.iter().rev() {
            let Some(text) = entry.message_text() else { continue };
            let from_partner = matches!(entry.kind, EntryKind::Partner(_));
            for link in find_links(text).into_iter().rev() {
                if !out.iter().any(|l| l.url == link.url) {
                    out.push(ChatLink { url: link.url, from_partner });
                }
            }
        }
        out
    }

    /// Plain-text transcript for `/log`.
    pub fn transcript(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.transcript_line());
            out.push('\n');
        }
        out
    }
}

/// The web client's partner description sentence.
pub fn partner_summary(info: &PartnerInfo) -> String {
    format!("Your partner is a {}, {}, {} interested in: {}.", info.role, info.gender, info.species, info.kinks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> DateTime<Local> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap().into()
    }

    fn chat_with_geometry(total: usize, height: usize) -> Chat {
        Chat { last_total: total, last_height: height, ..Chat::default() }
    }

    #[test]
    fn scrolling_pins_and_returns_to_follow() {
        let mut chat = chat_with_geometry(100, 20);
        assert!(chat.is_following());
        assert_eq!(chat.top_line(100, 20), 80);
        chat.scroll_up(5);
        assert_eq!(chat.pinned_top, Some(75));
        chat.scroll_up(500);
        assert_eq!(chat.pinned_top, Some(0));
        chat.scroll_down(10);
        assert_eq!(chat.pinned_top, Some(10));
        chat.scroll_down(1000);
        assert!(chat.is_following());
    }

    #[test]
    fn scroll_up_is_a_noop_when_everything_fits() {
        let mut chat = chat_with_geometry(5, 20);
        chat.scroll_up(3);
        assert!(chat.is_following());
    }

    #[test]
    fn pinned_view_counts_unread_and_stays_put() {
        let mut chat = chat_with_geometry(100, 20);
        chat.scroll_up(10);
        chat.push(at(), EntryKind::Partner("hi".into()));
        chat.push(at(), EntryKind::Partner("hello?".into()));
        assert_eq!(chat.unread, 2);
        // More lines below doesn't move a pinned view.
        assert_eq!(chat.top_line(110, 20), 70);
        chat.follow();
        assert_eq!(chat.unread, 0);
    }

    #[test]
    fn collects_links_newest_first_without_duplicates() {
        let mut chat = Chat::default();
        chat.push(at(), EntryKind::You("mine https://a.com/1 and https://a.com/2".into()));
        chat.push(at(), EntryKind::System("https://ignored.com".into()));
        chat.push(at(), EntryKind::Partner("theirs https://b.com/x and again https://a.com/1".into()));
        let urls: Vec<_> = chat.links().into_iter().map(|l| (l.url, l.from_partner)).collect();
        assert_eq!(
            urls,
            vec![("https://a.com/1".into(), true), ("https://b.com/x".into(), true), ("https://a.com/2".into(), false),]
        );
    }

    #[test]
    fn transcript_formats_every_kind() {
        let mut chat = Chat::default();
        chat.push(at(), EntryKind::System("connected".into()));
        chat.push(at(), EntryKind::You("hi".into()));
        chat.push(at(), EntryKind::Partner("hey".into()));
        let t = chat.transcript();
        assert!(t.contains("] * connected\n"));
        assert!(t.contains("] You: hi\n"));
        assert!(t.contains("] Partner: hey\n"));
    }
}
