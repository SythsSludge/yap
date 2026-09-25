//! Saving a chat as plain text, Markdown or a standalone HTML page. Markdown and HTML
//! keep the roleplay formatting (`*actions*`, `((asides))`), your names, and images
//! from trusted hosts.

use crate::app::chat::{Entry, EntryKind, partner_summary};
use crate::links::find_links;
use crate::text::{Markup, roleplay_markup};
use chrono::{DateTime, Local};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Markdown,
    Html,
}

impl Format {
    /// From the file extension; anything unrecognised is plain text.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("md" | "markdown") => Format::Markdown,
            Some("html" | "htm") => Format::Html,
            _ => Format::Text,
        }
    }
}

/// A chat ready to be written out.
pub struct Doc<'a> {
    /// "Chat with Dominant Female Fox".
    pub title: String,
    pub started: Option<DateTime<Local>>,
    pub you: String,
    pub partner: String,
    pub entries: &'a [Entry],
    /// The image links in a message that may be shown inline.
    pub images: &'a dyn Fn(&str) -> Vec<String>,
}

pub fn render(doc: &Doc, format: Format) -> String {
    match format {
        Format::Text => text(doc),
        Format::Markdown => markdown(doc),
        Format::Html => html(doc),
    }
}

fn started_line(doc: &Doc) -> Option<String> {
    doc.started.map(|s| format!("Started {}", s.format("%Y-%m-%d %H:%M")))
}

// ----- plain text -------------------------------------------------------------------

fn text(doc: &Doc) -> String {
    let mut out = doc.title.clone();
    if let Some(started) = started_line(doc) {
        out.push_str(&format!(" · {}", started.to_lowercase()));
    }
    out.push_str("\n\n");
    for e in doc.entries {
        out.push_str(&e.transcript_line(&doc.you, &doc.partner));
        out.push('\n');
    }
    out
}

// ----- shared -----------------------------------------------------------------------

/// A message split into plain text, links and roleplay markup.
enum Piece<'a> {
    Plain(&'a str),
    Link(&'a str),
    Styled(Markup, &'a str),
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    let links = find_links(text);
    let ranges: Vec<(usize, usize)> = links.iter().map(|l| (l.start, l.end)).collect();
    let mut marks: Vec<(usize, usize, Option<Markup>)> = links.iter().map(|l| (l.start, l.end, None)).collect();
    marks.extend(roleplay_markup(text, &ranges).into_iter().map(|(s, e, m)| (s, e, Some(m))));
    marks.sort_by_key(|m| m.0);
    let mut out = Vec::new();
    let mut at = 0;
    for (start, end, markup) in marks {
        if start > at {
            out.push(Piece::Plain(&text[at..start]));
        }
        out.push(match markup {
            None => Piece::Link(&text[start..end]),
            Some(m) => Piece::Styled(m, &text[start..end]),
        });
        at = end;
    }
    if at < text.len() {
        out.push(Piece::Plain(&text[at..]));
    }
    out
}

#[derive(PartialEq, Clone, Copy)]
enum Who {
    You,
    Partner,
}

// ----- Markdown ---------------------------------------------------------------------

fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '[' | ']' | '<' | '>' | '_' | '|') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn md_message(text: &str) -> String {
    let mut out = String::new();
    for piece in pieces(text) {
        match piece {
            Piece::Plain(s) => out.push_str(&md_escape(s)),
            Piece::Link(url) => out.push_str(&format!("<{url}>")),
            // `*waves*` is already Markdown italics; keep what's inside readable.
            Piece::Styled(Markup::Action, s) => out.push_str(&format!("*{}*", md_escape(&s[1..s.len() - 1]))),
            Piece::Styled(Markup::OutOfCharacter, s) => out.push_str(&md_escape(s)),
        }
    }
    // Stop a message that starts like a heading, quote or list from becoming one.
    if out.starts_with(['#', '-', '+']) || out.starts_with("* ") { format!("\\{out}") } else { out }
}

fn markdown(doc: &Doc) -> String {
    let mut out = format!("# {}\n\n", md_escape(&doc.title));
    if let Some(started) = started_line(doc) {
        out.push_str(&format!("{started}\n\n"));
    }
    out.push_str("---\n\n");
    let mut last: Option<Who> = None;
    for e in doc.entries {
        let time = e.at.format("%H:%M");
        let (who, text) = match &e.kind {
            EntryKind::You(t) => (Who::You, t),
            EntryKind::Partner(t) => (Who::Partner, t),
            EntryKind::System(t) => {
                out.push_str(&format!("_{time} · {}_\n\n", md_escape(t)));
                last = None;
                continue;
            }
            EntryKind::Warning(t) => {
                out.push_str(&format!("> **!** {}\n\n", md_escape(t)));
                last = None;
                continue;
            }
            EntryKind::PartnerInfo { info, common } => {
                let kinks: Vec<String> = info
                    .kink_list()
                    .iter()
                    .map(|k| if common.iter().any(|c| c == k) { format!("**{}**", md_escape(k)) } else { md_escape(k) })
                    .collect();
                out.push_str(&format!(
                    "> {} · {} · {}  \n> into {}\n\n",
                    md_escape(&info.role),
                    md_escape(&info.gender),
                    md_escape(&info.species),
                    kinks.join(", ")
                ));
                last = None;
                continue;
            }
        };
        if last != Some(who) {
            let name = if who == Who::You { &doc.you } else { &doc.partner };
            out.push_str(&format!("**{}** · {time}  \n", md_escape(name)));
        }
        last = Some(who);
        out.push_str(&md_message(text));
        out.push_str("\n\n");
        for url in (doc.images)(text) {
            out.push_str(&format!("![]({url})\n\n"));
        }
    }
    out
}

// ----- HTML -------------------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn html_message(text: &str) -> String {
    let mut out = String::new();
    for piece in pieces(text) {
        match piece {
            Piece::Plain(s) => out.push_str(&esc(s)),
            Piece::Link(url) => out.push_str(&format!(r#"<a href="{0}" rel="noreferrer noopener">{0}</a>"#, esc(url))),
            Piece::Styled(Markup::Action, s) => out.push_str(&format!("<em>{}</em>", esc(s))),
            Piece::Styled(Markup::OutOfCharacter, s) => {
                out.push_str(&format!(r#"<span class="ooc">{}</span>"#, esc(s)))
            }
        }
    }
    out
}

const STYLE: &str = "
:root { --bg: #f7f7f9; --fg: #1d1d22; --muted: #6b6b76; --you: #3b5bdb; --you-fg: #fff;
  --them: #e9e9ef; --warn: #b35c00; --mark: #7048e8; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #16161a; --fg: #e8e8ee; --muted: #8d8d99; --you: #4c6ef5; --you-fg: #fff;
    --them: #26262d; --warn: #ffa94d; --mark: #b197fc; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 16px/1.5 system-ui, -apple-system, 'Segoe UI', sans-serif; }
main { max-width: 46rem; margin: 0 auto; padding: 2rem 1rem 4rem; }
h1 { font-size: 1.4rem; margin: 0 0 .25rem; }
.meta, .sys, .who { color: var(--muted); }
.meta { margin: 0 0 2rem; }
.sys { text-align: center; font-size: .9rem; margin: 1rem 0; }
.warn { color: var(--warn); font-size: .9rem; margin: 1rem 0; }
.info { text-align: center; font-size: .9rem; margin: 1rem 0; color: var(--muted); }
.info b { color: var(--fg); }
.info mark { background: none; color: var(--mark); font-weight: 600; }
.group { display: flex; flex-direction: column; align-items: flex-start; margin: 1rem 0; }
.group.you { align-items: flex-end; }
.who { font-size: .8rem; margin: 0 .5rem .2rem; }
.msg { max-width: 80%; padding: .45rem .8rem; border-radius: 1rem; margin: .1rem 0;
  background: var(--them); overflow-wrap: anywhere; }
.you .msg { background: var(--you); color: var(--you-fg); }
.you .msg a { color: inherit; }
.ooc { opacity: .6; }
img { display: block; max-width: min(100%, 24rem); border-radius: .6rem; margin: .3rem 0; }
";

fn html(doc: &Doc) -> String {
    let mut body = format!("<h1>{}</h1>\n", esc(&doc.title));
    if let Some(started) = started_line(doc) {
        body.push_str(&format!("<p class=\"meta\">{}</p>\n", esc(&started)));
    }
    let mut open: Option<Who> = None;
    let close = |body: &mut String, open: &mut Option<Who>| {
        if open.take().is_some() {
            body.push_str("</div>\n");
        }
    };
    for e in doc.entries {
        let time = e.at.format("%H:%M").to_string();
        let (who, text) = match &e.kind {
            EntryKind::You(t) => (Who::You, t),
            EntryKind::Partner(t) => (Who::Partner, t),
            EntryKind::System(t) => {
                close(&mut body, &mut open);
                body.push_str(&format!("<p class=\"sys\">{time} · {}</p>\n", esc(t)));
                continue;
            }
            EntryKind::Warning(t) => {
                close(&mut body, &mut open);
                body.push_str(&format!("<p class=\"warn\">{}</p>\n", esc(t)));
                continue;
            }
            EntryKind::PartnerInfo { info, common } => {
                close(&mut body, &mut open);
                let kinks: Vec<String> = info
                    .kink_list()
                    .iter()
                    .map(|k| if common.iter().any(|c| c == k) { format!("<mark>{}</mark>", esc(k)) } else { esc(k) })
                    .collect();
                body.push_str(&format!(
                    "<p class=\"info\" title=\"{}\"><b>{} · {} · {}</b><br>into {}</p>\n",
                    esc(&partner_summary(info)),
                    esc(&info.role),
                    esc(&info.gender),
                    esc(&info.species),
                    kinks.join(", ")
                ));
                continue;
            }
        };
        if open != Some(who) {
            close(&mut body, &mut open);
            let (class, name) = if who == Who::You { ("you", &doc.you) } else { ("them", &doc.partner) };
            body.push_str(&format!("<div class=\"group {class}\">\n<div class=\"who\">{} · {time}</div>\n", esc(name)));
            open = Some(who);
        }
        body.push_str(&format!("<div class=\"msg\">{}</div>\n", html_message(text)));
        for url in (doc.images)(text) {
            body.push_str(&format!(r#"<img src="{}" alt="" loading="lazy" referrerpolicy="no-referrer">"#, esc(&url)));
            body.push('\n');
        }
    }
    close(&mut body, &mut open);
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>\n{body}</main>\n</body>\n</html>\n",
        esc(&doc.title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PartnerInfo;

    fn entries() -> Vec<Entry> {
        let at = DateTime::from_timestamp(1_700_000_000, 0).unwrap().with_timezone(&Local);
        let e = |kind| Entry { at, kind };
        vec![
            e(EntryKind::System("Connected.".into())),
            e(EntryKind::PartnerInfo {
                info: PartnerInfo {
                    gender: "Female".into(),
                    species: "Fox".into(),
                    kinks: "Musk, Biting".into(),
                    role: "Dominant".into(),
                    language: None,
                },
                common: vec!["Musk".into()],
            }),
            e(EntryKind::Partner("*waves* hi <b> ((brb)) https://i.imgur.com/a_b.png".into())),
            e(EntryKind::Partner("# not a heading".into())),
            e(EntryKind::You("hey".into())),
        ]
    }

    fn doc(entries: &[Entry]) -> Doc<'_> {
        Doc {
            title: "Chat with Dominant Female Fox".into(),
            started: None,
            you: "Rook".into(),
            partner: "Vix".into(),
            entries,
            images: &|text: &str| find_links(text).into_iter().map(|l| l.url).collect(),
        }
    }

    #[test]
    fn format_follows_the_extension() {
        assert_eq!(Format::from_path(Path::new("a.MD")), Format::Markdown);
        assert_eq!(Format::from_path(Path::new("a.html")), Format::Html);
        assert_eq!(Format::from_path(Path::new("a.txt")), Format::Text);
        assert_eq!(Format::from_path(Path::new("a")), Format::Text);
    }

    #[test]
    fn markdown_keeps_roleplay_and_names() {
        let entries = entries();
        let md = render(&doc(&entries), Format::Markdown);
        assert!(md.starts_with("# Chat with Dominant Female Fox\n"));
        assert!(md.contains("> Dominant · Female · Fox  \n> into **Musk**, Biting"));
        assert!(md.contains("**Vix** · "));
        assert!(md.contains("*waves* hi \\<b\\> ((brb)) <https://i.imgur.com/a_b.png>"), "{md}");
        assert!(md.contains("![](https://i.imgur.com/a_b.png)"));
        assert!(md.contains("\\# not a heading"));
        assert_eq!(md.matches("**Vix**").count(), 1, "consecutive messages share a header");
        assert!(md.contains("**Rook** · "));
    }

    #[test]
    fn html_escapes_and_styles() {
        let entries = entries();
        let html = render(&doc(&entries), Format::Html);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<em>*waves*</em> hi &lt;b&gt; <span class=\"ooc\">((brb))</span>"), "{html}");
        assert!(html.contains(r#"<a href="https://i.imgur.com/a_b.png""#));
        assert!(html.contains(r#"<img src="https://i.imgur.com/a_b.png""#));
        assert!(html.contains("<mark>Musk</mark>, Biting"));
        assert!(html.contains("<div class=\"group you\">"));
        assert!(!html.contains("<b> "), "partner text is escaped");
    }
}
