//! A little companion beside the message box that reacts to what happens: a match,
//! a message, your name, a partner leaving. Built-ins ship as TOML, and your own go
//! in `~/.config/yap/buddies/*.toml` in the same format.

use crate::theme::parse_color;
use ratatui::style::Color;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// The most a buddy can be: it sits beside the three-row message box.
pub const MAX_LINES: usize = 3;
pub const MAX_WIDTH: usize = 12;

const BUILT_IN: &[(&str, &str)] = &[
    ("fox", include_str!("../assets/buddies/fox.toml")),
    ("cat", include_str!("../assets/buddies/cat.toml")),
    ("blob", include_str!("../assets/buddies/blob.toml")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mood {
    Idle,
    Happy,
    Excited,
    Love,
    Sad,
    Surprised,
    Sleepy,
    Curious,
    Searching,
    Proud,
    Dizzy,
}

impl Mood {
    pub const ALL: [Mood; 11] = [
        Mood::Idle,
        Mood::Happy,
        Mood::Excited,
        Mood::Love,
        Mood::Sad,
        Mood::Surprised,
        Mood::Sleepy,
        Mood::Curious,
        Mood::Searching,
        Mood::Proud,
        Mood::Dizzy,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mood::Idle => "idle",
            Mood::Happy => "happy",
            Mood::Excited => "excited",
            Mood::Love => "love",
            Mood::Sad => "sad",
            Mood::Surprised => "surprised",
            Mood::Sleepy => "sleepy",
            Mood::Curious => "curious",
            Mood::Searching => "searching",
            Mood::Proud => "proud",
            Mood::Dizzy => "dizzy",
        }
    }

    fn from_name(name: &str) -> Option<Mood> {
        Mood::ALL.into_iter().find(|m| m.name() == name)
    }
}

/// Something the buddy reacts to. The names are the keys of a buddy's `[says]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Matched,
    /// Matched with someone who shares three or more of your kinks.
    SharedKinks,
    Message,
    /// Your character's name or a keyword came up.
    Mentioned,
    /// A heart in their message.
    Heart,
    Typing,
    Sent,
    /// You sent a long post.
    LongPost,
    Left,
    Dropped,
    Skipped,
    Blocked,
    Searching,
    ConnectionLost,
    Reconnected,
    /// You clicked it.
    Petted,
}

impl Event {
    pub const ALL: [Event; 16] = [
        Event::Matched,
        Event::SharedKinks,
        Event::Message,
        Event::Mentioned,
        Event::Heart,
        Event::Typing,
        Event::Sent,
        Event::LongPost,
        Event::Left,
        Event::Dropped,
        Event::Skipped,
        Event::Blocked,
        Event::Searching,
        Event::ConnectionLost,
        Event::Reconnected,
        Event::Petted,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Event::Matched => "matched",
            Event::SharedKinks => "shared_kinks",
            Event::Message => "message",
            Event::Mentioned => "mentioned",
            Event::Heart => "heart",
            Event::Typing => "typing",
            Event::Sent => "sent",
            Event::LongPost => "long_post",
            Event::Left => "left",
            Event::Dropped => "dropped",
            Event::Skipped => "skipped",
            Event::Blocked => "blocked",
            Event::Searching => "searching",
            Event::ConnectionLost => "connection_lost",
            Event::Reconnected => "reconnected",
            Event::Petted => "petted",
        }
    }

    /// How it makes the buddy feel, and for how many seconds.
    pub fn mood(self) -> (Mood, u64) {
        match self {
            Event::Matched => (Mood::Excited, 6),
            Event::SharedKinks | Event::Heart | Event::Petted => (Mood::Love, 5),
            Event::Message => (Mood::Happy, 2),
            Event::Mentioned => (Mood::Surprised, 4),
            Event::Typing => (Mood::Curious, 2),
            Event::Sent => (Mood::Happy, 2),
            Event::LongPost => (Mood::Proud, 5),
            Event::Left | Event::Dropped => (Mood::Sad, 8),
            Event::Skipped | Event::Blocked => (Mood::Proud, 3),
            Event::Searching => (Mood::Searching, 3),
            Event::ConnectionLost => (Mood::Dizzy, 8),
            Event::Reconnected => (Mood::Happy, 4),
        }
    }

    /// Whether it says something, not just pulls a face. Frequent events stay quiet.
    pub fn speaks(self) -> bool {
        !matches!(self, Event::Message | Event::Typing | Event::Sent)
    }
}

/// Lines every buddy knows, unless its file says otherwise. `{partner_species}` and
/// the other snippet placeholders are filled in.
fn default_says(event: Event) -> &'static [&'static str] {
    match event {
        Event::Matched => &["ooh, a {partner_species}!", "hi hi!", "a new friend!"],
        Event::SharedKinks => &["you two have lots in common…", "ooh, compatible!"],
        Event::Mentioned => &["they said your name!", "ears up, that's you!"],
        Event::Heart => &["aww ♥", "♥♥♥"],
        Event::LongPost => &["that's a big one!", "look at you go"],
        Event::Left => &["they left… next one'll be nicer", "aw, bye then"],
        Event::Dropped => &["poof, gone", "the gremlins got them"],
        Event::Skipped => &["nope, not for us", "skip!"],
        Event::Blocked => &["good riddance", "blocked!"],
        Event::Searching => &["sniffing around…", "let's find someone"],
        Event::ConnectionLost => &["uh oh, we're offline", "the internet ate us"],
        Event::Reconnected => &["back online!"],
        Event::Petted => &["hehe", "♥", "again!"],
        Event::Message | Event::Typing | Event::Sent => &[],
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Buddy {
    pub name: String,
    pub color: Option<Color>,
    /// Milliseconds per animation frame.
    pub speed: u64,
    /// Frames for each mood; a frame is its lines.
    pub moods: BTreeMap<Mood, Vec<Vec<String>>>,
    /// Lines for events, by event name.
    pub says: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuddyFile {
    name: Option<String>,
    extends: Option<String>,
    color: Option<String>,
    speed: Option<u64>,
    #[serde(default)]
    moods: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    says: BTreeMap<String, Vec<String>>,
}

impl Buddy {
    /// The frames for `mood`, or `idle`'s if it has none.
    pub fn frames(&self, mood: Mood) -> &[Vec<String>] {
        self.moods.get(&mood).or_else(|| self.moods.get(&Mood::Idle)).map_or(&[], Vec::as_slice)
    }

    /// The frame to show `elapsed_ms` into a mood.
    pub fn frame(&self, mood: Mood, elapsed_ms: u128) -> &[String] {
        let frames = self.frames(mood);
        if frames.is_empty() {
            return &[];
        }
        &frames[(elapsed_ms / u128::from(self.speed.max(50))) as usize % frames.len()]
    }

    /// Columns the widest frame needs.
    pub fn width(&self) -> usize {
        self.moods.values().flatten().flatten().map(|l| crate::text::width(l)).max().unwrap_or(0)
    }

    /// What it might say for `event`.
    pub fn lines_for(&self, event: Event) -> Vec<String> {
        match self.says.get(event.name()) {
            Some(lines) => lines.clone(),
            None => default_says(event).iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// Parse a buddy file against the buddies loaded so far (for `extends`).
pub fn parse(src: &str, fallback_name: &str, known: &[Buddy]) -> Result<Buddy, String> {
    let file: BuddyFile = toml::from_str(src).map_err(|e| e.message().to_owned())?;
    let mut buddy = match &file.extends {
        Some(parent) => {
            known.iter().find(|b| b.name == *parent).cloned().ok_or(format!("extends unknown buddy `{parent}`"))?
        }
        None => Buddy { name: String::new(), color: None, speed: 700, moods: BTreeMap::new(), says: BTreeMap::new() },
    };
    buddy.name = file.name.unwrap_or_else(|| fallback_name.to_owned());
    if let Some(color) = &file.color {
        buddy.color = Some(parse_color(color).map_err(|e| format!("color: {e}"))?);
    }
    if let Some(speed) = file.speed {
        buddy.speed = speed;
    }
    for (name, frames) in file.moods {
        let mood = Mood::from_name(&name).ok_or_else(|| {
            let valid: Vec<&str> = Mood::ALL.iter().map(|m| m.name()).collect();
            format!("unknown mood `{name}` (valid: {})", valid.join(", "))
        })?;
        let frames: Vec<Vec<String>> = frames
            .iter()
            .map(|f| f.trim_start_matches('\n').lines().map(|l| l.trim_end().to_owned()).collect::<Vec<_>>())
            .collect();
        for frame in &frames {
            if frame.len() > MAX_LINES || frame.iter().any(|l| crate::text::width(l) > MAX_WIDTH) {
                return Err(format!("mood `{name}`: frames can be at most {MAX_LINES} lines of {MAX_WIDTH} columns"));
            }
        }
        if !frames.is_empty() {
            buddy.moods.insert(mood, frames);
        }
    }
    for (event, lines) in file.says {
        if !Event::ALL.iter().any(|e| e.name() == event) {
            let valid: Vec<&str> = Event::ALL.iter().map(|e| e.name()).collect();
            return Err(format!("unknown event `{event}` in [says] (valid: {})", valid.join(", ")));
        }
        buddy.says.insert(event, lines);
    }
    if !buddy.moods.contains_key(&Mood::Idle) {
        return Err("needs at least an `idle` mood".into());
    }
    Ok(buddy)
}

pub fn builtins() -> Vec<Buddy> {
    let mut out: Vec<Buddy> = Vec::new();
    for (name, src) in BUILT_IN {
        let buddy = parse(src, name, &out).unwrap_or_else(|e| panic!("built-in buddy {name}: {e}"));
        out.push(buddy);
    }
    out
}

/// Built-ins plus every `*.toml` in `dir` (sorted, so `extends` can use earlier files).
/// A file with a built-in's name replaces it.
pub fn load_all(dir: &Path) -> (Vec<Buddy>, Vec<String>) {
    let mut buddies = builtins();
    let mut errors = Vec::new();
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    for path in paths {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("buddy").to_owned();
        let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| parse(&s, &stem, &buddies));
        match parsed {
            Ok(b) => match buddies.iter_mut().find(|x| x.name == b.name) {
                Some(existing) => *existing = b,
                None => buddies.push(b),
            },
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    (buddies, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_fit_and_have_every_mood() {
        for b in builtins() {
            assert!(b.width() <= MAX_WIDTH, "{} is too wide", b.name);
            for mood in Mood::ALL {
                assert!(!b.frames(mood).is_empty(), "{} has no {}", b.name, mood.name());
            }
        }
    }

    #[test]
    fn custom_buddies_extend_and_are_checked() {
        let known = builtins();
        let src =
            "extends = \"cat\"\nspeed = 300\n[moods]\nhappy = ['''\n(^_^)''']\n[says]\nmatched = [\"hey {partner}\"]\n";
        let b = parse(src, "mine", &known).unwrap();
        assert_eq!(b.name, "mine");
        assert_eq!(b.frames(Mood::Happy), [vec!["(^_^)".to_owned()]]);
        assert_eq!(b.frames(Mood::Sad), known[1].frames(Mood::Sad), "the rest comes from the cat");
        assert_eq!(b.lines_for(Event::Matched), ["hey {partner}"]);
        assert!(b.lines_for(Event::Left).iter().any(|l| l.contains("bye")), "defaults fill the gaps");
        assert_eq!(b.frame(Mood::Idle, 0), b.frames(Mood::Idle)[0].as_slice());

        assert!(parse("[moods]\nhappy = ['x']", "x", &known).unwrap_err().contains("idle"));
        assert!(parse("[moods]\nidle = ['way too wide for a buddy']", "x", &known).unwrap_err().contains("12 columns"));
        assert!(parse("[moods]\nidle = ['x']\nangry = ['x']", "x", &known).unwrap_err().contains("unknown mood"));
        assert!(
            parse("[moods]\nidle = ['x']\n[says]\nwat = ['x']", "x", &known).unwrap_err().contains("unknown event")
        );
    }
}
