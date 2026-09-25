//! The chat tab: sidebar, transcript with inline images, drawer panel, input.
//! Transcript layout is shared with the logs tab.

use super::{columns, list, section};
use crate::app::chat::{Entry, EntryKind};
use crate::app::{App, ChatFocus, PartnerState};
use crate::catalog::{ANY, MAX_MESSAGE_LEN};
use crate::config::ChatStyle;
use crate::images::{ImageState, Loaded};
use crate::links::find_links;
use crate::prefs::Field;
use crate::text::{truncate, width, wrap};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use std::sync::Arc;

const SIDEBAR_WIDTH: u16 = 28;
const DRAWER_WIDTH: u16 = 32;
const MAX_INPUT_ROWS: usize = 6;
/// Width of the name column in the compact layout.
const NAME_COL: usize = 9;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
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
    let (sidebar, middle, drawer) = (cols[0], cols[1], cols[2]);

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
    Image { loaded: Arc<Loaded>, x: u16 },
}

impl Chunk {
    fn height(&self) -> usize {
        match self {
            Chunk::Lines(l) => l.len(),
            Chunk::Image { loaded, .. } => loaded.rows() as usize,
        }
    }
}

pub(super) fn total_height(chunks: &[Chunk]) -> usize {
    chunks.iter().map(Chunk::height).sum()
}

fn message_spans(text: &str, base: Style, link: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut last = 0;
    for l in find_links(text) {
        if l.start > last {
            spans.push(Span::styled(text[last..l.start].to_owned(), base));
        }
        spans.push(Span::styled(l.url.clone(), link));
        last = l.end;
    }
    if last < text.len() {
        spans.push(Span::styled(text[last..].to_owned(), base));
    }
    spans
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
pub(super) fn layout_entries(app: &App, entries: &[Entry], width: usize, typing: bool) -> Vec<Chunk> {
    let t = &app.theme;
    let s = &app.config.settings;
    let style = s.chat_style;
    let mut out = Vec::new();
    let mut last_who: Option<Who> = None;

    for entry in entries {
        let time = s.timestamps.then(|| entry.at.format("%H:%M").to_string());
        let (who, text) = match &entry.kind {
            EntryKind::You(text) => (Who::You, text),
            EntryKind::Partner(text) => (Who::Partner, text),
            other => {
                // Status lines: a dim bullet (centred in the messages layout).
                let spans: Vec<Span> = match other {
                    EntryKind::System(text) => vec![Span::styled(text.clone(), t.muted())],
                    EntryKind::Warning(text) => vec![Span::styled(text.clone(), Style::new().fg(t.warning))],
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
                        last_who = None;
                        continue;
                    }
                    EntryKind::You(_) | EntryKind::Partner(_) => unreachable!(),
                };
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
                last_who = None;
                continue;
            }
        };

        let (name, name_color) = match who {
            Who::You => ("you", t.you),
            Who::Partner => ("partner", t.partner),
        };
        let name_style = Style::new().fg(name_color).add_modifier(Modifier::BOLD);
        let new_group = last_who != Some(who);
        last_who = Some(who);

        let image_x: u16 = match style {
            ChatStyle::Cozy => {
                if new_group {
                    out.push(Chunk::Lines(vec![Line::default()]));
                    let mut header = vec![Span::styled(name, name_style)];
                    if let Some(time) = &time {
                        header.push(Span::styled(format!("  {time}"), t.muted()));
                    }
                    out.push(Chunk::Lines(vec![Line::from(header)]));
                }
                let spans = message_spans(text, Style::new().fg(t.fg), t.link());
                out.push(Chunk::Lines(wrap(&spans, width, 2)));
                2
            }
            ChatStyle::Compact => {
                let mut prefix = Vec::new();
                let mut indent = NAME_COL;
                if let Some(time) = &time {
                    prefix.push(Span::styled(format!("{time} "), t.muted()));
                    indent += 6;
                }
                let shown = if new_group { name } else { "" };
                prefix.push(Span::styled(format!("{shown:<w$}", w = NAME_COL), name_style));
                let spans = message_spans(text, Style::new().fg(t.fg), t.link());
                out.push(Chunk::Lines(hanging(prefix, indent, &spans, width)));
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
                let wrapped = wrap(&message_spans(text, base, link), max_inner, 0);
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
                out.push(Chunk::Lines(lines));
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
                    out.push(Chunk::Image { loaded: loaded.clone(), x });
                }
                Some(ImageState::Loading) => out.push(note("loading preview…", t.muted())),
                Some(ImageState::Failed(e)) => {
                    out.push(note(&format!("preview unavailable: {}", truncate(e, 60)), t.muted()))
                }
                None => out.push(note("[image] ^O then p to preview", t.muted())),
            }
        }
    }

    if typing {
        out.push(Chunk::Lines(vec![Line::default()]));
        out.push(Chunk::Lines(vec![Line::from(Span::styled(
            "partner is typing…",
            t.muted().add_modifier(Modifier::ITALIC),
        ))]));
    }
    // Drop the leading spacer so the first message sits at the top.
    if let Some(Chunk::Lines(first)) = out.first()
        && first.len() == 1
        && first[0].width() == 0
    {
        out.remove(0);
    }
    out
}

/// Render chunks into `area`, starting from transcript line `top`.
pub(super) fn render_chunks(frame: &mut Frame, area: Rect, chunks: &[Chunk], top: usize) {
    let height = area.height as usize;
    let mut y = 0usize;
    for chunk in chunks {
        let h = chunk.height();
        if y + h > top && y < top + height {
            match chunk {
                Chunk::Lines(lines) => {
                    for (i, line) in lines.iter().enumerate() {
                        let row = y + i;
                        if row >= top && row < top + height {
                            let rect = Rect::new(area.x, area.y + (row - top) as u16, area.width, 1);
                            frame.render_widget(Paragraph::new(line.clone()), rect);
                        }
                    }
                }
                Chunk::Image { loaded, x } => {
                    let pos = SignedPosition { x: *x as i16, y: (y as i64 - top as i64) as i16 };
                    frame.render_widget(SlicedImage::new(&loaded.inline, pos), area);
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
            key("^F"),
            dim(" to find a partner"),
        ])),
        Err(e) => lines.push(Line::from(vec![
            pointer(),
            dim("set your preferences first ("),
            key("F3"),
            dim(format!("): {e}").as_str()),
        ])),
    }
    lines.push(Line::from(vec![pointer(), key("F1"), dim(" for keys, "), key("/help"), dim(" for commands")]));
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

    let chunks = layout_entries(app, &app.chat.entries, content.width as usize, app.chat.partner_typing);
    let total = total_height(&chunks);
    let height = content.height as usize;
    let top = app.chat.top_line(total, height);
    app.chat.last_total = total;
    app.chat.last_height = height;
    render_chunks(frame, content, &chunks, top);

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
            Line::from(Span::styled("or ^O then s on a chat link", t.muted())),
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
    let mut state = ListState::default().with_selected(focused.then_some(app.drawer_ui.list.selected));
    frame.render_stateful_widget(list(app, items, focused).style(Style::new().fg(t.fg)), inner, &mut state);
}

// ----- input --------------------------------------------------------------------

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
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
