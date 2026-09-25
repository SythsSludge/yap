//! Spellchecking the message box: loading the dictionary and fixing words.

use super::*;
use crate::spell::Speller;

impl App {
    /// (Re)load the dictionary for the current settings, teaching it your words and
    /// your characters' names.
    pub fn load_speller(&mut self) {
        let s = &self.config.settings;
        if !s.spellcheck {
            self.speller = None;
            return;
        }
        let config_dir = self.paths.config_file.parent().map(std::path::Path::to_owned).unwrap_or_default();
        match Speller::load(&s.spell_language, &config_dir) {
            Ok(mut speller) => {
                for word in s.spell_words.iter().chain(self.config.profiles.iter().map(|p| &p.character)) {
                    speller.learn(word);
                }
                self.speller = Some(speller);
            }
            Err(e) => {
                self.speller = None;
                self.toast(Level::Warning, format!("Spellcheck: {e}"));
            }
        }
    }

    /// Misspelled words in the message box, as byte ranges.
    pub fn misspelled(&self) -> Vec<(usize, usize)> {
        match &self.speller {
            Some(s) => s.misspelled(self.input.text(), self.input.cursor()),
            None => Vec::new(),
        }
    }

    /// alt+s: fixes for the misspelled word at the cursor, or else the nearest one before it.
    pub fn open_spelling(&mut self) {
        let Some(speller) = &self.speller else {
            return self.toast(Level::Info, "Spellcheck is off (Settings → Spellcheck).");
        };
        let text = self.input.text();
        let cursor = self.input.cursor();
        let bad = speller.misspelled(text, usize::MAX);
        let at_cursor = crate::spell::word_at(text, cursor).filter(|w| bad.contains(w));
        let Some((start, end)) = at_cursor.or_else(|| bad.iter().rev().find(|(s, _)| *s <= cursor).copied()) else {
            return self.toast(Level::Info, "No misspelled words before the cursor.");
        };
        let word = text[start..end].to_owned();
        let suggestions = speller.suggest(&word);
        self.modal = Some(Modal::Spelling { word, start, end, suggestions, selected: 0 });
    }

    /// Apply the chosen row of the spelling popup.
    pub(super) fn apply_spelling(&mut self, word: &str, start: usize, end: usize, choice: Option<&str>) {
        match choice {
            Some(fix) => {
                let cursor = self.input.cursor();
                self.input.replace(start, end, fix);
                // Put the cursor back where it was, shifted by the change in length.
                let back = if cursor >= end { cursor + fix.len() - (end - start) } else { cursor.min(start) };
                self.input.set_cursor(back);
                self.input_changed();
            }
            None => {
                if let Some(s) = &mut self.speller {
                    s.learn(word);
                }
                if !self.config.settings.spell_words.iter().any(|w| w == word) {
                    self.config.settings.spell_words.push(word.to_owned());
                    self.config_changed();
                }
                self.toast(Level::Success, format!("Added `{word}` to your dictionary."));
            }
        }
    }
}
