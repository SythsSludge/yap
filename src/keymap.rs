//! Rebindable keys for global actions.
//!
//! Bindings live in the config as `[settings.keys]`, holding only changes from the
//! defaults:
//!
//! ```toml
//! [settings.keys]
//! find = "alt+f"
//! leave = ["ctrl+l", "f9"]
//! block = []            # unbound
//! ```

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

macro_rules! actions {
    ($($variant:ident => $name:literal, $desc:literal, [$($key:literal),*];)*) => {
        /// Something a global key can do.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action {
            $($variant,)*
        }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$variant,)*];

            /// The name used in the config file.
            pub fn name(self) -> &'static str {
                match self {
                    $(Action::$variant => $name,)*
                }
            }

            pub fn describe(self) -> &'static str {
                match self {
                    $(Action::$variant => $desc,)*
                }
            }

            pub fn defaults(self) -> &'static [&'static str] {
                match self {
                    $(Action::$variant => &[$($key),*],)*
                }
            }

            pub fn from_name(name: &str) -> Option<Action> {
                match name {
                    $($name => Some(Action::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

actions! {
    Help => "help", "Show help", ["f1"];
    Palette => "palette", "Command palette: search everything", ["ctrl+k"];
    Find => "find", "Find a partner", ["ctrl+f"];
    Next => "next", "Skip to the next partner (no confirmation)", ["ctrl+n"];
    Leave => "leave", "Leave partner / stop searching", ["ctrl+d"];
    Block => "block", "Block partner", ["ctrl+b"];
    Links => "links", "Links in this chat", ["ctrl+o"];
    Snippets => "snippets", "Insert a snippet", ["ctrl+g"];
    Editor => "editor", "Write the message in your editor", ["ctrl+x"];
    SelectMessage => "select-message", "Select a message to quote, copy or save", ["alt+m"];
    SearchChat => "search-chat", "Search this chat", ["alt+/"];
    Spelling => "spelling", "Fix the misspelled word at the cursor", ["alt+s"];
    Kinks => "kinks", "Kinks, explained: your partner's next to yours", ["alt+k"];
    Drawer => "drawer", "Toggle the drawer panel", ["ctrl+e"];
    Profile => "profile", "Switch profile", ["ctrl+p"];
    Theme => "theme", "Switch theme", ["ctrl+t"];
    Sidebar => "sidebar", "Toggle the sidebar", ["ctrl+s"];
    Reconnect => "reconnect", "Reconnect", ["ctrl+r"];
    Quit => "quit", "Quit", ["ctrl+q"];
    NewChat => "new-chat", "Open another chat", ["alt+n"];
    CloseChat => "close-chat", "Close this chat", ["alt+w"];
    NextChat => "next-chat", "Next chat", ["ctrl+pgdn"];
    PrevChat => "prev-chat", "Previous chat", ["ctrl+pgup"];
    TabChat => "tab-chat", "Chat tab", ["f2", "alt+1"];
    TabPreferences => "tab-preferences", "Preferences tab", ["f3", "alt+2"];
    TabDrawer => "tab-drawer", "Drawer tab", ["f4", "alt+3"];
    TabLogs => "tab-logs", "Logs tab", ["f5", "alt+4"];
    TabTraffic => "tab-traffic", "Traffic tab", ["f6", "alt+5"];
    TabSettings => "tab-settings", "Settings tab", ["f7", "alt+6"];
    ScrollPageUp => "scroll-page-up", "Scroll chat up a page", ["pgup"];
    ScrollPageDown => "scroll-page-down", "Scroll chat down a page", ["pgdn"];
    ScrollUp => "scroll-up", "Scroll chat up a line", ["shift+up", "ctrl+up"];
    ScrollDown => "scroll-down", "Scroll chat down a line", ["shift+down", "ctrl+down"];
    JumpToNewest => "jump-to-newest", "Jump to the newest message", ["ctrl+end"];
}

impl Action {
    /// Scrolling only means something in the chat tab; elsewhere those keys move lists.
    pub fn chat_only(self) -> bool {
        matches!(
            self,
            Action::ScrollPageUp
                | Action::ScrollPageDown
                | Action::ScrollUp
                | Action::ScrollDown
                | Action::JumpToNewest
        )
    }
}

/// A key plus modifiers, normalised so that equivalent events compare equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    pub fn new(code: KeyCode, mods: KeyModifiers) -> Self {
        let mut mods = mods & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let code = match code {
            // Terminals disagree on whether ctrl+shift+f is 'F' or 'f'+SHIFT.
            KeyCode::Char(c) if c.is_uppercase() => {
                mods |= KeyModifiers::SHIFT;
                KeyCode::Char(c.to_lowercase().next().unwrap_or(c))
            }
            // For symbols the shifted character already says it all ('?' not shift+'/').
            KeyCode::Char(c) if !c.is_alphabetic() => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::Char(c)
            }
            KeyCode::BackTab => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            other => other,
        };
        Chord { code, mods }
    }

    pub fn from_event(key: &KeyEvent) -> Self {
        Chord::new(key.code, key.modifiers)
    }

    /// Parse `ctrl+f`, `alt+shift+x`, `f5`, `pgup`, `shift+up`...
    pub fn parse(input: &str) -> Result<Chord, String> {
        let input = input.trim().to_lowercase();
        if input.is_empty() {
            return Err("empty key".into());
        }
        let parts: Vec<&str> = input.split('+').collect();
        let (key, mod_parts) = parts.split_last().expect("split yields at least one part");
        let mut mods = KeyModifiers::NONE;
        for m in mod_parts {
            mods |= match *m {
                "ctrl" | "control" | "c" => KeyModifiers::CONTROL,
                "alt" | "meta" | "option" | "m" => KeyModifiers::ALT,
                "shift" | "s" => KeyModifiers::SHIFT,
                other => return Err(format!("unknown modifier `{other}` in `{input}`")),
            };
        }
        let code = match *key {
            "" => return Err(format!("`{input}` is missing a key (use `plus` for +)")),
            "plus" => KeyCode::Char('+'),
            "space" => KeyCode::Char(' '),
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "backspace" | "bs" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "insert" | "ins" => KeyCode::Insert,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pgup" | "pageup" => KeyCode::PageUp,
            "pgdn" | "pagedown" | "pgdown" => KeyCode::PageDown,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            f if f.starts_with('f') && f.len() > 1 && f[1..].chars().all(|c| c.is_ascii_digit()) => {
                match f[1..].parse::<u8>() {
                    Ok(n @ 1..=24) => KeyCode::F(n),
                    _ => return Err(format!("no such function key `{f}`")),
                }
            }
            k if k.chars().count() == 1 => KeyCode::Char(k.chars().next().expect("one char")),
            other => return Err(format!("unknown key `{other}`")),
        };
        Ok(Chord::new(code, mods))
    }

    fn key_name(self) -> String {
        match self.code {
            KeyCode::Char('+') => "plus".into(),
            KeyCode::Char(' ') => "space".into(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("f{n}"),
            KeyCode::Enter => "enter".into(),
            KeyCode::Esc => "esc".into(),
            KeyCode::Tab => "tab".into(),
            KeyCode::BackTab => "shift+tab".into(),
            KeyCode::Backspace => "backspace".into(),
            KeyCode::Delete => "delete".into(),
            KeyCode::Insert => "insert".into(),
            KeyCode::Home => "home".into(),
            KeyCode::End => "end".into(),
            KeyCode::PageUp => "pgup".into(),
            KeyCode::PageDown => "pgdn".into(),
            KeyCode::Up => "up".into(),
            KeyCode::Down => "down".into(),
            KeyCode::Left => "left".into(),
            KeyCode::Right => "right".into(),
            other => format!("{other:?}").to_lowercase(),
        }
    }

    /// Compact form for hints: `^F` for ctrl+letter, `F5` for a bare function key,
    /// otherwise the config form.
    pub fn short(self) -> String {
        match self.code {
            KeyCode::Char(c) if self.mods == KeyModifiers::CONTROL && c.is_ascii_alphabetic() => {
                format!("^{}", c.to_ascii_uppercase())
            }
            KeyCode::F(n) if self.mods.is_empty() => format!("F{n}"),
            _ => self.to_string(),
        }
    }

    /// Keys the app needs for typing and editing, which can't be rebound.
    pub fn reserved(self) -> Option<&'static str> {
        let ctrl = self.mods.contains(KeyModifiers::CONTROL);
        let alt = self.mods.contains(KeyModifiers::ALT);
        let shift = self.mods.contains(KeyModifiers::SHIFT);
        match self.code {
            KeyCode::Char('c') if ctrl && !alt => Some("ctrl+c is always quit"),
            KeyCode::Char('w' | 'u') if ctrl && !alt => Some("it's used for editing text"),
            KeyCode::Char(_) if !ctrl && !alt => Some("it's needed for typing"),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace if ctrl || alt => Some("it's used for editing text"),
            KeyCode::Enter
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Up
            | KeyCode::Down
                if !ctrl && !alt && !shift =>
            {
                Some("it's used for typing and moving around lists")
            }
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab if !ctrl && !alt => {
                Some("it's used to go back and switch focus")
            }
            _ => None,
        }
    }
}

impl fmt::Display for Chord {
    /// The config-file form: `ctrl+alt+shift+key`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            s.push_str("ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            s.push_str("alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            s.push_str("shift+");
        }
        s.push_str(&self.key_name());
        f.write_str(&s)
    }
}

/// One key or several, as written in the config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyList {
    One(String),
    Many(Vec<String>),
}

impl KeyList {
    pub fn items(&self) -> Vec<&str> {
        match self {
            KeyList::One(s) if s.trim().is_empty() => Vec::new(),
            KeyList::One(s) => vec![s.as_str()],
            KeyList::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }

    fn from_chords(chords: &[Chord]) -> Self {
        match chords {
            [one] => KeyList::One(one.to_string()),
            many => KeyList::Many(many.iter().map(Chord::to_string).collect()),
        }
    }
}

pub type Overrides = BTreeMap<String, KeyList>;

#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    bindings: BTreeMap<Action, Vec<Chord>>,
    lookup: HashMap<Chord, Action>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::build(&Overrides::new()).0
    }
}

fn default_chords(action: Action) -> Vec<Chord> {
    action.defaults().iter().map(|k| Chord::parse(k).expect("built-in keys parse")).collect()
}

impl Keymap {
    /// Defaults with the user's overrides applied. Problems become warnings and the
    /// offending entry is skipped, so a typo never stops the app from starting.
    pub fn build(overrides: &Overrides) -> (Keymap, Vec<String>) {
        let mut warnings = Vec::new();
        let mut custom: BTreeMap<Action, Vec<Chord>> = BTreeMap::new();
        for (name, keys) in overrides {
            let Some(action) = Action::from_name(name) else {
                warnings.push(format!("keys: unknown action `{name}`"));
                continue;
            };
            let mut chords = Vec::new();
            for key in keys.items() {
                match Chord::parse(key) {
                    Ok(c) => match c.reserved() {
                        Some(why) => warnings.push(format!("keys.{name}: can't use `{c}`, {why}")),
                        None => chords.push(c),
                    },
                    Err(e) => warnings.push(format!("keys.{name}: {e}")),
                }
            }
            custom.insert(action, chords);
        }

        let mut map = Keymap { bindings: BTreeMap::new(), lookup: HashMap::new() };
        // Customised actions claim their keys first, so they win over defaults.
        for (&action, chords) in &custom {
            let mut kept = Vec::new();
            for &c in chords {
                match map.lookup.get(&c) {
                    Some(other) => warnings.push(format!(
                        "keys: `{c}` is set for both `{}` and `{}`; keeping `{}`",
                        other.name(),
                        action.name(),
                        other.name()
                    )),
                    None => {
                        map.lookup.insert(c, action);
                        kept.push(c);
                    }
                }
            }
            map.bindings.insert(action, kept);
        }
        for &action in Action::ALL {
            if custom.contains_key(&action) {
                continue;
            }
            // A default key taken by a customised action is simply dropped.
            let kept: Vec<Chord> = default_chords(action).into_iter().filter(|c| !map.lookup.contains_key(c)).collect();
            for &c in &kept {
                map.lookup.insert(c, action);
            }
            map.bindings.insert(action, kept);
        }
        (map, warnings)
    }

    pub fn action(&self, key: &KeyEvent) -> Option<Action> {
        self.lookup.get(&Chord::from_event(key)).copied()
    }

    pub fn keys(&self, action: Action) -> &[Chord] {
        self.bindings.get(&action).map_or(&[], Vec::as_slice)
    }

    /// The primary key in hint form, or `None` if unbound.
    pub fn hint(&self, action: Action) -> Option<String> {
        self.keys(action).first().map(|c| c.short())
    }

    /// Like [`Keymap::hint`] but always printable.
    pub fn label(&self, action: Action) -> String {
        self.hint(action).unwrap_or_else(|| format!("({})", action.name()))
    }

    /// Make `chord` the only key for `action`, taking it from whatever had it.
    /// Returns the action it was taken from.
    pub fn bind(&mut self, action: Action, chord: Chord) -> Option<Action> {
        let previous = self.lookup.insert(chord, action).filter(|a| *a != action);
        if let Some(prev) = previous
            && let Some(keys) = self.bindings.get_mut(&prev)
        {
            keys.retain(|c| *c != chord);
        }
        for old in self.bindings.insert(action, vec![chord]).unwrap_or_default() {
            if old != chord {
                self.lookup.remove(&old);
            }
        }
        previous
    }

    /// Restore an action's default keys (those not in use by something else).
    pub fn reset(&mut self, action: Action) {
        for old in self.bindings.remove(&action).unwrap_or_default() {
            self.lookup.remove(&old);
        }
        let kept: Vec<Chord> = default_chords(action).into_iter().filter(|c| !self.lookup.contains_key(c)).collect();
        for &c in &kept {
            self.lookup.insert(c, action);
        }
        self.bindings.insert(action, kept);
    }

    pub fn unbind(&mut self, action: Action) {
        for old in self.bindings.insert(action, Vec::new()).unwrap_or_default() {
            self.lookup.remove(&old);
        }
    }

    /// Only the actions that differ from their defaults, for saving.
    pub fn overrides(&self) -> Overrides {
        Action::ALL
            .iter()
            .filter(|&&a| self.keys(a) != default_chords(a).as_slice())
            .map(|&a| (a.name().to_owned(), KeyList::from_chords(self.keys(a))))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn over(pairs: &[(&str, &[&str])]) -> Overrides {
        pairs.iter().map(|(k, v)| (k.to_string(), KeyList::Many(v.iter().map(|s| s.to_string()).collect()))).collect()
    }

    #[test]
    fn every_default_parses_and_nothing_collides() {
        let (map, warnings) = Keymap::build(&Overrides::new());
        assert!(warnings.is_empty(), "{warnings:?}");
        for &a in Action::ALL {
            assert!(!map.keys(a).is_empty(), "{a:?} has no default key");
            assert_eq!(Action::from_name(a.name()), Some(a));
            for c in map.keys(a) {
                assert_eq!(c.reserved(), None, "default {c} for {a:?} is reserved");
            }
        }
    }

    #[test]
    fn parses_and_prints_round_trip() {
        for s in [
            "ctrl+f",
            "alt+1",
            "f5",
            "pgup",
            "shift+up",
            "ctrl+alt+shift+x",
            "ctrl+plus",
            "ctrl+space",
            "ctrl+end",
            "shift+tab",
        ] {
            let c = Chord::parse(s).unwrap();
            assert_eq!(c.to_string(), s, "{s}");
            assert_eq!(Chord::parse(&c.to_string()).unwrap(), c);
        }
        assert_eq!(Chord::parse("Control+PageUp").unwrap().to_string(), "ctrl+pgup");
        // Config files are case-insensitive: `Ctrl+F` is plain ctrl+f; shift must be explicit.
        assert_eq!(Chord::parse("Ctrl+F").unwrap(), Chord::parse("ctrl+f").unwrap());
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(Chord::parse("").is_err());
        assert!(Chord::parse("hyper+f").unwrap_err().contains("modifier"));
        assert!(Chord::parse("ctrl+").unwrap_err().contains("plus"));
        assert!(Chord::parse("f99").is_err());
        assert!(Chord::parse("ctrl+banana").unwrap_err().contains("banana"));
    }

    #[test]
    fn events_normalise_like_config_keys() {
        let map = Keymap::default();
        let check = |code, mods, expected| assert_eq!(map.action(&ev(code, mods)), expected, "{code:?} {mods:?}");
        check(KeyCode::Char('f'), KeyModifiers::CONTROL, Some(Action::Find));
        check(KeyCode::Char('1'), KeyModifiers::ALT, Some(Action::TabChat));
        check(KeyCode::F(5), KeyModifiers::NONE, Some(Action::TabLogs));
        check(KeyCode::Up, KeyModifiers::SHIFT, Some(Action::ScrollUp));
        check(KeyCode::Char('f'), KeyModifiers::NONE, None);
        // Some terminals report ctrl+shift+f as 'F'.
        let upper = Chord::from_event(&ev(KeyCode::Char('F'), KeyModifiers::CONTROL));
        assert_eq!(upper, Chord::parse("ctrl+shift+f").unwrap());
        assert_eq!(Chord::from_event(&ev(KeyCode::BackTab, KeyModifiers::SHIFT)), Chord::parse("shift+tab").unwrap());
        assert_eq!(Chord::from_event(&ev(KeyCode::Char('?'), KeyModifiers::SHIFT)), Chord::parse("?").unwrap());
    }

    #[test]
    fn typing_and_editing_keys_are_reserved() {
        for s in
            ["a", "shift+a", "enter", "up", "esc", "tab", "ctrl+c", "ctrl+w", "ctrl+u", "ctrl+left", "alt+backspace"]
        {
            assert!(Chord::parse(s).unwrap().reserved().is_some(), "{s} should be reserved");
        }
        for s in ["ctrl+g", "alt+a", "f9", "pgup", "shift+up", "ctrl+enter", "alt+enter"] {
            assert_eq!(Chord::parse(s).unwrap().reserved(), None, "{s} should be allowed");
        }
    }

    #[test]
    fn short_hints() {
        assert_eq!(Chord::parse("ctrl+f").unwrap().short(), "^F");
        assert_eq!(Chord::parse("alt+f").unwrap().short(), "alt+f");
        assert_eq!(Chord::parse("f1").unwrap().short(), "F1");
        assert_eq!(Chord::parse("shift+f1").unwrap().short(), "shift+f1");
        let map = Keymap::build(&over(&[("block", &[])])).0;
        assert_eq!(map.hint(Action::Block), None);
        assert_eq!(map.label(Action::Block), "(block)");
    }

    #[test]
    fn overrides_replace_defaults_and_win_conflicts() {
        let (map, warnings) = Keymap::build(&over(&[("find", &["alt+f", "f9"]), ("block", &["ctrl+d"])]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(map.action(&ev(KeyCode::Char('f'), KeyModifiers::ALT)), Some(Action::Find));
        assert_eq!(map.action(&ev(KeyCode::Char('f'), KeyModifiers::CONTROL)), None, "old default is gone");
        // block took ctrl+d from leave's defaults, leaving leave unbound.
        assert_eq!(map.action(&ev(KeyCode::Char('d'), KeyModifiers::CONTROL)), Some(Action::Block));
        assert!(map.keys(Action::Leave).is_empty());
    }

    #[test]
    fn bad_overrides_warn_and_are_skipped() {
        let (map, warnings) =
            Keymap::build(&over(&[("fnid", &["alt+f"]), ("find", &["q", "ctrl+nope", "alt+g"]), ("quit", &["alt+g"])]));
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("unknown action `fnid`")));
        assert!(warnings.iter().any(|w| w.contains("needed for typing")));
        assert!(warnings.iter().any(|w| w.contains("nope")));
        assert!(warnings.iter().any(|w| w.contains("set for both")));
        assert_eq!(map.keys(Action::Find), &[Chord::parse("alt+g").unwrap()]);
    }

    #[test]
    fn single_strings_and_empty_strings_work() {
        let src = "find = \"alt+f\"\nblock = \"\"\nleave = [\"ctrl+l\", \"f9\"]";
        let o: Overrides = toml::from_str(src).unwrap();
        let (map, warnings) = Keymap::build(&o);
        assert!(warnings.is_empty());
        assert_eq!(map.hint(Action::Find).as_deref(), Some("alt+f"));
        assert!(map.keys(Action::Block).is_empty());
        assert_eq!(map.keys(Action::Leave).len(), 2);
    }

    #[test]
    fn bind_steals_and_overrides_round_trip() {
        let mut map = Keymap::default();
        let ctrl_b = Chord::parse("ctrl+b").unwrap();
        assert_eq!(map.bind(Action::Find, ctrl_b), Some(Action::Block));
        assert!(map.keys(Action::Block).is_empty());
        assert_eq!(map.action(&ev(KeyCode::Char('f'), KeyModifiers::CONTROL)), None);
        assert_eq!(map.bind(Action::Find, ctrl_b), None, "rebinding the same key is a no-op");

        let saved = map.overrides();
        assert_eq!(saved.keys().collect::<Vec<_>>(), vec!["block", "find"]);
        assert_eq!(Keymap::build(&saved).0, map);

        map.reset(Action::Find);
        map.reset(Action::Block);
        assert_eq!(map, Keymap::default());
        assert!(map.overrides().is_empty());
    }

    #[test]
    fn reset_skips_defaults_now_used_elsewhere() {
        let mut map = Keymap::default();
        map.bind(Action::Theme, Chord::parse("ctrl+f").unwrap());
        map.reset(Action::Find);
        assert!(map.keys(Action::Find).is_empty(), "ctrl+f belongs to theme now");
        map.unbind(Action::Theme);
        map.reset(Action::Find);
        assert_eq!(map.hint(Action::Find).as_deref(), Some("^F"));
    }
}
