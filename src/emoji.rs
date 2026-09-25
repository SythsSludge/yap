//! `:smile:` style emoji shortcodes (the GitHub / Slack names).

use std::sync::OnceLock;

/// Every shortcode with its emoji, shortest first so the likeliest match comes first.
///
/// Sequences joined with a zero-width joiner are left out: terminals disagree about how
/// wide they are, which would throw off wrapping and the cursor.
fn table() -> &'static [(&'static str, &'static str)] {
    static TABLE: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut all: Vec<_> = emojis::iter()
            .filter(|e| !e.as_str().contains('\u{200d}'))
            .flat_map(|e| e.shortcodes().map(move |code| (code, e.as_str())))
            .collect();
        all.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(a.0.cmp(b.0)));
        all
    })
}

fn lookup(code: &str) -> Option<&'static str> {
    table().iter().find(|(c, _)| *c == code).map(|(_, e)| *e)
}

fn is_code_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '+' | '-')
}

/// Replace known `:shortcodes:` with their emoji. Unknown ones and anything inside a
/// link are left as they are.
pub fn expand(text: &str) -> String {
    let links = crate::links::find_links(text);
    let in_link = |at: usize| links.iter().any(|l| (l.start..l.end).contains(&at));
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    let mut from = 0;
    while let Some(open) = text[from..].find(':').map(|i| from + i) {
        let Some(close) = text[open + 1..].find(':').map(|i| open + 1 + i) else { break };
        let code = &text[open + 1..close];
        match lookup(code) {
            Some(emoji) if code.chars().all(is_code_char) && !in_link(open) => {
                out.push_str(&text[rest..open]);
                out.push_str(emoji);
                rest = close + 1;
                from = close + 1;
            }
            // The closing colon may open the next code, as in `10:30:smile:`.
            _ => from = close,
        }
    }
    out.push_str(&text[rest..]);
    out
}

/// A shortcode being typed just before `cursor`: its byte offset (at the colon) and
/// the letters so far. Needs at least two letters, so a plain `:` or `:)` never counts.
pub fn partial(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = &text[..cursor];
    let open = before.rfind(':')?;
    let word = &before[open + 1..];
    let starts_word = before[..open].chars().next_back().is_none_or(char::is_whitespace);
    (starts_word && word.len() >= 2 && word.chars().all(is_code_char)).then_some((open, word))
}

/// Shortcodes starting with `prefix`, then ones containing it: `(code, emoji)`.
pub fn suggest(prefix: &str, limit: usize) -> Vec<(&'static str, &'static str)> {
    let table = table();
    let starts = table.iter().filter(|(c, _)| c.starts_with(prefix));
    let contains = table.iter().filter(|(c, _)| !c.starts_with(prefix) && c.contains(prefix));
    starts.chain(contains).take(limit).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_codes_only() {
        assert_eq!(expand("hi :smile: :wave:"), "hi 😄 👋");
        assert_eq!(expand(":nope: stays"), ":nope: stays");
        assert_eq!(expand("at 10:30:smile:"), "at 10:30😄");
        assert_eq!(expand("a : b : c"), "a : b : c");
        assert_eq!(expand(":+1::heart:"), "👍❤️");
    }

    #[test]
    fn links_are_left_alone() {
        let url = "https://example.com/a:smile:b";
        assert_eq!(expand(&format!("see {url} :smile:")), format!("see {url} 😄"));
    }

    #[test]
    fn partial_codes_at_the_cursor() {
        assert_eq!(partial("hi :smi", 7), Some((3, "smi")));
        assert_eq!(partial(":sm", 3), Some((0, "sm")));
        assert_eq!(partial("hi :s", 5), None, "one letter isn't enough");
        assert_eq!(partial("10:30", 5), None, "not at the start of a word");
        assert_eq!(partial("hi :smile: ok", 13), None);
        assert_eq!(partial("hi :smi later", 7), Some((3, "smi")));
    }

    #[test]
    fn suggestions_prefer_prefixes() {
        let s = suggest("smil", 3);
        assert_eq!(s[0], ("smile", "😄"));
        assert!(s.iter().all(|(c, _)| c.starts_with("smil")));
        assert!(suggest("zzzz", 5).is_empty());
    }
}
