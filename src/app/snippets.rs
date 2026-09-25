//! Snippets (reusable text) and moving the drawer between machines.

use super::*;
use crate::drawer::expand_placeholders;

impl App {
    /// Values for `{placeholders}` in snippets, from your profile and current partner.
    pub fn snippet_vars(&self) -> Vec<(&'static str, String)> {
        let p = &self.config.active().preferences;
        let mut vars = vec![
            ("me", self.my_label()),
            ("profile", self.config.active_profile.clone()),
            ("gender", p.summary(Field::Gender)),
            ("species", p.summary(Field::Species)),
            ("role", p.summary(Field::Role)),
        ];
        if let PartnerState::Connected(info) = &self.partner {
            vars.push(("partner", self.partner_label()));
            vars.extend([
                ("partner_gender", info.gender.clone()),
                ("partner_species", info.species.clone()),
                ("partner_role", info.role.clone()),
            ]);
            if let Some(lang) = &info.language {
                vars.push(("partner_language", lang.clone()));
            }
        }
        vars
    }

    /// A snippet's text as it would be sent: placeholders filled, paragraphs joined.
    pub fn filled_snippet(&self, index: usize) -> Option<String> {
        let snippet = self.drawer.snippets.get(index)?;
        Some(crate::app::join_paragraphs(
            &expand_placeholders(&snippet.text, &self.snippet_vars()),
            &self.config.settings.paragraph_break,
        ))
    }

    /// Put a snippet into the message box, placeholders filled in.
    pub fn insert_snippet(&mut self, index: usize) {
        let Some(text) = self.filled_snippet(index) else { return };
        self.insert_into_input(&text);
    }

    /// Snippets matching a `;name` trigger being typed at the cursor: where the `;` is,
    /// then snippet indices (names starting with it first).
    pub fn snippet_suggestions(&self) -> Option<(usize, Vec<usize>)> {
        let text = self.input.text();
        let before = &text[..self.input.cursor()];
        let open = before.rfind(';')?;
        let typed = before[open + 1..].to_lowercase();
        let starts_word = before[..open].chars().next_back().is_none_or(char::is_whitespace);
        let name_like = typed.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
        if !starts_word || typed.is_empty() || !name_like {
            return None;
        }
        let names = &self.drawer.snippets;
        let starts = (0..names.len()).filter(|&i| names[i].name.starts_with(&typed));
        let contains =
            (0..names.len()).filter(|&i| !names[i].name.starts_with(&typed) && names[i].name.contains(&typed));
        let found: Vec<usize> = starts.chain(contains).take(6).collect();
        (!found.is_empty()).then_some((open, found))
    }

    /// Tab after `;intro`: swap the trigger for the snippet's text.
    pub(super) fn complete_snippet(&mut self) -> bool {
        let Some((start, found)) = self.snippet_suggestions() else { return false };
        let Some(text) = self.filled_snippet(found[0]) else { return false };
        self.input.replace_to_cursor(start, &text);
        true
    }

    pub fn insert_snippet_named(&mut self, name: &str) {
        match self
            .drawer
            .snippets
            .iter()
            .position(|s| Some(&s.name) == crate::drawer::normalize_snippet_name(name).as_ref())
        {
            Some(i) => self.insert_snippet(i),
            None => self.toast(Level::Error, format!("No snippet called `{name}`. /snip lists them.")),
        }
    }

    pub fn open_snippet_picker(&mut self) {
        if self.drawer.snippets.is_empty() {
            self.toast(
                Level::Info,
                "No snippets yet. Add some in the drawer (s switches to snippets), or /snip-add <name> <text>.",
            );
        } else {
            self.modal = Some(Modal::Snippets { filter: String::new(), selected: 0 });
        }
    }

    pub fn add_snippet(&mut self, name: &str, text: &str) -> Option<usize> {
        match self.drawer.add_snippet(name, text) {
            Ok(i) => {
                self.drawer_changed();
                let name = self.drawer.snippets[i].name.clone();
                self.toast(Level::Success, format!("Saved snippet `{name}`. Use it with /snip {name}."));
                Some(i)
            }
            Err(e) => {
                self.toast(Level::Error, e.to_string());
                None
            }
        }
    }

    pub fn edit_snippet_in_editor(&mut self, index: usize) {
        let Some(snippet) = self.drawer.snippets.get(index) else { return };
        let text = snippet.text.clone();
        let command = self.editor_command();
        self.effect(Effect::OpenEditor { command, text, purpose: EditorPurpose::Snippet(index) });
    }

    pub fn export_drawer(&mut self, path: &str) {
        let path = config::expand_tilde(path);
        match self.drawer.export_to(&path) {
            Ok(()) => self.toast(Level::Success, format!("Exported the drawer to {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Export failed: {e:#}")),
        }
    }

    pub fn import_drawer(&mut self, path: &str) {
        let path = config::expand_tilde(path);
        match Drawer::read_import(&path) {
            Ok(other) => {
                let r = self.drawer.merge(other);
                self.drawer_changed();
                let mut msg = format!("Imported {} links and {} snippets", r.links_added, r.snippets_added);
                if r.links_updated > 0 {
                    msg.push_str(&format!("; updated {} existing links", r.links_updated));
                }
                if r.snippets_renamed > 0 {
                    msg.push_str(&format!("; renamed {} clashing snippets", r.snippets_renamed));
                }
                self.toast(Level::Success, msg);
            }
            Err(e) => self.toast(Level::Error, format!("Import failed: {e:#}")),
        }
    }
}
