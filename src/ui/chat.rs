//! The chat tab: sidebar, transcript with inline images, drawer panel, input.
//! Transcript layout is shared with the logs tab.

use super::{Rows, columns, render_list, section};
use crate::app::chat::ChatMode;
use crate::app::chat::{Entry, EntryKind};
use crate::app::{App, ChatFocus, Hit, ListId, Pane, PartnerState, Split};
use crate::catalog::{ANY, MAX_MESSAGE_LEN};
use crate::config::ChatStyle;
use crate::images::{ImageState, Loaded};
use crate::keymap::Action;
use crate::links::find_links;
use crate::prefs::Field;
use crate::stats::human_duration;
use crate::text::{Markup, find_keywords, roleplay_markup, truncate, width, wrap};
use crate::theme::Theme;
use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, ListItem, Paragraph, Wrap};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use std::sync::Arc;

const SIDEBAR_WIDTH: u16 = 28;
const DRAWER_WIDTH: u16 = 32;
const MAX_INPUT_ROWS: usize = 6;
/// Narrower than this, a split shows only the chat you're typing in.
const SPLIT_MIN_WIDTH: u16 = 100;
/// A partner this quiet shows their last-heard time as a warning.
const QUIET: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    app.mark_seen();
    if let Some(split) = app.split
        && area.width >= SPLIT_MIN_WIDTH
    {
        return draw_split(frame, app, area, split);
    }
    app.narrow = area.width < 90;
    let show_sidebar = app.config.settings.show_sidebar && !app.narrow;
    let show_drawer = app.drawer_panel && area.width >= 60;
    let cols = columns(
        frame,
        app,
        area,
        &[
            Constraint::Length(if show_sidebar { SIDEBAR_WIDTH } else { 0 }),
            Constraint::Min(20),
            Constraint::Length(if show_drawer { DRAWER_WIDTH } else { 0 }),
        ],
    );
    let (sidebar, mut middle, drawer) = (cols[0], cols[1], cols[2]);
    if app.session_count() > 1 && middle.height > 6 {
        let [strip, rest] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(middle);
        draw_sessions(frame, app, Rect::new(strip.x, strip.y, strip.width, 1));
        middle = rest;
    }
    if show_sidebar {
        draw_sidebar(frame, app, sidebar);
    }
    if show_drawer {
        draw_drawer_panel(frame, app, drawer);
    }
    draw_live(frame, app, middle);
    // Too narrow to sit beside the chat: drawn over it instead, while toggled on.
    if app.narrow && app.sidebar_overlay {
        let w = (SIDEBAR_WIDTH + 2).min(area.width);
        let rect = Rect::new(area.x, area.y, w, area.height.saturating_sub(3));
        frame.render_widget(Clear, rect);
        frame.render_widget(Block::new().style(super::float_style(app)), rect);
        draw_sidebar(frame, app, rect.inner(ratatui::layout::Margin::new(1, 0)));
    }
}

/// The visible chat: transcript, message box and the completion popups.
fn draw_live(frame: &mut Frame, app: &mut App, middle: Rect) {
    // The buddy sits to the right of the message box, when there's room.
    let buddy_w = app.buddy().filter(|_| middle.width >= 60).map_or(0, |b| b.width() as u16 + 3);
    let input_width = middle.width.saturating_sub(6 + buddy_w).max(1) as usize;
    let (rows, _) = app.input.layout(input_width);
    let input_height = rows.len().clamp(1, MAX_INPUT_ROWS) as u16 + 2;
    let [transcript, input_row] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(input_height)]).areas(middle);
    let [input, buddy] = Layout::horizontal([Constraint::Min(10), Constraint::Length(buddy_w)]).areas(input_row);
    draw_transcript(frame, app, transcript);
    draw_input(frame, app, input);
    if buddy_w > 0 {
        draw_buddy(frame, app, buddy, transcript);
    }
    draw_command_hints(frame, app, transcript);
    draw_arg_hints(frame, app, transcript);
    draw_emoji_hints(frame, app, transcript);
    draw_snippet_hints(frame, app, transcript);
}

/// The buddy, and its speech bubble just above it (over the transcript's corner).
fn draw_buddy(frame: &mut Frame, app: &App, area: Rect, above: Rect) {
    let t = &app.theme;
    let color = app.buddy().and_then(|b| b.color).unwrap_or(t.accent);
    let lines: Vec<Line> =
        app.buddy_frame().into_iter().map(|l| Line::from(Span::styled(l, Style::new().fg(color)))).collect();
    let h = (lines.len() as u16).min(area.height);
    let art = Rect::new(area.x + 2, area.bottom() - h, area.width.saturating_sub(2), h);
    frame.render_widget(Paragraph::new(lines), art);
    app.hit(area, Hit::Buddy);

    let Some(text) = app.buddy_speech() else { return };
    let wrapped = wrap(&[Span::styled(text.to_owned(), Style::new().fg(t.fg))], 26, 0);
    let w = (wrapped.iter().map(Line::width).max().unwrap_or(0) as u16 + 4).min(above.width);
    let h = wrapped.len() as u16 + 2;
    if above.height < h + 2 || w < 6 {
        return;
    }
    // One row up, leaving the transcript's bottom row for the "new messages" badge.
    let right = area.right().min(above.right());
    let bubble = Rect::new(right.saturating_sub(w), above.bottom() - h - 1, w, h);
    let block =
        super::popup_block(app, t.surface, Style::new().fg(color)).padding(ratatui::widgets::Padding::horizontal(1));
    super::popup_fill(frame, app, bubble, t.surface);
    frame.render_widget(Paragraph::new(wrapped).block(block), bubble);
}

// ----- split view -----------------------------------------------------------------

/// Two panes: the chat you're typing in, and another chat or the traffic log.
fn draw_split(frame: &mut Frame, app: &mut App, mut area: Rect, split: Split) {
    if app.session_count() > 1 && area.height > 6 {
        let [strip, rest] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
        draw_sessions(frame, app, Rect::new(strip.x, strip.y, strip.width, 1));
        area = rest;
    }
    let cols = columns(frame, app, area, &[Constraint::Percentage(50), Constraint::Percentage(50)]);
    let left_active = split.left == app.session_id;
    let (active, passive) = if left_active { (cols[0], cols[1]) } else { (cols[1], cols[0]) };
    let other = if left_active { split.right } else { Pane::Session(split.left) };

    let title = pane_title(app, app.session_id, true);
    let body = section(frame, app, active, title, true);
    draw_live(frame, app, body);
    match other {
        Pane::Traffic => {
            let body = section(frame, app, passive, "traffic", false);
            super::traffic::draw(frame, app, body);
        }
        Pane::Session(id) => {
            let title = pane_title(app, id, false);
            let body = section(frame, app, passive, title, false);
            draw_parked(frame, app, body, id);
            // Clicks anywhere on it switch to it (rather than acting on its messages).
            app.hit(passive, Hit::Pane(id));
        }
    }
}

fn pane_title(app: &App, id: u64, active: bool) -> Line<'static> {
    let t = &app.theme;
    let summary = app.sessions().into_iter().find(|s| s.id == id);
    let (number, label) = summary.map_or((0, String::new()), |s| (s.number, s.label));
    let mut spans = vec![Span::raw(format!("chat {number} · {label}"))];
    if !active && let Some(key) = app.keymap.hint(Action::OtherPane) {
        spans.push(Span::styled(format!(" · {key} to type here"), t.muted().add_modifier(Modifier::DIM)));
    }
    Line::from(spans)
}

/// A chat that isn't the one you're typing in: its newest messages and its draft.
fn draw_parked(frame: &mut Frame, app: &mut App, area: Rect, id: u64) {
    let Some(pos) = app.others.iter().position(|s| s.id == id) else { return };
    app.others[pos].unseen = 0;
    let t = app.theme.clone();
    let [transcript, input] = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(area);
    let s = &app.others[pos];
    let character = app.config.profiles.iter().find(|p| p.name == s.profile).map(|p| p.character.trim());
    let view = View {
        you: character.filter(|c| !c.is_empty()).unwrap_or("you").to_owned(),
        partner: s.partner_nick.clone().unwrap_or_else(|| "partner".into()),
        typing: s.chat.partner_typing,
        focus: None,
        search: None,
        unsure: &s.chat.unsure,
        new_from: s.chat.new_from,
        labels: Vec::new(),
        dots: 3,
    };
    let content = Rect::new(transcript.x + 1, transcript.y, transcript.width.saturating_sub(2), transcript.height);
    if s.chat.entries.is_empty() {
        let note = Paragraph::new(Span::styled("nothing here yet", t.muted()));
        frame.render_widget(note, content);
    } else {
        let laid = layout_entries(app, &s.chat.entries, content.width as usize, &view);
        let top = laid.total().saturating_sub(content.height as usize);
        render_chunks(frame, app, content, &laid.chunks, top, None);
    }
    let draft = s.input.text().lines().next().unwrap_or_default();
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(t.muted().add_modifier(Modifier::DIM));
    let shown =
        if draft.is_empty() { Span::styled("…", t.muted()) } else { Span::styled(draft.to_owned(), t.muted()) };
    frame.render_widget(Paragraph::new(Line::from(vec![Span::styled("> ", t.muted()), shown])).block(block), input);
}

// ----- transcript layout ----------------------------------------------------------

/// A vertical slice of the transcript.
pub(super) enum Chunk {
    Lines(Vec<Line<'static>>),
    /// A message body. Clicking opens its links, or selects it if it has none.
    Message {
        lines: Vec<Line<'static>>,
        entry: usize,
        links: Vec<String>,
    },
    Image {
        loaded: Arc<Loaded>,
        x: u16,
        url: String,
    },
}

impl Chunk {
    fn height(&self) -> usize {
        match self {
            Chunk::Lines(l) | Chunk::Message { lines: l, .. } => l.len(),
            Chunk::Image { loaded, .. } => loaded.rows() as usize,
        }
    }
}

pub(super) fn total_height(chunks: &[Chunk]) -> usize {
    chunks.iter().map(Chunk::height).sum()
}

/// Pauses at least this long get a time line in the transcript.
const GAP: chrono::TimeDelta = chrono::TimeDelta::minutes(10);

fn width_of_label(label: &str) -> usize {
    width(label) + 2
}

/// What to mark between two entries: the day when it changes, the time after a
/// long pause, or nothing.
pub(super) fn gap_label(prev: DateTime<Local>, next: DateTime<Local>, today: chrono::NaiveDate) -> Option<String> {
    if next.date_naive() != prev.date_naive() {
        let day = next.date_naive();
        return Some(if day == today {
            "Today".into()
        } else if today.pred_opt() == Some(day) {
            "Yesterday".into()
        } else {
            next.format("%a %-d %b").to_string()
        });
    }
    (next - prev >= GAP).then(|| next.format("%H:%M").to_string())
}

/// Put link-hint letters in front of the links they label. `labels` are byte offsets
/// into the message, which always fall on span boundaries (links are cut out).
fn with_hint_labels(spans: Vec<Span<'static>>, labels: &[(usize, char)], style: Style) -> Vec<Span<'static>> {
    if labels.is_empty() {
        return spans;
    }
    let mut out = Vec::with_capacity(spans.len() + labels.len());
    let mut at = 0;
    for span in spans {
        if let Some(&(_, label)) = labels.iter().find(|(start, _)| *start == at) {
            out.push(Span::styled(format!(" {label} "), style));
        }
        at += span.content.len();
        out.push(span);
    }
    out
}

/// Split text into styled spans: links in `link`, and each mark's style patched over
/// whatever it covers (later marks win).
fn message_spans(text: &str, base: Style, link: Style, marks: &[(usize, usize, Style)]) -> Vec<Span<'static>> {
    let links: Vec<(usize, usize)> = find_links(text).iter().map(|l| (l.start, l.end)).collect();
    let mut cuts: Vec<usize> = vec![0, text.len()];
    cuts.extend(links.iter().flat_map(|&(s, e)| [s, e]));
    cuts.extend(marks.iter().flat_map(|&(s, e, _)| [s, e]));
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter(|w| w[0] < w[1] && text.is_char_boundary(w[0]) && text.is_char_boundary(w[1]))
        .map(|w| {
            let at = w[0];
            let mut style = if links.iter().any(|&(s, e)| s <= at && at < e) { link } else { base };
            for &(s, e, mark) in marks {
                if s <= at && at < e {
                    style = style.patch(mark);
                }
            }
            Span::styled(text[w[0]..w[1]].to_owned(), style)
        })
        .collect()
}

/// Wrap with a hanging indent: the first line starts with `prefix` (exactly `indent`
/// cells wide), continuation lines with spaces.
fn hanging(prefix: Vec<Span<'static>>, indent: usize, spans: &[Span<'_>], width: usize) -> Vec<Line<'static>> {
    let mut lines = wrap(spans, width, indent);
    if let Some(first) = lines.first_mut()
        && let Some(lead) = first.spans.first_mut()
        && lead.content.len() >= indent
        && lead.content[..indent].chars().all(|c| c == ' ')
    {
        let rest = lead.content[indent..].to_owned();
        if rest.is_empty() {
            first.spans.remove(0);
        } else {
            lead.content = rest.into();
        }
        let mut spans = prefix;
        spans.append(&mut first.spans);
        first.spans = spans;
    }
    lines
}

/// Centre lines within `width` (SMS-style status text).
fn centre(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|mut l| {
            let pad = width.saturating_sub(l.width()) / 2;
            l.spans.insert(0, Span::raw(" ".repeat(pad)));
            l
        })
        .collect()
}

fn kink_spans(kinks: &[&str], common: &[String], t: &Theme, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, kink) in kinks.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(", ", base));
        }
        let shown = if *kink == ANY { "Any / All" } else { kink };
        let style = if common.iter().any(|c| c == kink) {
            Style::new().fg(t.highlight).add_modifier(Modifier::BOLD)
        } else {
            base
        };
        spans.push(Span::styled(shown.to_owned(), style));
    }
    spans
}

/// Who sent a message, for grouping in the messages layout.
#[derive(PartialEq, Clone, Copy)]
enum Who {
    You,
    Partner,
}

/// Lay out a run of chat entries for a `width`-cell column.
/// What to layer over the plain transcript.
pub(super) struct View<'a> {
    pub you: String,
    pub partner: String,
    pub typing: bool,
    /// The entry to highlight: the selected message or the current search hit.
    pub focus: Option<usize>,
    /// Text to highlight wherever it appears.
    pub search: Option<&'a str>,
    /// Your messages that may not have arrived.
    pub unsure: &'a [usize],
    /// Draw a "new" divider above this entry.
    pub new_from: Option<usize>,
    /// Link hint letters: (entry, byte offset of the link, letter).
    pub labels: Vec<(usize, usize, char)>,
    /// How many dots of "is typing..." to show (1-3).
    pub dots: usize,
}

/// A laid-out transcript: the chunks, plus which chunks each entry produced.
pub(super) struct Laid {
    pub chunks: Vec<Chunk>,
    entry_chunks: Vec<(usize, usize, usize)>,
}

impl Laid {
    pub fn total(&self) -> usize {
        total_height(&self.chunks)
    }

    /// The transcript lines `[start, end)` an entry occupies.
    /// The first and last entries with any line inside `[top, top + height)`.
    pub fn visible_entries(&self, top: usize, height: usize) -> Option<(usize, usize)> {
        let on_screen: Vec<usize> = self
            .entry_chunks
            .iter()
            .map(|&(e, _, _)| e)
            .filter(|&e| self.lines_of(e).is_some_and(|(s, end)| s < top + height && end > top))
            .collect();
        Some((*on_screen.first()?, *on_screen.last()?))
    }

    pub fn lines_of(&self, entry: usize) -> Option<(usize, usize)> {
        let &(_, first, last) = self.entry_chunks.iter().find(|(e, _, _)| *e == entry)?;
        let start = total_height(&self.chunks[..first]);
        Some((start, start + total_height(&self.chunks[first..last])))
    }
}

/// Highlights for a piece of text: roleplay markup, keywords (partner messages only)
/// and search matches.
fn marks_for(app: &App, text: &str, partner: bool, view: &View, current: bool) -> Vec<(usize, usize, Style)> {
    let t = &app.theme;
    let s = &app.config.settings;
    let mut marks = Vec::new();
    if s.rp_formatting {
        let links: Vec<(usize, usize)> = find_links(text).iter().map(|l| (l.start, l.end)).collect();
        for (start, end, kind) in roleplay_markup(text, &links) {
            let style = match kind {
                Markup::Action => Style::new().add_modifier(Modifier::ITALIC),
                Markup::OutOfCharacter => Style::new().add_modifier(Modifier::DIM),
            };
            marks.push((start, end, style));
        }
    }
    if partner {
        let style = Style::new().fg(t.highlight).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        // Your keywords, and your character's name when the partner uses it.
        let mut words = s.notify.keywords.clone();
        if !view.you.eq_ignore_ascii_case("you") {
            words.push(view.you.clone());
        }
        marks.extend(find_keywords(text, &words).into_iter().map(|(a, b)| (a, b, style)));
    }
    if let Some(query) = view.search.map(str::to_lowercase).filter(|q| !q.is_empty()) {
        let lower = text.to_lowercase();
        // Byte offsets only line up when lowercasing kept the length.
        if lower.len() == text.len() {
            let style = if current {
                Style::new().fg(t.selection_fg).bg(t.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::new().add_modifier(Modifier::REVERSED)
            };
            let mut from = 0;
            while let Some(pos) = lower[from..].find(&query) {
                marks.push((from + pos, from + pos + query.len(), style));
                from += pos + query.len();
            }
        }
    }
    marks
}

/// Lay out a run of chat entries for a `width`-cell column.
pub(super) fn layout_entries(app: &App, entries: &[Entry], width: usize, view: &View) -> Laid {
    let t = &app.theme;
    let s = &app.config.settings;
    let style = s.chat_style;
    let mut out = Vec::new();
    let mut entry_chunks = Vec::new();
    let mut last_who: Option<Who> = None;
    let name_col = crate::text::width(&view.you).max(crate::text::width(&view.partner)).clamp(7, 16) + 2;

    let mut previous: Option<DateTime<Local>> = None;
    let today = Local::now().date_naive();
    for (index, entry) in entries.iter().enumerate() {
        // A quiet line where the day changes or after a long pause.
        if let Some(label) = previous.and_then(|prev| gap_label(prev, entry.at, today)) {
            let side = width.saturating_sub(width_of_label(&label)) / 2;
            let rule = t.muted().add_modifier(Modifier::DIM);
            out.push(Chunk::Lines(vec![Line::default()]));
            out.push(Chunk::Lines(vec![Line::from(vec![
                Span::styled("─".repeat(side.saturating_sub(1)), rule),
                Span::styled(format!(" {label} "), t.muted()),
                Span::styled("─".repeat(width.saturating_sub(side + width_of_label(&label) + 1)), rule),
            ])]));
            last_who = None;
        }
        previous = Some(entry.at);
        if view.new_from == Some(index) {
            let label = " new ";
            let side = width.saturating_sub(label.len()) / 2;
            let rule = Style::new().fg(t.accent);
            out.push(Chunk::Lines(vec![Line::default()]));
            out.push(Chunk::Lines(vec![Line::from(vec![
                Span::styled("─".repeat(side), rule.add_modifier(Modifier::DIM)),
                Span::styled(label, rule.add_modifier(Modifier::BOLD)),
                Span::styled("─".repeat(width.saturating_sub(side + label.len())), rule.add_modifier(Modifier::DIM)),
            ])]));
            // The next message starts its own group so its name shows under the line.
            last_who = None;
        }
        let first_chunk = out.len();
        let current = view.focus == Some(index);
        let time = s.timestamps.then(|| entry.at.format("%H:%M").to_string());
        let (who, text) = match &entry.kind {
            EntryKind::You(text) => (Who::You, text),
            EntryKind::Partner(text) => (Who::Partner, text),
            EntryKind::PartnerInfo { info, common } => {
                let head = format!("{} · {} · {}", info.role, info.gender, info.species);
                let base = t.muted();
                let mut lines = Vec::new();
                let into: Vec<Span> = std::iter::once(Span::styled("into ", base))
                    .chain(kink_spans(&info.kink_list(), common, t, base))
                    .collect();
                if style == ChatStyle::Sms {
                    lines.extend(centre(
                        wrap(&[Span::styled(head, Style::new().fg(t.partner))], width.saturating_sub(8), 0),
                        width,
                    ));
                    lines.extend(centre(wrap(&into, width.saturating_sub(8), 0), width));
                } else {
                    let bullet = vec![Span::styled("• ", Style::new().fg(t.partner))];
                    lines.extend(hanging(bullet, 2, &[Span::styled(head, Style::new().fg(t.partner))], width));
                    lines.extend(wrap(&into, width, 2));
                }
                if style != ChatStyle::Compact {
                    out.push(Chunk::Lines(vec![Line::default()]));
                }
                out.push(Chunk::Lines(lines));
                entry_chunks.push((index, first_chunk, out.len()));
                last_who = None;
                continue;
            }
            other => {
                // Status lines: a dim bullet (centred in the messages layout).
                let (text, base) = match other {
                    EntryKind::Warning(text) => (text, Style::new().fg(t.warning)),
                    EntryKind::System(text) => (text, t.muted()),
                    _ => unreachable!("messages and partner info are handled above"),
                };
                let marks = marks_for(app, text, false, view, current);
                let spans = message_spans(text, base, t.link(), &marks);
                let lines = if style == ChatStyle::Sms {
                    centre(wrap(&spans, width.saturating_sub(8), 0), width)
                } else {
                    let mark = if matches!(other, EntryKind::Warning(_)) {
                        Span::styled("! ", Style::new().fg(t.warning).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled("• ", t.muted())
                    };
                    hanging(vec![mark], 2, &spans, width)
                };
                if style != ChatStyle::Compact && last_who.is_some() {
                    out.push(Chunk::Lines(vec![Line::default()]));
                }
                out.push(Chunk::Lines(lines));
                entry_chunks.push((index, first_chunk, out.len()));
                last_who = None;
                continue;
            }
        };

        let (name, name_color) = match who {
            Who::You => (view.you.as_str(), t.you),
            Who::Partner => (view.partner.as_str(), t.partner),
        };
        let name_style = Style::new().fg(name_color).add_modifier(Modifier::BOLD);
        let new_group = last_who != Some(who);
        last_who = Some(who);
        let links: Vec<String> = find_links(text).into_iter().map(|l| l.url).collect();
        let marks = marks_for(app, text, who == Who::Partner, view, current);
        let message = |lines| Chunk::Message { lines, entry: index, links: links.clone() };
        let labels: Vec<(usize, char)> =
            view.labels.iter().filter(|(e, _, _)| *e == index).map(|&(_, at, c)| (at, c)).collect();
        let hint = Style::new().fg(t.selection_fg).bg(t.accent).add_modifier(Modifier::BOLD);

        let image_x: u16 = match style {
            ChatStyle::Cozy => {
                if new_group {
                    out.push(Chunk::Lines(vec![Line::default()]));
                    let mut header = vec![Span::styled(name.to_owned(), name_style)];
                    if let Some(time) = &time {
                        header.push(Span::styled(format!("  {time}"), t.muted()));
                    }
                    out.push(Chunk::Lines(vec![Line::from(header)]));
                }
                let spans =
                    with_hint_labels(message_spans(text, Style::new().fg(t.fg), t.link(), &marks), &labels, hint);
                out.push(message(wrap(&spans, width, 2)));
                2
            }
            ChatStyle::Compact => {
                let mut prefix = Vec::new();
                let mut indent = name_col;
                if let Some(time) = &time {
                    prefix.push(Span::styled(format!("{time} "), t.muted()));
                    indent += 6;
                }
                let shown = if new_group { truncate(name, name_col - 2) } else { String::new() };
                prefix.push(Span::styled(format!("{shown:<w$}", w = name_col), name_style));
                let spans =
                    with_hint_labels(message_spans(text, Style::new().fg(t.fg), t.link(), &marks), &labels, hint);
                out.push(message(hanging(prefix, indent, &spans, width)));
                indent as u16
            }
            ChatStyle::Sms => {
                let (bg, fg) = match who {
                    Who::You => (t.bubble_out, t.selection_fg),
                    Who::Partner => (t.bubble_in, t.fg),
                };
                if new_group {
                    out.push(Chunk::Lines(vec![Line::default()]));
                    let label = match &time {
                        Some(time) => format!("{name} · {time}"),
                        None => name.to_owned(),
                    };
                    let label = Span::styled(label, t.muted());
                    let pad = if who == Who::You { width.saturating_sub(label.width()) } else { 0 };
                    out.push(Chunk::Lines(vec![Line::from(vec![Span::raw(" ".repeat(pad)), label])]));
                }
                let max_inner = (width * 3 / 4).max(8).min(width.saturating_sub(2)).max(1);
                let base = Style::new().fg(fg).bg(bg);
                let link = base.add_modifier(Modifier::UNDERLINED);
                let wrapped =
                    wrap(&with_hint_labels(message_spans(text, base, link, &marks), &labels, hint), max_inner, 0);
                let inner = wrapped.iter().map(Line::width).max().unwrap_or(0);
                let bubble_w = inner + 2;
                let pad = if who == Who::You { width.saturating_sub(bubble_w) } else { 0 };
                let lines = wrapped
                    .into_iter()
                    .map(|l| {
                        let fill = inner.saturating_sub(l.width());
                        let mut spans = vec![Span::raw(" ".repeat(pad)), Span::styled(" ", base)];
                        spans.extend(
                            l.spans.into_iter().map(|s| if s.style.bg.is_none() { s.patch_style(base) } else { s }),
                        );
                        spans.push(Span::styled(" ".repeat(fill + 1), base));
                        Line::from(spans)
                    })
                    .collect();
                out.push(message(lines));
                pad as u16
            }
        };

        if who == Who::You && view.unsure.contains(&index) {
            let note = "may not have arrived";
            let pad = if style == ChatStyle::Sms { width.saturating_sub(note.len()) } else { image_x as usize };
            out.push(Chunk::Lines(vec![Line::from(Span::styled(
                format!("{}{note}", " ".repeat(pad)),
                Style::new().fg(t.warning).add_modifier(Modifier::ITALIC),
            ))]));
        }

        for url in app.preview_urls(text) {
            let note = |s: &str, st: Style| {
                Chunk::Lines(vec![Line::from(Span::styled(format!("{}{s}", " ".repeat(image_x as usize)), st))])
            };
            match app.images.get(&url) {
                Some(ImageState::Ready(loaded)) => {
                    let x = if style == ChatStyle::Sms && who == Who::You {
                        (width as u16).saturating_sub(loaded.inline.size().width)
                    } else {
                        image_x
                    };
                    out.push(Chunk::Image { loaded: loaded.clone(), x, url: url.clone() });
                }
                Some(ImageState::Loading) => out.push(note("loading preview…", t.muted())),
                Some(ImageState::Failed(e)) => {
                    out.push(note(&format!("preview unavailable: {}", truncate(e, 60)), t.muted()))
                }
                None => {
                    out.push(note(&format!("[image] {} then p to preview", app.keymap.label(Action::Links)), t.muted()))
                }
            }
        }
        entry_chunks.push((index, first_chunk, out.len()));
    }

    if view.typing {
        out.push(Chunk::Lines(vec![Line::default()]));
        // Dots that fill in and start over, so it reads as live.
        let dots = format!("{:<3}", ".".repeat(view.dots.clamp(1, 3)));
        out.push(Chunk::Lines(vec![Line::from(Span::styled(
            format!("{} is typing{dots}", view.partner),
            t.muted().add_modifier(Modifier::ITALIC),
        ))]));
    }
    // Drop the leading spacer so the first message sits at the top.
    if let Some(Chunk::Lines(first)) = out.first()
        && first.len() == 1
        && first[0].width() == 0
    {
        out.remove(0);
        for (_, first, last) in &mut entry_chunks {
            *first = first.saturating_sub(1);
            *last = last.saturating_sub(1);
        }
    }
    Laid { chunks: out, entry_chunks }
}

/// Render chunks into `area`, starting from transcript line `top`.
/// Render chunks into `area` from transcript line `top`, recording click targets for
/// messages with links and for images.
pub(super) fn render_chunks(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    chunks: &[Chunk],
    top: usize,
    fill: Option<(usize, usize)>,
) {
    let height = area.height as usize;
    // Shade the focused entry's rows before drawing text over them.
    if let Some((start, end)) = fill {
        for row in start.max(top)..end.min(top + height) {
            let rect = Rect::new(area.x.saturating_sub(1), area.y + (row - top) as u16, area.width + 1, 1);
            frame.render_widget(Block::new().style(Style::new().bg(app.theme.surface)), rect);
            frame.render_widget(
                Paragraph::new(Span::styled("▎", Style::new().fg(app.theme.accent))),
                Rect::new(rect.x, rect.y, 1, 1),
            );
        }
    }
    let mut y = 0usize;
    for chunk in chunks {
        let h = chunk.height();
        if y + h > top && y < top + height {
            match chunk {
                Chunk::Lines(lines) | Chunk::Message { lines, .. } => {
                    for (i, line) in lines.iter().enumerate() {
                        let row = y + i;
                        if row >= top && row < top + height {
                            let rect = Rect::new(area.x, area.y + (row - top) as u16, area.width, 1);
                            frame.render_widget(Paragraph::new(line.clone()), rect);
                            if let Chunk::Message { entry, links, .. } = chunk {
                                app.hit(rect, Hit::Message { entry: *entry, links: links.clone() });
                            }
                        }
                    }
                }
                Chunk::Image { loaded, x, url } => {
                    let pos = SignedPosition { x: *x as i16, y: (y as i64 - top as i64) as i16 };
                    frame.render_widget(SlicedImage::new(&loaded.inline, pos), area);
                    let first = y.max(top);
                    let last = (y + h).min(top + height);
                    let size = loaded.inline.size();
                    let x = area.x + (*x).min(area.width);
                    let rect = Rect::new(
                        x,
                        area.y + (first - top) as u16,
                        size.width.min(area.right() - x),
                        (last - first) as u16,
                    );
                    app.hit(rect, Hit::Image(url.clone()));
                }
            }
        }
        y += h;
        if y >= top + height {
            break;
        }
    }
}

fn welcome(app: &App) -> Vec<Line<'static>> {
    let t = &app.theme;
    let key = |k: &str| Span::styled(k.to_owned(), Style::new().fg(t.fg).add_modifier(Modifier::BOLD));
    let dim = |s: &str| Span::styled(s.to_owned(), t.muted());
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled("yap", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))),
        Line::from(dim("YiffSpot in your terminal")),
        Line::default(),
        Line::from(Span::styled("You must be 18 or older to use YiffSpot.", Style::new().fg(t.error))),
        Line::default(),
        Line::from(Span::styled(
            "Chat one-on-one with a random partner matched on the preferences you pick. \
             You stay anonymous unless you decide otherwise.",
            Style::new().fg(t.fg),
        )),
        Line::default(),
    ];
    let pointer = || Span::styled("› ", Style::new().fg(t.accent));
    match app.config.active().preferences.validate() {
        Ok(()) => lines.push(Line::from(vec![
            pointer(),
            dim("profile "),
            key(&app.config.active_profile),
            dim(" is ready. Press "),
            key(&app.keymap.label(Action::Find)),
            dim(" to find a partner"),
        ])),
        Err(e) => lines.push(Line::from(vec![
            pointer(),
            dim("set your preferences first ("),
            key(&app.keymap.label(Action::TabPreferences)),
            dim(format!("): {e}").as_str()),
        ])),
    }
    lines.push(Line::from(vec![
        pointer(),
        key(&app.keymap.label(Action::Help)),
        dim(" for keys, "),
        key("/help"),
        dim(" for commands"),
    ]));
    lines
}

fn draw_transcript(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 4 || area.height == 0 {
        return;
    }
    let content = Rect::new(area.x + 1, area.y, area.width - 2, area.height);
    let t = app.theme.clone();

    if app.chat.entries.is_empty() && !app.chat.partner_typing {
        frame.render_widget(Paragraph::new(welcome(app)).wrap(Wrap { trim: false }), content);
        app.chat.last_total = 0;
        app.chat.last_height = content.height as usize;
        return;
    }

    let search = match &app.chat.mode {
        ChatMode::Search(s) => Some(s.query.clone()),
        _ => None,
    };
    let view = View {
        you: app.my_label(),
        partner: app.partner_label(),
        typing: app.chat.partner_typing,
        focus: app.chat.focus_entry(),
        search: search.as_deref(),
        unsure: &app.chat.unsure,
        new_from: app.chat.new_from,
        labels: match &app.chat.mode {
            ChatMode::Hints(hints) => hints.iter().map(|h| (h.entry, h.start, h.label)).collect(),
            _ => Vec::new(),
        },
        dots: app
            .clock
            .typing_since
            .map_or(3, |since| (app.now.saturating_duration_since(since).as_millis() / 400 % 3) as usize + 1),
    };
    let laid = layout_entries(app, &app.chat.entries, content.width as usize, &view);
    let fill = view.focus.and_then(|e| laid.lines_of(e));
    let total = laid.total();
    // Keep a selected message or search hit on screen.
    if let Some((start, end)) = fill {
        let height = content.height as usize;
        let top = app.chat.top_line(total, height);
        if start < top {
            app.chat.pinned_top = Some(start);
        } else if end > top + height {
            let wanted = end.saturating_sub(height);
            app.chat.pinned_top = (wanted < total.saturating_sub(height)).then_some(wanted);
        }
    }
    let height = content.height as usize;
    let top = app.chat.top_line(total, height);
    app.chat.last_total = total;
    app.chat.last_height = height;
    app.chat.visible = laid.visible_entries(top, height);
    render_chunks(frame, app, content, &laid.chunks, top, fill);

    if !app.chat.is_following() {
        let label = match app.chat.unread {
            0 => " ↓ esc to jump down ".to_owned(),
            n => format!(" ↓ {n} new message{} · esc ", if n == 1 { "" } else { "s" }),
        };
        let w = (width(&label) as u16).min(area.width);
        let rect = Rect::new(area.right() - w, area.bottom() - 1, w, 1);
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new(label).style(super::float_style(app).fg(t.accent)), rect);
    }
}

// ----- sidebar ------------------------------------------------------------------

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let prefs = &app.config.active().preferences;
    let row = |label: &str, value: String| {
        Line::from(vec![Span::styled(format!("{label:<9}"), t.muted()), Span::styled(value, Style::new().fg(t.fg))])
    };
    // The partner matters most, so when space is short "you" shrinks to one line and
    // the timers share a line.
    let width = area.width as usize;
    let full = partner_lines(app, width, false);
    let (you_rows, (lines, kinks)) = if area.height as usize >= 12 + 1 + full.0.len() {
        (12, full)
    } else if area.height as usize >= 3 + 1 + full.0.len() {
        (3, full)
    } else {
        (3, partner_lines(app, width, true))
    };
    let [you, partner] = Layout::vertical([Constraint::Length(you_rows), Constraint::Min(0)]).areas(area);

    let title = Line::from(vec![
        Span::raw("you "),
        Span::styled(format!("· {}", truncate(&app.config.active_profile, 16)), t.muted()),
    ]);
    let inner = section(frame, app, you, title, false);
    let you_lines = if you_rows == 12 {
        vec![
            row("gender", prefs.summary(Field::Gender)),
            row("species", prefs.summary(Field::Species)),
            row("role", prefs.summary(Field::Role)),
            row("language", prefs.summary(Field::Language)),
            Line::default(),
            Line::from(Span::styled("seeking", t.muted().add_modifier(Modifier::BOLD))),
            row("gender", prefs.summary(Field::PartnerGender)),
            row("species", prefs.summary(Field::PartnerSpecies)),
            row("role", prefs.summary(Field::PartnerRole)),
            row("kinks", prefs.summary(Field::Kinks)),
        ]
    } else {
        let me = [Field::Gender, Field::Species, Field::Role].map(|f| prefs.summary(f)).join(" ");
        vec![Line::from(Span::styled(truncate(&me, width), Style::new().fg(t.fg)))]
    };
    frame.render_widget(Paragraph::new(you_lines), inner);

    let inner = section(frame, app, partner, "partner", false);
    if let Some((top, rows)) = kinks {
        let height = rows.min(inner.height.saturating_sub(top));
        app.hit(Rect::new(inner.x, inner.y + top, inner.width, height), Hit::Kinks);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The sidebar's partner section, and where the (clickable) kinks sit in it.
fn partner_lines(app: &App, width: usize, compact: bool) -> (Vec<Line<'static>>, Option<(u16, u16)>) {
    let t = &app.theme;
    let row = |label: &str, value: String| {
        Line::from(vec![Span::styled(format!("{label:<9}"), t.muted()), Span::styled(value, Style::new().fg(t.fg))])
    };
    match &app.partner {
        PartnerState::None => (vec![Line::from(Span::styled("not connected", t.muted()))], None),
        PartnerState::Searching => {
            let mut lines = vec![Line::from(Span::styled("looking for a match…", Style::new().fg(t.warning)))];
            if let Some(since) = app.clock.searching_since {
                lines.push(row("for", human_duration(app.now.saturating_duration_since(since).as_secs())));
            }
            (lines, None)
        }
        PartnerState::Connected(info) => {
            let mut lines = vec![
                row("gender", info.gender.clone()),
                row("species", info.species.clone()),
                row("role", info.role.clone()),
            ];
            if let Some(lang) = &info.language {
                lines.push(row("language", lang.clone()));
            }
            let groups = app.kink_groups().unwrap_or_default();
            let kinks_top = lines.len() as u16;
            lines.push(Line::from(vec![
                Span::styled("into", t.muted()),
                Span::styled(format!(" · {} shared", groups.shared.len()), t.muted().add_modifier(Modifier::DIM)),
            ]));
            // Shared kinks first, so the overlap is visible at a glance.
            let ordered: Vec<&str> = groups.shared.iter().chain(&groups.theirs).map(String::as_str).collect();
            let spans = kink_spans(&ordered, &groups.shared, t, Style::new().fg(t.fg));
            lines.extend(wrap(&spans, width, 0));
            if let Some(key) = app.keymap.hint(Action::Kinks) {
                lines.push(Line::from(Span::styled(format!("{key} explains"), t.muted().add_modifier(Modifier::DIM))));
            }
            let kinks = (kinks_top, lines.len() as u16 - kinks_top);
            lines.push(Line::default());
            if compact {
                lines.push(compact_clock(app));
            } else {
                lines.extend(clock_rows(app));
            }
            (lines, Some(kinks))
        }
    }
}

/// The timers on one line, for short terminals.
fn compact_clock(app: &App) -> Line<'static> {
    let t = &app.theme;
    let ago = |at: std::time::Instant| human_duration(app.now.saturating_duration_since(at).as_secs());
    let mut parts = Vec::new();
    if let Some(since) = app.clock.partner_since {
        parts.push(ago(since));
    }
    parts.push(match app.clock.last_heard {
        Some(at) => format!("heard {} ago", ago(at)),
        None => "not heard yet".into(),
    });
    Line::from(Span::styled(parts.join(" · "), t.muted()))
}

/// How long you've been together, when they last said something, and how long
/// they've been typing.
fn clock_rows(app: &App) -> Vec<Line<'static>> {
    let t = &app.theme;
    let ago = |at: std::time::Instant| app.now.saturating_duration_since(at);
    let row = |label: &str, value: String, style: Style| {
        Line::from(vec![Span::styled(format!("{label:<9}"), t.muted()), Span::styled(value, style)])
    };
    let plain = Style::new().fg(t.fg);
    let mut lines = Vec::new();
    if let Some(since) = app.clock.partner_since {
        lines.push(row("together", human_duration(ago(since).as_secs()), plain));
    }
    let (theirs, mine) = app.chat.average_words();
    if theirs.is_some() || mine.is_some() {
        let n = |w: Option<usize>| w.map_or("–".to_owned(), |w| w.to_string());
        lines.push(row("words", format!("them {} · you {}", n(theirs), n(mine)), plain));
    }
    let heard = match app.clock.last_heard {
        Some(at) => {
            let quiet = ago(at);
            let style = if quiet >= QUIET { Style::new().fg(t.warning) } else { plain };
            row("heard", format!("{} ago", human_duration(quiet.as_secs())), style)
        }
        None => row("heard", "nothing yet".into(), t.muted()),
    };
    lines.push(heard);
    if app.chat.partner_typing
        && let Some(since) = app.clock.typing_since
    {
        lines.push(row("typing", human_duration(ago(since).as_secs()), Style::new().fg(t.accent)));
    }
    lines
}

// ----- drawer panel -------------------------------------------------------------

fn draw_drawer_panel(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.chat_focus == ChatFocus::Drawer;
    let inner = section(frame, app, area, "drawer", focused);
    if app.drawer.items.is_empty() {
        let text = vec![
            Line::from(Span::styled("nothing saved yet", t.muted())),
            Line::default(),
            Line::from(Span::styled("/save <url> [label] [#tags]", Style::new().fg(t.fg))),
            Line::from(Span::styled(
                format!("or {} then s on a chat link", app.keymap.label(Action::Links)),
                t.muted(),
            )),
        ];
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
        return;
    }
    let width = inner.width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app
        .drawer
        .items
        .iter()
        .map(|i| {
            let tags: String = i.tags.iter().map(|t| format!("#{t} ")).collect();
            let host = url::Url::parse(&i.url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
            let second = if tags.is_empty() { host } else { tags };
            ListItem::new(vec![
                Line::from(Span::raw(truncate(&i.label, width))),
                Line::from(Span::styled(truncate(&second, width), t.muted())),
            ])
        })
        .collect();
    let selected = focused.then_some(app.drawer_ui.list.selected);
    render_list(frame, app, inner, items, Rows::new(ListId::ChatDrawer, selected, focused));
}

/// One tab per open chat, with unread counts; `+` opens another.
fn draw_sessions(frame: &mut Frame, app: &App, area: Rect) {
    /// Append `parts` to the strip, recording a click target for them if given.
    fn place(
        app: &App,
        area: Rect,
        x: &mut u16,
        spans: &mut Vec<Span<'static>>,
        parts: Vec<Span<'static>>,
        hit: Option<Hit>,
    ) {
        let w: u16 = parts.iter().map(|p| p.width() as u16).sum();
        if let Some(hit) = hit
            && *x + w <= area.right()
        {
            app.hit(Rect::new(*x, area.y, w, 1), hit);
        }
        *x += w;
        spans.extend(parts);
    }

    let t = &app.theme;
    let mut x = area.x;
    let mut spans = Vec::new();
    let mixed = app.mixed_profiles();
    for s in app.sessions() {
        let label = if mixed {
            format!(" {} {} · {} ", s.number, truncate(&s.label, 14), truncate(&s.profile, 10))
        } else {
            format!(" {} {} ", s.number, truncate(&s.label, 18))
        };
        let style = if s.active {
            Style::new().fg(t.fg).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else if s.online {
            Style::new().fg(t.muted)
        } else {
            t.muted().add_modifier(Modifier::DIM)
        };
        let mut parts = vec![Span::styled(label, style)];
        if s.unseen > 0 {
            parts.push(Span::styled(format!("{} ", s.unseen), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)));
        }
        place(app, area, &mut x, &mut spans, parts, Some(Hit::Session(s.id)));
        place(app, area, &mut x, &mut spans, vec![Span::styled("│", t.muted().add_modifier(Modifier::DIM))], None);
    }
    let new_label = match app.keymap.hint(Action::NewChat) {
        Some(k) => format!(" + new ({k}) "),
        None => " + new ".into(),
    };
    place(app, area, &mut x, &mut spans, vec![Span::styled(new_label, t.muted())], Some(Hit::NewSession));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ----- input --------------------------------------------------------------------

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    if let ChatMode::Search(search) = &app.chat.mode {
        return draw_search_bar(frame, app, area, search);
    }
    let t = &app.theme;
    let focused = app.chat_focus == ChatFocus::Input && app.modal.is_none() && app.viewer.is_none();
    let mut block = Block::bordered().border_type(BorderType::Rounded).border_style(if focused {
        Style::new().fg(t.muted)
    } else {
        t.muted().add_modifier(Modifier::DIM)
    });
    let count = app.input.text().encode_utf16().count();
    if count > MAX_MESSAGE_LEN * 4 / 5 {
        let style = if count >= MAX_MESSAGE_LEN { Style::new().fg(t.error) } else { Style::new().fg(t.warning) };
        block =
            block.title_top(Line::from(Span::styled(format!(" {count}/{MAX_MESSAGE_LEN} "), style)).right_aligned());
    }
    let text = app.input.text();
    let words = crate::app::chat::word_count(text);
    if words > 0 && !(text.starts_with('/') && !text.starts_with("//")) {
        let label = format!(" {words} word{} ", if words == 1 { "" } else { "s" });
        block = block.title_bottom(Line::from(Span::styled(label, t.muted())).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.hit(area, Hit::Input);
    if inner.width < 3 || inner.height == 0 {
        return;
    }
    let prompt = Rect::new(inner.x, inner.y, 2, 1);
    frame.render_widget(
        Paragraph::new(Span::styled("> ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))),
        prompt,
    );
    let text_area = Rect::new(inner.x + 2, inner.y, inner.width - 2, inner.height);

    if app.input.is_empty() {
        let hint = if app.has_partner() { "say hi…" } else { "type a message, or / for commands" };
        frame.render_widget(Paragraph::new(Span::styled(hint, t.muted())), text_area);
    }
    let (rows, (crow, ccol)) = app.input.layout(text_area.width.max(1) as usize);
    let visible = text_area.height as usize;
    let first = (crow + 1).saturating_sub(visible);
    if !app.input.is_empty() {
        let bad = app.misspelled();
        let text_all = app.input.text();
        let wrong = Style::new().fg(t.error).add_modifier(Modifier::UNDERLINED);
        // Rows are consecutive slices of the text, so their byte offsets add up.
        let mut offset = 0;
        let mut lines = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let start = offset;
            offset += row.len();
            // A paragraph break ends the row but isn't part of it.
            if text_all[offset..].starts_with('\n') {
                offset += 1;
            }
            if i < first || i >= first + visible {
                continue;
            }
            lines.push(Line::from(styled_ranges(row, start, &bad, wrong)));
        }
        frame.render_widget(Paragraph::new(lines).style(Style::new().fg(t.fg)), text_area);
    }
    if focused && visible > 0 && text_area.width > 0 {
        let col = (ccol as u16).min(text_area.width - 1);
        frame.set_cursor_position(Position::new(text_area.x + col, text_area.y + (crow - first) as u16));
    }
}

/// Replaces the message box while searching the chat.
fn draw_search_bar(frame: &mut Frame, app: &App, area: Rect, search: &crate::app::chat::Search) {
    let t = &app.theme;
    let count = match search.hits.len() {
        0 if search.query.is_empty() => String::new(),
        0 => " no matches ".into(),
        n => format!(" {} of {n} ", search.current + 1),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.accent))
        .title(Span::styled(" search ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD)))
        .title_top(Line::from(Span::styled(count, t.muted())).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 3 || inner.height == 0 {
        return;
    }
    let mut spans =
        vec![Span::styled("/ ", Style::new().fg(t.accent)), Span::styled(search.query.clone(), Style::new().fg(t.fg))];
    if !search.editing {
        spans.push(Span::styled("   n older · N newer · enter select · / edit · esc close", t.muted()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    if search.editing && app.modal.is_none() {
        let x = (width(&search.query) as u16 + 2).min(inner.width - 1);
        frame.set_cursor_position(Position::new(inner.x + x, inner.y));
    }
}

/// Split `row` (starting at byte `offset` of the whole text) into spans, giving the
/// parts inside `ranges` the `marked` style.
fn styled_ranges(row: &str, offset: usize, ranges: &[(usize, usize)], marked: Style) -> Vec<Span<'static>> {
    let end = offset + row.len();
    let mut spans = Vec::new();
    let mut at = offset;
    for &(s, e) in ranges.iter().filter(|(s, e)| *s < end && *e > offset) {
        let (s, e) = (s.max(offset), e.min(end));
        if s > at {
            spans.push(Span::raw(row[at - offset..s - offset].to_owned()));
        }
        spans.push(Span::styled(row[s - offset..e - offset].to_owned(), marked));
        at = e;
    }
    if at < end {
        spans.push(Span::raw(row[at - offset..].to_owned()));
    }
    spans
}

/// While typing `;intr…`, offer matching snippets just above the input.
fn draw_snippet_hints(frame: &mut Frame, app: &App, transcript: Rect) {
    let Some((_, found)) = app.snippet_suggestions() else { return };
    if app.modal.is_some() || transcript.height < found.len() as u16 + 2 {
        return;
    }
    let t = &app.theme;
    let box_w = transcript.width.saturating_sub(4).min(74);
    let h = found.len() as u16;
    let rect = Rect::new(transcript.x + 2, transcript.bottom() - h, box_w, h);
    let lines: Vec<Line> = found
        .iter()
        .enumerate()
        .map(|(i, &index)| {
            let snippet = &app.drawer.snippets[index];
            let (key, style) = if i == 0 {
                ("tab ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))
            } else {
                ("    ", Style::new().fg(t.fg))
            };
            let name = format!(";{:<15}", snippet.name);
            let preview = app.filled_snippet(index).unwrap_or_default();
            let room = (box_w as usize).saturating_sub(5 + width(&name));
            Line::from(vec![
                Span::styled(format!(" {key}"), style),
                Span::styled(name, style),
                Span::styled(truncate(&preview, room), t.muted()),
            ])
        })
        .collect();
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).style(super::float_style(app)), rect);
}

/// While typing `:smi…`, offer matching emoji just above the input.
fn draw_emoji_hints(frame: &mut Frame, app: &App, transcript: Rect) {
    let Some((_, found)) = app.emoji_suggestions() else { return };
    if app.modal.is_some() || transcript.height < 3 {
        return;
    }
    let t = &app.theme;
    let mut spans = vec![Span::styled(" tab ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))];
    let max = transcript.width.saturating_sub(4) as usize;
    for (i, (code, emoji)) in found.iter().enumerate() {
        let part = format!(" {emoji} :{code}: ");
        if spans.iter().map(Span::width).sum::<usize>() + width(&part) > max {
            break;
        }
        let style = if i == 0 { Style::new().fg(t.fg) } else { t.muted() };
        spans.push(Span::styled(part, style));
    }
    let w = spans.iter().map(Span::width).sum::<usize>() as u16;
    let rect = Rect::new(transcript.x + 2, transcript.bottom() - 1, w, 1);
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(Line::from(spans)).style(super::float_style(app)), rect);
}

/// While typing a command's argument (`/theme n…`), list what it could be.
fn draw_arg_hints(frame: &mut Frame, app: &App, transcript: Rect) {
    let Some((_, found)) = app.arg_suggestions() else { return };
    if app.modal.is_some() || transcript.height < found.len() as u16 + 2 {
        return;
    }
    let t = &app.theme;
    let w =
        (found.iter().map(|f| width(f)).max().unwrap_or(0) + 8).min(transcript.width.saturating_sub(4) as usize) as u16;
    let h = found.len() as u16;
    let rect = Rect::new(transcript.x + 2, transcript.bottom() - h, w, h);
    let lines: Vec<Line> = found
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let (key, style) = if i == 0 {
                ("tab ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))
            } else {
                ("    ", Style::new().fg(t.fg))
            };
            Line::from(vec![Span::styled(format!(" {key}"), style), Span::styled(truncate(f, w as usize - 6), style)])
        })
        .collect();
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).style(super::float_style(app)), rect);
}

/// While typing `/co…`, list matching commands just above the input.
fn draw_command_hints(frame: &mut Frame, app: &App, transcript: Rect) {
    if app.modal.is_some() {
        return;
    }
    let t = &app.theme;
    let matches: Vec<_> = crate::commands::completions(app.input.text()).into_iter().take(8).collect();
    if matches.is_empty() {
        return;
    }
    let width = transcript.width.saturating_sub(4).min(74);
    let h = matches.len() as u16;
    if transcript.height < h + 2 {
        return;
    }
    let rect = Rect::new(transcript.x + 2, transcript.bottom() - h, width, h);
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .map(|(i, (cmd, what))| {
            // Tab takes the first one.
            let (key, style) = if i == 0 {
                ("tab ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))
            } else {
                ("    ", Style::new().fg(t.fg))
            };
            Line::from(vec![
                Span::styled(format!(" {key}"), style),
                Span::styled(format!("{cmd:<28}"), style),
                Span::styled(*what, t.muted()),
            ])
        })
        .collect();
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).style(super::float_style(app)), rect);
}
