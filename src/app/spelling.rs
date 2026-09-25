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

    fn dictionaries_dir(&self) -> PathBuf {
        self.paths.config_file.parent().map(|d| d.join("dictionaries")).unwrap_or_default()
    }

    /// `/dict`, `/dict get en-GB`, `/dict use en-GB`.
    pub fn dict_command(&mut self, args: &str) {
        let (verb, lang) = args.split_once(' ').map_or((args, ""), |(v, l)| (v, l.trim()));
        match (verb, lang) {
            ("get" | "download", "") | ("use", "") => self.toast(Level::Error, "Which language? e.g. /dict get en-GB"),
            ("get" | "download", lang) => {
                self.toast(Level::Info, format!("Downloading the {lang} dictionary…"));
                self.effect(Effect::FetchDictionary(lang.to_owned()));
            }
            ("use", lang) => {
                self.config.settings.spell_language = lang.to_owned();
                self.config.settings.spellcheck = true;
                self.config_changed();
                self.load_speller();
                if self.speller.is_some() {
                    self.toast(Level::Success, format!("Spellcheck uses {lang}."));
                }
            }
            ("" | "list", _) => {
                let have = crate::spell::installed(&self.dictionaries_dir());
                let using = self.speller.as_ref().map_or("off".to_owned(), |s| s.source.clone());
                let have = if have.is_empty() { "none downloaded".to_owned() } else { have.join(", ") };
                self.toast(Level::Info, format!("Spellcheck: {using}. Downloaded: {have}. /dict get <lang> adds one."));
            }
            _ => self.toast(Level::Error, "Use /dict, /dict get <lang> or /dict use <lang>."),
        }
    }

    /// A `/dict get` download finished.
    pub fn on_dictionary(&mut self, language: String, result: Result<String, String>) {
        match result {
            Ok(license) => {
                self.config.settings.spell_language = language.clone();
                self.config.settings.spellcheck = true;
                self.config_changed();
                self.load_speller();
                self.toast(
                    Level::Success,
                    format!("Installed {language} (licence: {license}). Spellcheck uses it now."),
                );
            }
            Err(e) => self.toast(Level::Error, format!("Couldn't get the {language} dictionary: {e}")),
        }
    }

    /// Where `/dict get` saves dictionaries.
    pub fn dictionary_dir(&self) -> PathBuf {
        self.dictionaries_dir()
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
