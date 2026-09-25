//! A text editor for the message box and prompts: grapheme-aware cursor movement,
//! history, and undo.
//!
//! The web client uses a plain `<input type=text>`, so messages are sent as one line.
//! The message box may hold paragraph breaks (`\n`) while you write; they're joined
//! with your paragraph separator when sent. Prompts stay single-line.

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
    /// Keep `\n` as a paragraph break instead of turning it into a space.
    paragraphs: bool,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// What the last edit was, so runs of typing undo as one step.
    last_edit: Option<EditKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Deleting,
    Other,
}

const UNDO_LIMIT: usize = 200;

impl LineEditor {
    pub fn new() -> Self {
        Self::default()
    }

    /// An editor that keeps paragraph breaks (for the message box).
    pub fn with_paragraphs() -> Self {
        LineEditor { paragraphs: true, ..Self::default() }
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
        self.checkpoint(EditKind::Other);
        self.text = self.flatten(text);
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
        self.undo.clear();
        self.redo.clear();
        self.last_edit = None;
        if !text.trim().is_empty() && self.history.last() != Some(&text) {
            self.history.push(text.clone());
            if self.history.len() > 200 {
                self.history.remove(0);
            }
        }
        text
    }

    pub fn insert_char(&mut self, c: char) {
        // A space ends a word, so undo goes back a word at a time.
        self.checkpoint(if c.is_whitespace() { EditKind::Other } else { EditKind::Typing });
        let mut buf = [0; 4];
        self.insert_raw(c.encode_utf8(&mut buf));
    }

    /// Insert text at the cursor. Newlines (e.g. from a paste) become paragraph breaks
    /// in the message box and spaces elsewhere.
    pub fn insert_str(&mut self, s: &str) {
        self.checkpoint(EditKind::Other);
        self.insert_raw(s);
    }

    fn insert_raw(&mut self, s: &str) {
        let s = self.flatten(s);
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }

    /// Alt-Enter: start a new paragraph (a space where paragraphs aren't allowed).
    pub fn new_paragraph(&mut self) {
        self.insert_str("\n");
    }

    /// Remember the current state before an edit of `kind`. Consecutive typing (or
    /// deleting) shares one undo step.
    fn checkpoint(&mut self, kind: EditKind) {
        let grouped = kind != EditKind::Other && self.last_edit == Some(kind);
        let same = self.undo.last().is_some_and(|(t, _)| *t == self.text);
        if !grouped && !same {
            self.undo.push((self.text.clone(), self.cursor));
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = Some(kind);
    }

    /// Ctrl-Z. Returns false with nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some((text, cursor)) = self.undo.pop() else { return false };
        self.redo.push((std::mem::replace(&mut self.text, text), self.cursor));
        self.cursor = cursor;
        self.last_edit = None;
        true
    }

    /// Ctrl-Y. Returns false with nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some((text, cursor)) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.text, text), self.cursor));
        self.cursor = cursor;
        self.last_edit = None;
        true
    }

    /// Replace the text from byte offset `start` up to the cursor.
    pub fn replace_to_cursor(&mut self, start: usize, with: &str) {
        self.replace(start, self.cursor, with);
    }

    /// Replace bytes `start..end`, leaving the cursor just after the new text.
    pub fn replace(&mut self, start: usize, end: usize, with: &str) {
        self.checkpoint(EditKind::Other);
        let with = self.flatten(with);
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
            self.checkpoint(EditKind::Deleting);
            let start = self.prev_boundary(self.cursor);
            self.text.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
    }

    pub fn delete(&mut self) {
        let end = self.next_boundary(self.cursor);
        if end > self.cursor {
            self.checkpoint(EditKind::Deleting);
        }
        self.text.replace_range(self.cursor..end, "");
    }

    pub fn left(&mut self) {
        self.last_edit = None;
        self.cursor = self.prev_boundary(self.cursor);
    }

    pub fn right(&mut self) {
        self.last_edit = None;
        self.cursor = self.next_boundary(self.cursor);
    }

    pub fn word_left(&mut self) {
        self.last_edit = None;
        self.cursor = self.word_start_before(self.cursor);
    }

    pub fn word_right(&mut self) {
        self.last_edit = None;
        self.cursor = self.word_end_after(self.cursor);
    }

    pub fn home(&mut self) {
        self.last_edit = None;
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.last_edit = None;
        self.cursor = self.text.len();
    }

    /// Ctrl-W: delete the word before the cursor.
    pub fn delete_word_back(&mut self) {
        self.checkpoint(EditKind::Other);
        let start = self.word_start_before(self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Ctrl-U: delete everything before the cursor.
    pub fn delete_to_start(&mut self) {
        self.checkpoint(EditKind::Other);
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
            if g == "\n" {
                if i == self.cursor {
                    cursor = (rows.len() - 1, row_w);
                }
                rows.push(String::new());
                row_w = 0;
                continue;
            }
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

impl LineEditor {
    /// Clean text coming in: control characters go, and newlines become paragraph
    /// breaks or spaces.
    fn flatten(&self, s: &str) -> String {
        let s = crate::text::sanitize(&s.replace("\r\n", "\n").replace('\r', "\n"));
        if self.paragraphs { s } else { s.replace('\n', " ") }
    }

    /// The text as it will be sent: paragraphs joined with `separator`.
    pub fn joined(&self, separator: &str) -> String {
        join_lines(&self.text, separator)
    }
}

/// Join `\n`-separated paragraphs with `separator`, dropping empty ones.
pub fn join_lines(text: &str, separator: &str) -> String {
    if !text.contains('\n') {
        return text.to_owned();
    }
    text.split('\n').map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(separator)
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
        let mut e = LineEditor::with_paragraphs();
        e.insert_str("line one\r\nline two");
        assert_eq!(e.text(), "line one\nline two", "the message box keeps them as paragraphs");
    }

    #[test]
    fn paragraphs_lay_out_on_their_own_rows_and_join_for_sending() {
        let mut e = LineEditor::with_paragraphs();
        e.insert_str("first");
        e.new_paragraph();
        e.insert_str("second");
        let (rows, cursor) = e.layout(40);
        assert_eq!(rows, ["first", "second"]);
        assert_eq!(cursor, (1, 6));
        assert_eq!(e.joined(" / "), "first / second");
        assert_eq!(join_lines("a\n\n b \n", " / "), "a / b");
        let mut prompt = LineEditor::new();
        prompt.new_paragraph();
        assert_eq!(prompt.text(), " ", "prompts stay on one line");
    }

    #[test]
    fn undo_goes_back_a_word_at_a_time_and_redo_returns() {
        let mut e = typed("hello big world");
        assert!(e.undo());
        assert_eq!(e.text(), "hello big ");
        assert!(e.undo());
        assert_eq!(e.text(), "hello big");
        e.delete_to_start();
        assert_eq!(e.text(), "");
        assert!(e.undo(), "an accidental ctrl+u comes back");
        assert_eq!(e.text(), "hello big");
        assert!(e.redo());
        assert_eq!(e.text(), "");
        assert!(e.undo());
        e.insert_char('!');
        assert!(!e.redo(), "typing after undo drops the redo history");
        e.submit();
        assert!(!e.undo(), "sending starts afresh");
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
