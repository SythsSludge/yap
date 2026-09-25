//! The chat tab: sidebar, transcript with inline images, drawer panel, input.
//! Transcript layout is shared with the logs tab.

use super::{Rows, columns, render_list, section};
use crate::app::chat::ChatMode;
use crate::app::chat::{Entry, EntryKind};
use crate::app::{App, ChatFocus, Hit, ListId, PartnerState};
use crate::catalog::{ANY, MAX_MESSAGE_LEN};
use crate::config::ChatStyle;
use crate::images::{ImageState, Loaded};
use crate::keymap::Action;
use crate::links::find_links;
use crate::prefs::Field;
use crate::text::{Markup, find_keywords, roleplay_markup, truncate, width, wrap};
use crate::theme::Theme;
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

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    app.mark_seen();
    let show_sidebar = app.config.settings.show_sidebar && area.width >= 90;
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

    let input_width = middle.width.saturating_sub(6).max(1) as usize;
    let (rows, _) = app.input.layout(input_width);
    let input_height = rows.len().clamp(1, MAX_INPUT_ROWS) as u16 + 2;
    let [transcript, input] = Layout::vertical([Constraint::Min(3), Constraint::Length(input_height)]).areas(middle);

    if show_sidebar {
        draw_sidebar(frame, app, sidebar);
    }
    draw_transcript(frame, app, transcript);
    if show_drawer {
        draw_drawer_panel(frame, app, drawer);
    }
    draw_input(frame, app, input);
    draw_command_hints(frame, app, transcript);
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
        marks.extend(find_keywords(text, &s.notify.keywords).into_iter().map(|(a, b)| (a, b, style)));
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

    for (index, entry) in entries.iter().enumerate() {
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
                let spans = message_spans(text, Style::new().fg(t.fg), t.link(), &marks);
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
                let spans = message_spans(text, Style::new().fg(t.fg), t.link(), &marks);
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
                let wrapped = wrap(&message_spans(text, base, link, &marks), max_inner, 0);
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
        out.push(Chunk::Lines(vec![Line::from(Span::styled(
            format!("{} is typing…", view.partner),
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
    render_chunks(frame, app, content, &laid.chunks, top, fill);

    if !app.chat.is_following() {
        let label = match app.chat.unread {
            0 => " ↓ esc to jump down ".to_owned(),
            n => format!(" ↓ {n} new message{} · esc ", if n == 1 { "" } else { "s" }),
        };
        let w = (width(&label) as u16).min(area.width);
        let rect = Rect::new(area.right() - w, area.bottom() - 1, w, 1);
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new(label).style(Style::new().fg(t.accent).bg(t.surface)), rect);
    }
}

// ----- sidebar ------------------------------------------------------------------

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let prefs = &app.config.active().preferences;
    let row = |label: &str, value: String| {
        Line::from(vec![Span::styled(format!("{label:<9}"), t.muted()), Span::styled(value, Style::new().fg(t.fg))])
    };
    let [you, partner] = Layout::vertical([Constraint::Length(12), Constraint::Min(0)]).areas(area);

    let title = Line::from(vec![
        Span::raw("you "),
        Span::styled(format!("· {}", truncate(&app.config.active_profile, 16)), t.muted()),
    ]);
    let inner = section(frame, app, you, title, false);
    let lines = vec![
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
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    let inner = section(frame, app, partner, "partner", false);
    let lines = match &app.partner {
        PartnerState::None => vec![Line::from(Span::styled("not connected", t.muted()))],
        PartnerState::Searching => vec![Line::from(Span::styled("looking for a match…", Style::new().fg(t.warning)))],
        PartnerState::Connected(info) => {
            let mut lines = vec![
                row("gender", info.gender.clone()),
                row("species", info.species.clone()),
                row("role", info.role.clone()),
            ];
            if let Some(lang) = &info.language {
                lines.push(row("language", lang.clone()));
            }
            lines.push(Line::from(Span::styled("into", t.muted())));
            let spans = kink_spans(&info.kink_list(), &prefs.kinks, t, Style::new().fg(t.fg));
            lines.extend(wrap(&spans, inner.width as usize, 0));
            lines
        }
    };
    frame.render_widget(Paragraph::new(lines), inner);
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
    for s in app.sessions() {
        let label = format!(" {} {} ", s.number, truncate(&s.label, 18));
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
        let lines: Vec<Line> = rows.iter().skip(first).take(visible).map(|r| Line::from(r.clone())).collect();
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

/// While typing `/co…`, list matching commands just above the input.
fn draw_command_hints(frame: &mut Frame, app: &App, transcript: Rect) {
    let text = app.input.text();
    if !text.starts_with('/') || text.starts_with("//") || text.contains(' ') || app.modal.is_some() {
        return;
    }
    let t = &app.theme;
    let matches: Vec<_> = crate::commands::HELP
        .iter()
        .filter(|(cmd, _)| cmd.starts_with(text) && cmd.starts_with('/') && !cmd.starts_with("//"))
        .take(8)
        .collect();
    if matches.is_empty() {
        return;
    }
    let width = transcript.width.saturating_sub(4).min(70);
    let h = matches.len() as u16;
    if transcript.height < h + 2 {
        return;
    }
    let rect = Rect::new(transcript.x + 2, transcript.bottom() - h, width, h);
    let lines: Vec<Line> = matches
        .iter()
        .map(|(cmd, what)| {
            Line::from(vec![Span::styled(format!(" {cmd:<28}"), Style::new().fg(t.fg)), Span::styled(*what, t.muted())])
        })
        .collect();
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).style(Style::new().bg(t.surface)), rect);
}
