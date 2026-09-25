//! Snippets (reusable text) and moving the drawer between machines.

use super::*;
use crate::drawer::expand_placeholders;

impl App {
    /// Values for `{placeholders}` in snippets, from your profile and current partner.
    pub fn snippet_vars(&self) -> Vec<(&'static str, String)> {
        let p = &self.config.active().preferences;
        let mut vars = vec![
            ("profile", self.config.active_profile.clone()),
            ("gender", p.summary(Field::Gender)),
            ("species", p.summary(Field::Species)),
            ("role", p.summary(Field::Role)),
        ];
        if let PartnerState::Connected(info) = &self.partner {
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

    /// Put a snippet into the message box, placeholders filled in.
    pub fn insert_snippet(&mut self, index: usize) {
        let Some(snippet) = self.drawer.snippets.get(index) else { return };
        let text = crate::app::join_paragraphs(
            &expand_placeholders(&snippet.text, &self.snippet_vars()),
            &self.config.settings.paragraph_break,
        );
        self.insert_into_input(&text);
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
