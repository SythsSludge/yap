//! A small fuzzy matcher for the command palette.

/// How well `query` matches `candidate`: its characters must appear in order,
/// ignoring case and spaces in the query. Higher is better; `None` means no match.
///
/// Matches at word starts and runs of consecutive characters score higher, gaps cost a
/// little, and containing the query outright is best of all.
pub fn score(query: &str, candidate: &str) -> Option<i64> {
    let query: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = candidate.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut qi = 0;
    let mut last_match: Option<usize> = None;
    for (i, &c) in chars.iter().enumerate() {
        if qi == query.len() {
            break;
        }
        if c != query[qi] {
            continue;
        }
        score += 1;
        let word_start = i == 0 || !chars[i - 1].is_alphanumeric();
        if word_start {
            score += 6;
        }
        match last_match {
            Some(prev) if prev + 1 == i => score += 4,
            Some(prev) => score -= (i - prev - 1).min(5) as i64,
            None => score -= i.min(8) as i64 / 2,
        }
        last_match = Some(i);
        qi += 1;
    }
    if qi < query.len() {
        return None;
    }
    let needle: String = query.iter().collect();
    if candidate.to_lowercase().contains(&needle) {
        score += 15;
    }
    // Prefer shorter candidates when everything else is equal.
    Some(score * 10 - chars.len().min(60) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn best<'a>(query: &str, options: &[&'a str]) -> Vec<&'a str> {
        let mut scored: Vec<(i64, &str)> = options.iter().filter_map(|o| score(query, o).map(|s| (s, *o))).collect();
        scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        scored.into_iter().map(|(_, o)| o).collect()
    }

    #[test]
    fn requires_characters_in_order() {
        assert!(score("fnd", "Find a partner").is_some());
        assert!(score("dnf", "Find a partner").is_none());
        assert!(score("xyz", "Find a partner").is_none());
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("find partner", "Find a partner").is_some(), "spaces in the query are ignored");
    }

    #[test]
    fn prefers_word_starts_and_substrings() {
        let options = ["Theme: dracula", "Transparent background", "Toggle the sidebar"];
        assert_eq!(best("theme", &options)[0], "Theme: dracula");
        assert_eq!(best("tb", &options)[0], "Transparent background", "initials of words");
        let options = ["Skip to the next partner", "Snippet: intro", "Search this chat"];
        assert_eq!(best("snip", &options)[0], "Snippet: intro");
        assert_eq!(best("next", &options)[0], "Skip to the next partner");
    }

    #[test]
    fn ignores_case_and_handles_unicode() {
        assert!(score("ÉCL", "éclair").is_some());
        assert!(score("fox", "🦊 fox").is_some());
    }
}
