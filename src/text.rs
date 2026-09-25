//! Text sanitising and styled word-wrapping.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Make remote text safe to put in terminal cells.
///
/// Anything a partner sends is written to the terminal, so control characters must go:
/// a raw ESC would let them emit escape sequences (set our clipboard via OSC 52, rewrite
/// the window title, move the cursor...). Bidi overrides are stripped too, so a link
/// can't be made to render as something other than where it points.
pub fn sanitize(input: &str) -> String {
    input
        .chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            '\n' => Some('\n'),
            '\r' => None,
            c if c.is_control() => None,
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' => None,
            c => Some(c),
        })
        .collect()
}

/// Display width in terminal cells.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate to at most `max` cells, adding an ellipsis if anything was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for g in s.graphemes(true) {
        let gw = width(g);
        if w + gw > max - 1 {
            break;
        }
        out.push_str(g);
        w += gw;
    }
    out.push('…');
    out
}

/// A run of non-whitespace, possibly spanning several styles (e.g. a link then a comma).
#[derive(Debug, Default)]
struct Word {
    pieces: Vec<(Style, String)>,
    width: usize,
}

enum Token {
    Word(Word),
    Space(Style, String),
    Newline,
}

fn tokenize(spans: &[Span<'_>]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word = Word::default();
    let flush = |word: &mut Word, tokens: &mut Vec<Token>| {
        if !word.pieces.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    for span in spans {
        for g in span.content.graphemes(true) {
            if g == "\n" || g == "\r\n" {
                flush(&mut word, &mut tokens);
                tokens.push(Token::Newline);
            } else if g.chars().all(char::is_whitespace) {
                flush(&mut word, &mut tokens);
                match tokens.last_mut() {
                    Some(Token::Space(style, s)) if *style == span.style => s.push(' '),
                    _ => tokens.push(Token::Space(span.style, " ".into())),
                }
            } else {
                match word.pieces.last_mut() {
                    Some((style, s)) if *style == span.style => s.push_str(g),
                    _ => word.pieces.push((span.style, g.to_owned())),
                }
                word.width += width(g);
            }
        }
    }
    flush(&mut word, &mut tokens);
    tokens
}

struct Builder {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    used: usize,
    width: usize,
    indent: usize,
}

impl Builder {
    fn available(&self) -> usize {
        self.width.saturating_sub(self.used)
    }

    fn at_line_start(&self) -> bool {
        self.used <= self.indent
    }

    fn push(&mut self, style: Style, s: &str, w: usize) {
        match self.current.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(s),
            _ => self.current.push(Span::styled(s.to_owned(), style)),
        }
        self.used += w;
    }

    fn newline(&mut self) {
        // Drop trailing whitespace so wrapped lines don't end in padding.
        while let Some(last) = self.current.last_mut() {
            let trimmed = last.content.trim_end().len();
            if trimmed == 0 && !last.content.is_empty() && last.content.trim().is_empty() {
                self.current.pop();
            } else {
                last.content.to_mut().truncate(trimmed);
                break;
            }
        }
        self.lines.push(Line::from(std::mem::take(&mut self.current)));
        self.used = 0;
        self.start_line();
    }

    fn start_line(&mut self) {
        if self.indent > 0 {
            self.current.push(Span::raw(" ".repeat(self.indent)));
            self.used = self.indent;
        }
    }

    fn push_word(&mut self, word: Word) {
        if word.width > self.available() && !self.at_line_start() {
            self.newline();
        }
        if word.width <= self.available() {
            for (style, s) in &word.pieces {
                self.push(*style, s, width(s));
            }
            return;
        }
        // Longer than a whole line: hard-break it by grapheme.
        for (style, s) in &word.pieces {
            for g in s.graphemes(true) {
                let gw = width(g);
                if gw > self.available() && !self.at_line_start() {
                    self.newline();
                }
                self.push(*style, g, gw);
            }
        }
    }
}

/// Word-wrap styled spans to `width` cells, indenting every line by `indent` cells.
/// Always returns at least one line.
pub fn wrap(spans: &[Span<'_>], width: usize, indent: usize) -> Vec<Line<'static>> {
    // Leave at least one usable column even in absurdly narrow areas.
    let indent = indent.min(width.saturating_sub(1));
    let mut b = Builder { lines: Vec::new(), current: Vec::new(), used: 0, width: width.max(1), indent };
    b.start_line();
    for token in tokenize(spans) {
        match token {
            Token::Newline => b.newline(),
            Token::Space(style, s) => {
                if b.at_line_start() {
                    continue;
                }
                let w = s.len().min(b.available());
                if w == 0 {
                    b.newline();
                } else {
                    b.push(style, &s[..w], w);
                }
            }
            Token::Word(word) => b.push_word(word),
        }
    }
    b.newline();
    b.lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Stylize};

    fn plain(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    fn wrap_str(s: &str, w: usize, indent: usize) -> Vec<String> {
        plain(&wrap(&[Span::raw(s)], w, indent))
    }

    #[test]
    fn sanitize_strips_escape_sequences_and_bidi() {
        assert_eq!(sanitize("hi\x1b]52;c;ZXZpbA==\x07there"), "hi]52;c;ZXZpbA==there");
        assert_eq!(sanitize("a\tb\r\nc\u{7f}\u{9b}d"), "a b\ncd");
        assert_eq!(sanitize("click \u{202E}gnp.exe"), "click gnp.exe");
        assert_eq!(sanitize("🦊 okay ünïcode"), "🦊 okay ünïcode");
    }

    #[test]
    fn wraps_on_word_boundaries() {
        assert_eq!(wrap_str("the quick brown fox", 10, 0), vec!["the quick", "brown fox"]);
        assert_eq!(wrap_str("the quick brown fox", 100, 0), vec!["the quick brown fox"]);
    }

    #[test]
    fn indents_every_line() {
        assert_eq!(wrap_str("aaa bbb ccc", 6, 2), vec!["  aaa", "  bbb", "  ccc"]);
    }

    #[test]
    fn breaks_words_longer_than_a_line() {
        assert_eq!(wrap_str("abcdefghij", 4, 0), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap_str("hi https://x.com/abcdefgh", 8, 0), vec!["hi", "https://", "x.com/ab", "cdefgh"]);
    }

    #[test]
    fn respects_wide_characters() {
        // Each CJK char and emoji is two cells wide.
        assert_eq!(wrap_str("日本語テキスト", 6, 0), vec!["日本語", "テキス", "ト"]);
        assert_eq!(wrap_str("🦊🦊🦊", 5, 0), vec!["🦊🦊", "🦊"]);
    }

    #[test]
    fn honours_hard_newlines_and_collapses_leading_space() {
        assert_eq!(wrap_str("one\ntwo", 20, 0), vec!["one", "two"]);
        assert_eq!(wrap_str("aaaa    bbbb", 4, 0), vec!["aaaa", "bbbb"]);
        assert_eq!(wrap_str("", 10, 0), vec![""]);
    }

    #[test]
    fn keeps_styles_and_glues_words_across_spans() {
        let spans = [Span::raw("see "), "https://a.b".fg(Color::Blue), Span::raw(", ok")];
        let lines = wrap(&spans, 14, 0);
        // "https://a.b," stays together even though the comma is a different span.
        assert_eq!(plain(&lines), vec!["see", "https://a.b,", "ok"]);
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Blue));
        assert_eq!(lines[1].spans[1].content, ",");
    }

    #[test]
    fn survives_zero_width() {
        assert_eq!(wrap_str("ab", 0, 3), vec!["a", "b"]);
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("hi", 5), "hi");
        assert_eq!(truncate("🦊🦊🦊", 4), "🦊…");
        assert_eq!(truncate("abc", 0), "");
    }
}
