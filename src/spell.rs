//! Spellchecking the message box with Hunspell dictionaries.
//!
//! A dictionary installed on the system (or dropped into yap's config folder) is used
//! when there is one; otherwise yap falls back to its own US English dictionary.

use crate::links::find_links;
use std::path::{Path, PathBuf};
use unicode_segmentation::UnicodeSegmentation;

const BUNDLED_AFF: &str = include_str!("../assets/dictionaries/en_US.aff");
const BUNDLED_DIC: &str = include_str!("../assets/dictionaries/en_US.dic");

pub struct Speller {
    dict: spellbook::Dictionary,
    /// Where the dictionary came from, for the settings screen.
    pub source: String,
}

impl std::fmt::Debug for Speller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speller").field("source", &self.source).finish()
    }
}

/// Folders searched for `<language>.aff` / `.dic`, most specific first.
fn search_dirs(config_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![config_dir.join("dictionaries")];
    if let Some(data) = directories::BaseDirs::new().map(|d| d.data_dir().to_owned()) {
        dirs.push(data.join("hunspell"));
    }
    dirs.extend(["/usr/share/hunspell", "/usr/share/myspell/dicts", "/usr/share/myspell"].map(PathBuf::from));
    dirs
}

impl Speller {
    /// Load `language` (like `en_US`), from `config_dir/dictionaries` or the system,
    /// falling back to the bundled English dictionary for any `en` language.
    pub fn load(language: &str, config_dir: &Path) -> Result<Self, String> {
        for dir in search_dirs(config_dir) {
            let (aff, dic) = (dir.join(format!("{language}.aff")), dir.join(format!("{language}.dic")));
            if let (Ok(aff_src), Ok(dic_src)) = (std::fs::read_to_string(&aff), std::fs::read_to_string(&dic)) {
                return Self::from_sources(&aff_src, &dic_src, dic.display().to_string());
            }
        }
        if language.starts_with("en") {
            return Self::bundled();
        }
        Err(format!(
            "no {language} dictionary found; put {language}.aff and {language}.dic in {}",
            config_dir.join("dictionaries").display()
        ))
    }

    pub fn bundled() -> Result<Self, String> {
        Self::from_sources(BUNDLED_AFF, BUNDLED_DIC, "built-in en_US".into())
    }

    fn from_sources(aff: &str, dic: &str, source: String) -> Result<Self, String> {
        let mut dict = spellbook::Dictionary::new(aff, dic).map_err(|e| format!("{source}: {e}"))?;
        // Words this site uses all the time that no general dictionary knows.
        for word in crate::catalog::SPECIES.iter().chain(crate::catalog::KINKS).flat_map(|s| s.split_whitespace()) {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if !word.is_empty() && !dict.check(word) {
                let _ = dict.add(word);
            }
        }
        Ok(Speller { dict, source })
    }

    /// Accept `word` from now on (your own list, character names, nicknames).
    pub fn learn(&mut self, word: &str) {
        for w in word.split_whitespace() {
            if !self.dict.check(w) {
                let _ = self.dict.add(w);
            }
        }
    }

    pub fn check(&self, word: &str) -> bool {
        self.dict.check(word)
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.dict.suggest(word, &mut out);
        out.truncate(8);
        out
    }

    /// Byte ranges of misspelled words in `text`. Links, `:shortcodes:`, words with
    /// digits and the word still being typed at `cursor` are left alone.
    pub fn misspelled(&self, text: &str, cursor: usize) -> Vec<(usize, usize)> {
        if text.starts_with('/') && !text.starts_with("//") {
            return Vec::new();
        }
        let links = find_links(text);
        let skip = |start: usize, end: usize| {
            links.iter().any(|l| start < l.end && l.start < end)
                || end == cursor && cursor == text.len()
                || text[..start].ends_with(':') && text[end..].starts_with(':')
                || text[..start].ends_with(['@', '#', ';'])
        };
        text.unicode_word_indices()
            .map(|(start, word)| (start, start + word.len(), word))
            .filter(|&(start, end, word)| {
                word.chars().count() > 1
                    && word.chars().any(char::is_alphabetic)
                    && !word.chars().any(|c| c.is_ascii_digit())
                    && !skip(start, end)
                    && !self.check(word)
            })
            .map(|(start, end, _)| (start, end))
            .collect()
    }
}

/// Languages wooorm/dictionaries has, for `/dict get` suggestions. Others work too.
pub const DOWNLOADABLE: &[&str] = &[
    "en-GB", "en-AU", "en-CA", "en-ZA", "de", "de-AT", "de-CH", "es", "es-MX", "fr", "it", "nl", "pt", "pt-PT", "sv",
    "da", "nb", "nn", "fi", "pl", "cs", "ru", "uk", "tr", "ro", "hu", "el", "ca", "gl", "eu",
];

const DICTIONARIES_URL: &str = "https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries";

/// Download `language` from wooorm/dictionaries into `dir` as `<language>.aff` /
/// `.dic` (plus its licence). Blocking. Returns the licence name.
pub fn download(language: &str, dir: &Path) -> Result<String, String> {
    if language.is_empty() || !language.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("`{language}` isn't a dictionary name (try en-GB or de)"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .user_agent(format!("yap/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let get = |file: &str| -> Result<String, String> {
        let url = format!("{DICTIONARIES_URL}/{language}/{file}");
        let mut response = agent.get(&url).call().map_err(|e| match e {
            ureq::Error::StatusCode(404) => format!("there's no `{language}` dictionary to download"),
            e => e.to_string(),
        })?;
        response.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())
    };
    let (aff, dic) = (get("index.aff")?, get("index.dic")?);
    // Make sure it loads before keeping it.
    Speller::from_sources(&aff, &dic, language.to_owned())?;
    let package = get("package.json").unwrap_or_default();
    let license = serde_json::from_str::<serde_json::Value>(&package)
        .ok()
        .and_then(|v| v.get("license")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "see its licence file".into());
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let write = |ext: &str, body: &str| std::fs::write(dir.join(format!("{language}.{ext}")), body);
    write("aff", &aff).and_then(|()| write("dic", &dic)).map_err(|e| e.to_string())?;
    if let Ok(text) = get("license") {
        let _ = write("LICENSE", &text);
    }
    Ok(license)
}

/// Dictionaries in `dir`, by name.
pub fn installed(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.path().file_name()?.to_str()?.strip_suffix(".dic").map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// The word at or just before `cursor`, as a byte range.
pub fn word_at(text: &str, cursor: usize) -> Option<(usize, usize)> {
    text.unicode_word_indices().map(|(start, word)| (start, start + word.len())).rfind(|&(start, _)| start <= cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speller() -> Speller {
        Speller::bundled().unwrap()
    }

    #[test]
    fn finds_misspellings_but_not_links_codes_or_the_word_being_typed() {
        let s = speller();
        let text = "helo there, see https://e621.net/posts/1 :smile: #reff teh";
        let bad: Vec<&str> = s.misspelled(text, 0).iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(bad, ["helo", "teh"]);
        assert_eq!(s.misspelled(text, text.len()).len(), 1, "the word at the end cursor is still being typed");
        assert!(s.misspelled("/nick Zephyrine", 0).is_empty(), "commands aren't checked");
    }

    #[test]
    fn knows_site_words_and_learns_more() {
        let mut s = speller();
        assert!(s.check("Protogen"));
        assert!(s.check("Vorarephilia"));
        assert!(!s.check("Zephyrine"));
        s.learn("Zephyrine");
        assert!(s.check("Zephyrine"));
        assert!(s.suggest("helo").iter().any(|w| w == "hello"));
    }

    #[test]
    #[ignore = "downloads from GitHub"]
    fn downloads_a_dictionary() {
        let dir = tempfile::tempdir().unwrap();
        let license = download("en-GB", dir.path()).unwrap();
        assert!(!license.is_empty());
        assert_eq!(installed(dir.path()), ["en-GB"]);
        let gb = Speller::from_sources(
            &std::fs::read_to_string(dir.path().join("en-GB.aff")).unwrap(),
            &std::fs::read_to_string(dir.path().join("en-GB.dic")).unwrap(),
            "en-GB".into(),
        )
        .unwrap();
        assert!(gb.check("colour"));
        assert!(download("../etc", dir.path()).is_err());
        assert!(download("xx-nope", dir.path()).unwrap_err().contains("no `xx-nope`"));
    }

    #[test]
    fn word_at_cursor() {
        assert_eq!(word_at("hi thre", 7), Some((3, 7)));
        assert_eq!(word_at("hi thre", 4), Some((3, 7)));
        assert_eq!(word_at("", 0), None);
    }
}
