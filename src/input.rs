//! A single-line text editor with grapheme-aware cursor movement and history.
//!
//! The web client uses a plain `<input type=text>`, so messages never contain newlines;
//! long messages are soft-wrapped for display only.

use crate::text::width;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Default, Clone)]
pub struct LineEditor {
    text: String,
    /// Byte offset, always on a grapheme boundary.
    cursor: usize,
    history: Vec<String>,
    /// Index into `history` while browsing it, with the unsent draft stashed.
    browsing: Option<(usize, String)>,
}

impl LineEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(text: &str) -> Self {
        let mut e = Self::new();
        e.set(text);
        e
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Length in characters, which is what the web client's 3000 limit counts
    /// (approximately: JS counts UTF-16 units).
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Move the cursor to byte `at`, snapped back to a character boundary.
    pub fn set_cursor(&mut self, at: usize) {
        let mut at = at.min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        self.cursor = at;
    }

    pub fn set(&mut self, text: &str) {
        self.text = flatten(text);
        self.cursor = self.text.len();
        self.browsing = None;
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    /// Take the text out, recording it in history.
    pub fn submit(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.browsing = None;
        if !text.trim().is_empty() && self.history.last() != Some(&text) {
            self.history.push(text.clone());
            if self.history.len() > 200 {
                self.history.remove(0);
            }
        }
        text
    }

    pub fn insert_char(&mut self, c: char) {
        let mut buf = [0; 4];
        self.insert_str(c.encode_utf8(&mut buf));
    }

    /// Insert text at the cursor. Newlines (e.g. from a paste) become spaces.
    pub fn insert_str(&mut self, s: &str) {
        let s = flatten(s);
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }

    /// Replace the text from byte offset `start` up to the cursor.
    pub fn replace_to_cursor(&mut self, start: usize, with: &str) {
        self.replace(start, self.cursor, with);
    }

    /// Replace bytes `start..end`, leaving the cursor just after the new text.
    pub fn replace(&mut self, start: usize, end: usize, with: &str) {
        let with = flatten(with);
        self.text.replace_range(start..end, &with);
        self.cursor = start + with.len();
    }

    fn prev_boundary(&self, from: usize) -> usize {
        self.text[..from].grapheme_indices(true).next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, from: usize) -> usize {
        self.text[from..].graphemes(true).next().map_or(from, |g| from + g.len())
    }

    fn word_start_before(&self, from: usize) -> usize {
        let before = &self.text[..from];
        let trimmed = before.trim_end();
        trimmed.unicode_word_indices().next_back().map_or(0, |(i, _)| i).min(trimmed.len())
    }

    fn word_end_after(&self, from: usize) -> usize {
        self.text[from..].unicode_word_indices().next().map_or(self.text.len(), |(i, w)| from + i + w.len())
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let start = self.prev_boundary(self.cursor);
            self.text.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
    }

    pub fn delete(&mut self) {
        let end = self.next_boundary(self.cursor);
        self.text.replace_range(self.cursor..end, "");
    }

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_start_before(self.cursor);
    }

    pub fn word_right(&mut self) {
        self.cursor = self.word_end_after(self.cursor);
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Ctrl-W: delete the word before the cursor.
    pub fn delete_word_back(&mut self) {
        let start = self.word_start_before(self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Ctrl-U: delete everything before the cursor.
    pub fn delete_to_start(&mut self) {
        self.text.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    /// Recall the previous history entry. Returns false if there is none.
    pub fn history_prev(&mut self) -> bool {
        let idx = match &self.browsing {
            Some((0, _)) => return false,
            Some((i, _)) => i - 1,
            None if self.history.is_empty() => return false,
            None => self.history.len() - 1,
        };
        let draft = match self.browsing.take() {
            Some((_, d)) => d,
            None => self.text.clone(),
        };
        self.text = self.history[idx].clone();
        self.cursor = self.text.len();
        self.browsing = Some((idx, draft));
        true
    }

    pub fn history_next(&mut self) -> bool {
        let Some((idx, draft)) = self.browsing.take() else {
            return false;
        };
        if idx + 1 < self.history.len() {
            self.text = self.history[idx + 1].clone();
            self.browsing = Some((idx + 1, draft));
        } else {
            self.text = draft;
        }
        self.cursor = self.text.len();
        true
    }

    /// Soft-wrap into rows of at most `width` cells. Returns the rows and the
    /// (row, column) where the cursor sits.
    pub fn layout(&self, width_cells: usize) -> (Vec<String>, (usize, usize)) {
        let max = width_cells.max(1);
        let mut rows = vec![String::new()];
        let mut row_w = 0;
        let mut cursor = (0, 0);
        for (i, g) in self.text.grapheme_indices(true) {
            let gw = width(g);
            if row_w + gw > max && row_w > 0 {
                rows.push(String::new());
                row_w = 0;
            }
            if i == self.cursor {
                cursor = (rows.len() - 1, row_w);
            }
            rows.last_mut().expect("never empty").push_str(g);
            row_w += gw;
        }
        if self.cursor == self.text.len() {
            if row_w >= max {
                rows.push(String::new());
                row_w = 0;
            }
            cursor = (rows.len() - 1, row_w);
        }
        (rows, cursor)
    }
}

fn flatten(s: &str) -> String {
    crate::text::sanitize(s).replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> LineEditor {
        let mut e = LineEditor::new();
        for c in s.chars() {
            e.insert_char(c);
        }
        e
    }

    #[test]
    fn edits_at_cursor() {
        let mut e = typed("helo");
        e.left();
        e.insert_char('l');
        assert_eq!(e.text(), "hello");
        e.home();
        e.delete();
        assert_eq!(e.text(), "ello");
        e.end();
        e.backspace();
        assert_eq!(e.text(), "ell");
    }

    #[test]
    fn moves_by_grapheme_not_byte() {
        // A flag emoji is two code points; a combining accent attaches to its base.
        let mut e = typed("a🇬🇧e\u{301}");
        e.backspace();
        assert_eq!(e.text(), "a🇬🇧");
        e.left();
        assert_eq!(e.cursor(), 1);
        e.delete();
        assert_eq!(e.text(), "a");
    }

    #[test]
    fn word_motions_and_deletes() {
        let mut e = typed("hello big world");
        e.word_left();
        assert_eq!(&e.text()[e.cursor()..], "world");
        e.word_left();
        assert_eq!(&e.text()[e.cursor()..], "big world");
        e.word_right();
        assert_eq!(&e.text()[e.cursor()..], " world");
        e.end();
        e.delete_word_back();
        assert_eq!(e.text(), "hello big ");
        e.delete_word_back();
        assert_eq!(e.text(), "hello ");
        e.delete_to_start();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn paste_flattens_newlines_and_strips_controls() {
        let mut e = LineEditor::new();
        e.insert_str("line one\nline two\x1b[31m");
        assert_eq!(e.text(), "line one line two[31m");
    }

    #[test]
    fn history_recalls_and_restores_draft() {
        let mut e = LineEditor::new();
        e.insert_str("first");
        e.submit();
        e.insert_str("second");
        e.submit();
        e.insert_str("draft");
        assert!(e.history_prev());
        assert_eq!(e.text(), "second");
        assert!(e.history_prev());
        assert_eq!(e.text(), "first");
        assert!(!e.history_prev());
        assert!(e.history_next());
        assert_eq!(e.text(), "second");
        assert!(e.history_next());
        assert_eq!(e.text(), "draft");
        assert!(!e.history_next());
    }

    #[test]
    fn history_skips_blank_and_repeated_entries() {
        let mut e = LineEditor::new();
        for s in ["same", "same", "   "] {
            e.insert_str(s);
            e.submit();
        }
        assert!(e.history_prev());
        assert!(!e.history_prev());
    }

    #[test]
    fn layout_wraps_and_tracks_cursor() {
        let e = typed("abcdefg");
        assert_eq!(e.layout(3), (vec!["abc".into(), "def".into(), "g".into()], (2, 1)));
        // Cursor at the end of an exactly-full row moves to a fresh row.
        let e = typed("abcdef");
        assert_eq!(e.layout(3), (vec!["abc".into(), "def".into(), String::new()], (2, 0)));
        let mut e = typed("abcdef");
        e.home();
        e.right();
        e.right();
        e.right();
        assert_eq!(e.layout(3).1, (1, 0));
        // Wide chars never split across rows.
        assert_eq!(typed("🦊🦊").layout(3).0, vec!["🦊", "🦊"]);
    }
}
