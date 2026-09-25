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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub added: DateTime<Utc>,
}

impl Item {
    /// Every whitespace-separated term must match. `#tag` terms need that exact tag;
    /// other terms match label, URL, note or tags, ignoring case.
    pub fn matches(&self, filter: &str) -> bool {
        filter.split_whitespace().all(|term| match term.strip_prefix('#') {
            Some(tag) if !tag.is_empty() => normalize_tag(tag).is_some_and(|t| self.tags.contains(&t)),
            _ => {
                let f = term.to_lowercase();
                self.label.to_lowercase().contains(&f)
                    || self.url.to_lowercase().contains(&f)
                    || self.note.to_lowercase().contains(&f)
                    || self.tags.iter().any(|t| t.contains(&f))
            }
        })
    }

    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// Canonical tag form: lowercase, no leading `#`, inner spaces as `-`, only letters,
/// digits, `-` and `_`.
pub fn normalize_tag(input: &str) -> Option<String> {
    let tag: String = input
        .trim()
        .trim_start_matches('#')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!tag.is_empty()).then_some(tag)
}

/// Parse a tag list typed as `ref, nsfw outfits` or `#ref #nsfw`.
pub fn parse_tags(input: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for t in input.split([',', ' ']).filter_map(normalize_tag) {
        if !tags.contains(&t) {
            tags.push(t);
        }
    }
    tags
}

/// Split `my ref sheet #ref #nsfw` into the label and its hashtags.
pub fn split_label_tags(input: &str) -> (String, Vec<String>) {
    let (tags, words): (Vec<&str>, Vec<&str>) =
        input.split_whitespace().partition(|w| w.starts_with('#') && w.len() > 1);
    (words.join(" "), parse_tags(&tags.join(" ")))
}

/// Reusable text: an intro, your limits, a polite "not a match".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snippet {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Drawer {
    #[serde(default, rename = "item")]
    pub items: Vec<Item>,
    #[serde(default, rename = "snippet", skip_serializing_if = "Vec::is_empty")]
    pub snippets: Vec<Snippet>,
}

/// Snippet names are used in `/snip <name>`, so they follow the tag rules.
pub fn normalize_snippet_name(input: &str) -> Option<String> {
    normalize_tag(input)
}

/// Fill `{placeholders}` from `vars`. Unknown placeholders are left alone so a typo is
/// visible rather than silently vanishing.
pub fn expand_placeholders(text: &str, vars: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let key = &after[..close];
                match vars.iter().find(|(k, _)| *k == key) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// What merging an imported drawer did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MergeReport {
    pub links_added: usize,
    /// Links already present whose tags or note were filled in from the import.
    pub links_updated: usize,
    pub snippets_added: usize,
    /// Snippets whose name clashed and were saved under a new one.
    pub snippets_renamed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DrawerError {
    #[error("`{0}` isn't an http(s) link")]
    NotALink(String),
    #[error("that link is already in the drawer as `{0}`")]
    Duplicate(String),
    #[error("`{0}` can't be a snippet name (use letters, digits, - or _)")]
    BadSnippetName(String),
    #[error("there's already a snippet called `{0}`")]
    SnippetExists(String),
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
        .map(percent_decode)
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

    /// Add a link at the top. `#words` in the label become tags; an empty label gets a
    /// suggested one.
    pub fn add(&mut self, url: &str, label: &str, now: DateTime<Utc>) -> Result<usize, DrawerError> {
        let url = parse_link(url)?;
        if let Some(existing) = self.items.iter().find(|i| i.url == url) {
            return Err(DrawerError::Duplicate(existing.label.clone()));
        }
        let (label, tags) = split_label_tags(label);
        let label = if label.is_empty() { suggest_label(&url) } else { label };
        self.items.insert(0, Item { label, url, note: String::new(), tags, added: now });
        Ok(0)
    }

    pub fn add_snippet(&mut self, name: &str, text: &str) -> Result<usize, DrawerError> {
        let name = normalize_snippet_name(name).ok_or_else(|| DrawerError::BadSnippetName(name.into()))?;
        if self.snippet(&name).is_some() {
            return Err(DrawerError::SnippetExists(name));
        }
        self.snippets.push(Snippet { name: name.clone(), text: text.trim_end().to_owned() });
        self.snippets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(self.snippets.iter().position(|s| s.name == name).expect("just added"))
    }

    pub fn rename_snippet(&mut self, index: usize, name: &str) -> Result<(), DrawerError> {
        let name = normalize_snippet_name(name).ok_or_else(|| DrawerError::BadSnippetName(name.into()))?;
        if self.snippets.iter().enumerate().any(|(i, s)| i != index && s.name == name) {
            return Err(DrawerError::SnippetExists(name));
        }
        if let Some(s) = self.snippets.get_mut(index) {
            s.name = name;
        }
        self.snippets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }

    pub fn snippet(&self, name: &str) -> Option<&Snippet> {
        let name = normalize_snippet_name(name)?;
        self.snippets.iter().find(|s| s.name == name)
    }

    /// Snippets whose name or text contains `filter`, by index.
    pub fn filtered_snippets(&self, filter: &str) -> Vec<usize> {
        let f = filter.to_lowercase();
        (0..self.snippets.len())
            .filter(|&i| {
                f.is_empty() || self.snippets[i].name.contains(&f) || self.snippets[i].text.to_lowercase().contains(&f)
            })
            .collect()
    }

    /// Merge an imported drawer. Nothing is overwritten: known links gain any new tags
    /// (and a note if they had none), clashing snippet names get a suffix.
    pub fn merge(&mut self, other: Drawer) -> MergeReport {
        let mut report = MergeReport::default();
        for item in other.items {
            let Ok(url) = parse_link(&item.url) else { continue };
            match self.items.iter_mut().find(|i| i.url == url) {
                Some(existing) => {
                    let before = (existing.tags.len(), existing.note.is_empty());
                    for tag in item.tags.iter().filter_map(|t| normalize_tag(t)) {
                        if !existing.tags.contains(&tag) {
                            existing.tags.push(tag);
                        }
                    }
                    if existing.note.is_empty() && !item.note.is_empty() {
                        existing.note = item.note;
                    }
                    if (existing.tags.len(), existing.note.is_empty()) != before {
                        report.links_updated += 1;
                    }
                }
                None => {
                    self.items.push(Item {
                        url,
                        tags: item.tags.iter().filter_map(|t| normalize_tag(t)).collect(),
                        ..item
                    });
                    report.links_added += 1;
                }
            }
        }
        for snippet in other.snippets {
            // Same text under any name means we already have it (it may have been
            // renamed by an earlier import).
            if self.snippets.iter().any(|s| s.text == snippet.text) {
                continue;
            }
            let base = normalize_snippet_name(&snippet.name).unwrap_or_else(|| "snippet".into());
            let name = if self.snippet(&base).is_none() {
                base
            } else {
                report.snippets_renamed += 1;
                (2..).map(|n| format!("{base}-{n}")).find(|n| self.snippet(n).is_none()).expect("infinite")
            };
            self.snippets.push(Snippet { name, text: snippet.text });
            report.snippets_added += 1;
        }
        self.snippets.sort_by(|a, b| a.name.cmp(&b.name));
        report
    }

    /// Export as TOML, or JSON for a `.json` path.
    pub fn export_to(&self, path: &Path) -> Result<()> {
        let text = match crate::config::Format::from_path(path) {
            crate::config::Format::Json => serde_json::to_string_pretty(self)?,
            crate::config::Format::Toml => toml::to_string_pretty(self)?,
        };
        crate::config::write_atomic(path, &text)
    }

    pub fn read_import(path: &Path) -> Result<Drawer> {
        let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        if src.trim_start().starts_with('{') {
            serde_json::from_str(&src).context("not a yap drawer export")
        } else {
            toml::from_str(&src).context("not a yap drawer export")
        }
    }

    /// Every tag in use with how many items carry it, alphabetically.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts = std::collections::BTreeMap::new();
        for tag in self.items.iter().flat_map(|i| &i.tags) {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
        counts.into_iter().collect()
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

    /// Indices of items matching the filter (and tag, if any), in display order.
    pub fn filtered(&self, filter: &str, tag: Option<&str>) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.matches(filter) && tag.is_none_or(|t| i.has_tag(t)))
            .map(|(n, _)| n)
            .collect()
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
    fn tags_from_labels_and_lists() {
        assert_eq!(
            split_label_tags("my ref #Ref #nsfw sheet"),
            ("my ref sheet".into(), vec!["ref".into(), "nsfw".into()])
        );
        assert_eq!(split_label_tags("#only"), (String::new(), vec!["only".into()]));
        assert_eq!(split_label_tags("C# is # fine"), ("C# is # fine".into(), vec![]));
        assert_eq!(parse_tags("ref, NSFW  #ref  big outfits"), vec!["ref", "nsfw", "big", "outfits"]);
        assert_eq!(normalize_tag("  #Ref Sheet! "), Some("ref-sheet".into()));
        assert_eq!(normalize_tag("#"), None);

        let mut d = Drawer::default();
        d.add("https://a.com/x.png", "#ref", now()).unwrap();
        assert_eq!(d.items[0].label, "a.com · x.png", "tag-only labels still get a name");
        assert_eq!(d.items[0].tags, vec!["ref"]);
        d.add("https://b.com/", "b #ref #nsfw", now()).unwrap();
        assert_eq!(d.tags(), vec![("nsfw".into(), 1), ("ref".into(), 2)]);
    }

    #[test]
    fn snippets_add_rename_and_find() {
        let mut d = Drawer::default();
        d.add_snippet("Intro", "Hi! I'm a {species}.\n").unwrap();
        assert_eq!(d.snippets[0].name, "intro");
        assert_eq!(d.snippets[0].text, "Hi! I'm a {species}.");
        assert_eq!(d.add_snippet("intro", "x"), Err(DrawerError::SnippetExists("intro".into())));
        assert_eq!(d.add_snippet("!!", "x"), Err(DrawerError::BadSnippetName("!!".into())));
        d.add_snippet("limits", "no gore").unwrap();
        assert_eq!(d.snippet("INTRO").unwrap().text, "Hi! I'm a {species}.");
        assert_eq!(d.filtered_snippets("gore"), vec![1]);
        assert_eq!(d.rename_snippet(1, "intro"), Err(DrawerError::SnippetExists("intro".into())));
        d.rename_snippet(1, "a limits").unwrap();
        assert_eq!(d.snippets[0].name, "a-limits", "kept sorted");
    }

    #[test]
    fn placeholders_expand_known_keys_only() {
        let vars = [("species", "Wolf".to_string()), ("partner_species", "Fox".to_string())];
        assert_eq!(expand_placeholders("A {species} for a {partner_species}", &vars), "A Wolf for a Fox");
        assert_eq!(expand_placeholders("{unknown} {species", &vars), "{unknown} {species");
        assert_eq!(expand_placeholders("{}", &vars), "{}");
    }

    #[test]
    fn merge_adds_without_overwriting() {
        let mut mine = Drawer::default();
        mine.add("https://a.com/", "mine #ref", now()).unwrap();
        mine.add_snippet("intro", "my intro").unwrap();
        let mut theirs = Drawer::default();
        theirs.add("https://a.com/", "theirs #art", now()).unwrap();
        theirs.items[0].note = "from a friend".into();
        theirs.add("https://b.com/", "new", now()).unwrap();
        theirs.add_snippet("intro", "their intro").unwrap();
        theirs.add_snippet("bye", "later!").unwrap();

        let report = mine.merge(theirs.clone());
        assert_eq!(report, MergeReport { links_added: 1, links_updated: 1, snippets_added: 2, snippets_renamed: 1 });
        let a = mine.items.iter().find(|i| i.url == "https://a.com/").unwrap();
        assert_eq!(a.label, "mine", "labels are never overwritten");
        assert_eq!(a.tags, vec!["ref", "art"]);
        assert_eq!(a.note, "from a friend");
        assert_eq!(mine.snippet("intro").unwrap().text, "my intro");
        assert_eq!(mine.snippet("intro-2").unwrap().text, "their intro");
        // Importing the same thing again changes nothing.
        assert_eq!(mine.merge(theirs), MergeReport::default());
    }

    #[test]
    fn export_import_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Drawer::default();
        d.add("https://a.com/", "a #x", now()).unwrap();
        d.add_snippet("hi", "hello").unwrap();
        for name in ["drawer.toml", "drawer.json"] {
            let path = dir.path().join(name);
            d.export_to(&path).unwrap();
            assert_eq!(Drawer::read_import(&path).unwrap(), d, "{name}");
        }
        std::fs::write(dir.path().join("bad.toml"), "nonsense = [").unwrap();
        assert!(Drawer::read_import(&dir.path().join("bad.toml")).is_err());
    }

    #[test]
    fn filters_by_tag() {
        let mut d = Drawer::default();
        d.add("https://a.com/", "alpha #ref", now()).unwrap();
        d.add("https://b.com/", "beta #ref #nsfw", now()).unwrap();
        d.add("https://c.com/", "gamma", now()).unwrap();
        // Newest first: c, b, a
        assert_eq!(d.filtered("", Some("ref")), vec![1, 2]);
        assert_eq!(d.filtered("#nsfw", None), vec![1]);
        assert_eq!(d.filtered("#ref alpha", None), vec![2]);
        assert_eq!(d.filtered("nsf", None), vec![1], "plain terms search tags too");
        assert_eq!(d.filtered("", Some("missing")), Vec::<usize>::new());
    }

    #[test]
    fn filters_across_fields() {
        let mut d = Drawer::default();
        d.add("https://e621.net/posts/1", "Ref Sheet", now()).unwrap();
        d.add("https://i.imgur.com/x.png", "outfit", now()).unwrap();
        d.items[0].note = "the blue one".into();
        assert_eq!(d.filtered("ref", None), vec![1]);
        assert_eq!(d.filtered("IMGUR", None), vec![0]);
        assert_eq!(d.filtered("blue", None), vec![0]);
        assert_eq!(d.filtered("", None), vec![0, 1]);
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
