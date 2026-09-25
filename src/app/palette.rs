//! The command palette: one fuzzy search over everything you can do.

use super::settings::{self, Row};
use super::*;
use crate::keymap::Action;

/// Something the palette can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteItem {
    Action(Action),
    /// A slash command, by its help syntax (e.g. `/snip [name]`).
    Command(&'static str),
    Setting(Row),
    Profile(String),
    Theme(String),
    Snippet(usize),
    Chat(u64),
}

/// A palette row: what it does, what it says, and a hint on the right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub item: PaletteItem,
    pub label: String,
    pub kind: &'static str,
    pub hint: String,
}

const MAX_RESULTS: usize = 60;

impl App {
    fn palette_entries(&self) -> Vec<PaletteEntry> {
        let mut out = Vec::new();
        for &a in Action::ALL {
            if a == Action::Palette {
                continue;
            }
            out.push(PaletteEntry {
                item: PaletteItem::Action(a),
                label: a.describe().to_owned(),
                kind: "action",
                hint: self.keymap.hint(a).unwrap_or_default(),
            });
        }
        for (cmd, what) in crate::commands::HELP {
            if cmd.starts_with("//") {
                continue;
            }
            out.push(PaletteEntry {
                item: PaletteItem::Command(cmd),
                label: format!("{cmd}  {what}"),
                kind: "command",
                hint: String::new(),
            });
        }
        let s = &self.config.settings;
        for row in settings::rows(s) {
            if matches!(row, Row::Domain(_) | Row::Key(_)) {
                continue;
            }
            out.push(PaletteEntry {
                item: PaletteItem::Setting(row),
                label: format!("{}: {}", row.section().split(" (").next().unwrap_or_default(), row.label(s)),
                kind: "setting",
                hint: row.value(s),
            });
        }
        for p in &self.config.profiles {
            out.push(PaletteEntry {
                item: PaletteItem::Profile(p.name.clone()),
                label: format!("Use profile {}", p.name),
                kind: "profile",
                hint: if p.name == self.config.active_profile { "active".into() } else { String::new() },
            });
        }
        for th in &self.themes {
            out.push(PaletteEntry {
                item: PaletteItem::Theme(th.name.clone()),
                label: format!("Theme {}", th.name),
                kind: "theme",
                hint: if th.name == self.theme.name { "current".into() } else { String::new() },
            });
        }
        for (i, sn) in self.drawer.snippets.iter().enumerate() {
            out.push(PaletteEntry {
                item: PaletteItem::Snippet(i),
                label: format!("Insert snippet {}", sn.name),
                kind: "snippet",
                hint: crate::text::truncate(&sn.text.replace('\n', " "), 30),
            });
        }
        if self.session_count() > 1 {
            for s in self.sessions() {
                out.push(PaletteEntry {
                    item: PaletteItem::Chat(s.id),
                    label: format!("Go to chat {}: {}", s.number, s.label),
                    kind: "chat",
                    hint: if s.unseen > 0 { format!("{} unread", s.unseen) } else { String::new() },
                });
            }
        }
        out
    }

    /// Entries matching `query`, best first.
    pub fn palette_matches(&self, query: &str) -> Vec<PaletteEntry> {
        let mut scored: Vec<(i64, usize, PaletteEntry)> = self
            .palette_entries()
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let haystack = format!("{} {}", e.label, e.kind);
                crate::fuzzy::score(query, &haystack).map(|s| (s, i, e))
            })
            .collect();
        // Best score first; ties keep their natural order.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(MAX_RESULTS).map(|(_, _, e)| e).collect()
    }

    pub fn open_palette(&mut self) {
        self.modal = Some(Modal::Palette { query: String::new(), selected: 0 });
    }

    pub fn run_palette_item(&mut self, item: PaletteItem) {
        match item {
            PaletteItem::Action(a) => self.run_action(a),
            PaletteItem::Command(syntax) => {
                let name = syntax.split_whitespace().next().unwrap_or(syntax);
                if syntax.contains('<') {
                    // Needs an argument: start typing it in the message box.
                    self.tab = Tab::Chat;
                    self.chat_focus = ChatFocus::Input;
                    self.input.set(&format!("{name} "));
                } else if let crate::commands::Parsed::Command(cmd) = crate::commands::parse(name) {
                    self.run_command(cmd);
                }
            }
            PaletteItem::Setting(row) => {
                self.tab = Tab::Settings;
                self.settings_ui.selected =
                    settings::rows(&self.config.settings).iter().position(|r| *r == row).unwrap_or(0);
            }
            PaletteItem::Profile(name) => self.switch_profile(&name),
            PaletteItem::Theme(name) => self.set_theme(&name, true),
            PaletteItem::Snippet(i) => self.insert_snippet(i),
            PaletteItem::Chat(id) => self.switch_session(id),
        }
    }
}
