//! The storage drawer: labelled reference links you keep between sessions.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub label: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub added: DateTime<Utc>,
}

impl Item {
    /// Case-insensitive match over label, URL and note.
    pub fn matches(&self, filter: &str) -> bool {
        let f = filter.to_lowercase();
        f.is_empty()
            || self.label.to_lowercase().contains(&f)
            || self.url.to_lowercase().contains(&f)
            || self.note.to_lowercase().contains(&f)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Drawer {
    #[serde(default, rename = "item")]
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DrawerError {
    #[error("`{0}` isn't an http(s) link")]
    NotALink(String),
    #[error("that link is already in the drawer as `{0}`")]
    Duplicate(String),
}

/// Validate and normalise a URL for the drawer.
pub fn parse_link(input: &str) -> Result<String, DrawerError> {
    let input = input.trim();
    match url::Url::parse(input) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => Ok(u.to_string()),
        _ => Err(DrawerError::NotALink(input.into())),
    }
}

/// A default label: the last meaningful path segment, else the host.
pub fn suggest_label(url: &str) -> String {
    let Ok(u) = url::Url::parse(url) else {
        return url.into();
    };
    u.path_segments()
        .and_then(|mut s| s.rfind(|seg| !seg.is_empty()))
        .map(|seg| percent_decode(seg))
        .filter(|s| !s.is_empty())
        .map(|seg| format!("{} · {seg}", u.host_str().unwrap_or_default()))
        .unwrap_or_else(|| u.host_str().unwrap_or(url).to_owned())
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={s}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| s.to_owned())
}

impl Drawer {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(src) => toml::from_str(&src).with_context(|| format!("{} is not a valid drawer file", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Drawer::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::config::write_atomic(path, &toml::to_string_pretty(self)?)
    }

    /// Add a link at the top. An empty label gets a suggested one.
    pub fn add(&mut self, url: &str, label: &str, now: DateTime<Utc>) -> Result<usize, DrawerError> {
        let url = parse_link(url)?;
        if let Some(existing) = self.items.iter().find(|i| i.url == url) {
            return Err(DrawerError::Duplicate(existing.label.clone()));
        }
        let label = match label.trim() {
            "" => suggest_label(&url),
            l => l.to_owned(),
        };
        self.items.insert(0, Item { label, url, note: String::new(), added: now });
        Ok(0)
    }

    pub fn remove(&mut self, index: usize) -> Option<Item> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    /// Move an item up (`-1`) or down (`+1`). Returns its new index.
    pub fn shift(&mut self, index: usize, delta: isize) -> usize {
        let Some(target) = index.checked_add_signed(delta).filter(|t| *t < self.items.len()) else {
            return index;
        };
        self.items.swap(index, target);
        target
    }

    /// Indices of items matching the filter, in display order.
    pub fn filtered(&self, filter: &str) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, i)| i.matches(filter)).map(|(n, _)| n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn add_validates_and_dedupes() {
        let mut d = Drawer::default();
        assert_eq!(d.add("not a url", "", now()), Err(DrawerError::NotALink("not a url".into())));
        assert_eq!(d.add("javascript:alert(1)", "", now()), Err(DrawerError::NotALink("javascript:alert(1)".into())));
        d.add("https://e621.net/posts/1", "ref sheet", now()).unwrap();
        assert_eq!(d.add("https://e621.net/posts/1", "again", now()), Err(DrawerError::Duplicate("ref sheet".into())));
        d.add("https://example.com/b", "", now()).unwrap();
        assert_eq!(d.items[0].label, "example.com · b");
        assert_eq!(d.items[1].label, "ref sheet");
    }

    #[test]
    fn suggests_readable_labels() {
        assert_eq!(suggest_label("https://i.imgur.com/abc.png"), "i.imgur.com · abc.png");
        assert_eq!(suggest_label("https://e621.net/"), "e621.net");
        assert_eq!(suggest_label("https://x.com/a%20b/"), "x.com · a b");
    }

    #[test]
    fn shift_and_remove() {
        let mut d = Drawer::default();
        for u in ["https://a.com/", "https://b.com/", "https://c.com/"] {
            d.add(u, "", now()).unwrap();
        }
        // Newest first: c, b, a
        assert_eq!(d.shift(0, 1), 1);
        assert_eq!(d.items[0].url, "https://b.com/");
        assert_eq!(d.shift(0, -1), 0);
        assert_eq!(d.shift(2, 1), 2);
        assert_eq!(d.remove(1).unwrap().url, "https://c.com/");
        assert!(d.remove(5).is_none());
    }

    #[test]
    fn filters_across_fields() {
        let mut d = Drawer::default();
        d.add("https://e621.net/posts/1", "Ref Sheet", now()).unwrap();
        d.add("https://i.imgur.com/x.png", "outfit", now()).unwrap();
        d.items[0].note = "the blue one".into();
        assert_eq!(d.filtered("ref"), vec![1]);
        assert_eq!(d.filtered("IMGUR"), vec![0]);
        assert_eq!(d.filtered("blue"), vec![0]);
        assert_eq!(d.filtered(""), vec![0, 1]);
    }

    #[test]
    fn persists_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawer.toml");
        assert_eq!(Drawer::load(&path).unwrap(), Drawer::default());
        let mut d = Drawer::default();
        d.add("https://e621.net/posts/1", "ref", now()).unwrap();
        d.items[0].note = "note".into();
        d.save(&path).unwrap();
        assert_eq!(Drawer::load(&path).unwrap(), d);
    }
}
