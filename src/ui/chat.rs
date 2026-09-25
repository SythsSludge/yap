//! The chat tab: sidebar, transcript with inline images, drawer panel, input.

use super::{pane, thousands};
use crate::app::chat::{Entry, EntryKind, partner_summary};
use crate::app::{App, ChatFocus, PartnerState};
use crate::catalog::MAX_MESSAGE_LEN;
use crate::config::ChatStyle;
use crate::images::{ImageState, Loaded};
use crate::links::find_links;
use crate::prefs::Field;
use crate::text::{truncate, wrap};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use std::sync::Arc;

const SIDEBAR_WIDTH: u16 = 30;
const DRAWER_WIDTH: u16 = 34;
const MAX_INPUT_ROWS: usize = 6;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let show_sidebar = app.config.settings.show_sidebar && area.width >= 90;
    let show_drawer = app.drawer_panel && area.width >= 60;
    let [sidebar, middle, drawer] = Layout::horizontal([
        Constraint::Length(if show_sidebar { SIDEBAR_WIDTH } else { 0 }),
        Constraint::Min(20),
        Constraint::Length(if show_drawer { DRAWER_WIDTH } else { 0 }),
    ])
    .areas(area);

    let input_width = middle.width.saturating_sub(2).max(1) as usize;
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

// ----- transcript ---------------------------------------------------------------

/// A vertical slice of the transcript.
enum Chunk {
    Lines(Vec<Line<'static>>),
    Image { loaded: Arc<Loaded>, indent: u16 },
}

impl Chunk {
    fn height(&self) -> usize {
        match self {
            Chunk::Lines(l) => l.len(),
            Chunk::Image { loaded, .. } => loaded.rows() as usize,
        }
    }
}

fn message_spans(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut last = 0;
    for link in find_links(text) {
        if link.start > last {
            spans.push(Span::styled(text[last..link.start].to_owned(), Style::new().fg(theme.fg)));
        }
        spans.push(Span::styled(link.url.clone(), theme.link()));
        last = link.end;
    }
    if last < text.len() {
        spans.push(Span::styled(text[last..].to_owned(), Style::new().fg(theme.fg)));
    }
    spans
}

fn partner_info_spans(entry: &EntryKind, theme: &Theme) -> Vec<Span<'static>> {
    let EntryKind::PartnerInfo { info, common } = entry else { return Vec::new() };
    let sys = theme.system();
    let mut spans = vec![Span::styled(
        format!("Your partner is a {}, {}, {} interested in: ", info.role, info.gender, info.species),
        sys,
    )];
    let kinks = info.kink_list();
    for (i, kink) in kinks.iter().enumerate() {
        let shown = if *kink == crate::catalog::ANY { "Any / All" } else { kink };
        let style = if common.iter().any(|c| c == kink) {
            Style::new().fg(theme.highlight).add_modifier(Modifier::BOLD)
        } else {
            sys
        };
        spans.push(Span::styled(shown.to_owned(), style));
        spans.push(Span::styled(if i + 1 < kinks.len() { ", " } else { "." }, sys));
    }
    if kinks.is_empty() {
        // Shouldn't happen, but fall back to the plain sentence.
        spans = vec![Span::styled(partner_summary(info), sys)];
    }
    spans
}

fn entry_chunks(app: &App, entry: &Entry, width: usize, out: &mut Vec<Chunk>) {
    let t = &app.theme;
    let s = &app.config.settings;
    let time = s.timestamps.then(|| entry.at.format("%H:%M").to_string());
    let cozy = s.chat_style == ChatStyle::Cozy;

    let (who, who_style, text) = match &entry.kind {
        EntryKind::You(text) => ("You", Style::new().fg(t.you).bold(), text),
        EntryKind::Partner(text) => ("Partner", Style::new().fg(t.partner).bold(), text),
        other => {
            let mut spans = Vec::new();
            if let Some(time) = &time {
                spans.push(Span::styled(format!("{time} "), t.muted()));
            }
            match other {
                EntryKind::System(text) => spans.push(Span::styled(text.clone(), t.system())),
                EntryKind::Warning(text) => spans.push(Span::styled(format!("⚠ {text}"), Style::new().fg(t.warning))),
                info => spans.extend(partner_info_spans(info, t)),
            }
            out.push(Chunk::Lines(wrap(&spans, width, 0)));
            return;
        }
    };

    if cozy {
        let mut header = vec![Span::styled(who, who_style)];
        if let Some(time) = &time {
            header.push(Span::styled(format!("  {time}"), t.muted()));
        }
        let mut lines = vec![Line::from(header)];
        lines.extend(wrap(&message_spans(text, t), width, 2));
        out.push(Chunk::Lines(lines));
    } else {
        let mut spans = Vec::new();
        if let Some(time) = &time {
            spans.push(Span::styled(format!("{time} "), t.muted()));
        }
        spans.push(Span::styled(format!("{who}: "), who_style));
        spans.extend(message_spans(text, t));
        out.push(Chunk::Lines(wrap(&spans, width, 0)));
    }

    for url in app.preview_urls(text) {
        let note = |s: String, style: Style| Chunk::Lines(vec![Line::from(Span::styled(format!("  {s}"), style))]);
        match app.images.get(&url) {
            Some(ImageState::Ready(loaded)) => out.push(Chunk::Image { loaded: loaded.clone(), indent: 2 }),
            Some(ImageState::Loading) => out.push(note("⧗ loading preview…".into(), t.muted())),
            Some(ImageState::Failed(e)) => {
                out.push(note(format!("⚠ preview unavailable: {}", truncate(e, 60)), t.muted()))
            }
            None => out.push(note("🖼  Ctrl-O then p to preview".into(), t.muted())),
        }
    }
}

fn welcome(app: &App) -> Vec<Line<'static>> {
    let t = &app.theme;
    let key = |k: &str| Span::styled(k.to_owned(), Style::new().fg(t.accent).bold());
    let ready = app.config.active().preferences.validate();
    let mut lines = vec![
        Line::from(Span::styled("You must be 18 or older to use YiffSpot.", Style::new().fg(t.error).bold())),
        Line::default(),
        Line::from(Span::styled("yap — YiffSpot in your terminal", Style::new().fg(t.fg).bold())),
        Line::from(Span::styled(
            "Chat one-on-one with a random partner matched on the preferences you choose. \
             You stay anonymous unless you decide otherwise.",
            Style::new().fg(t.fg),
        )),
        Line::default(),
    ];
    match ready {
        Ok(()) => lines.push(Line::from(vec![
            Span::styled("Profile ", t.muted()),
            Span::styled(app.config.active_profile.clone(), Style::new().fg(t.fg).bold()),
            Span::styled(" is ready. Press ", t.muted()),
            key("Ctrl-F"),
            Span::styled(" to find a partner.", t.muted()),
        ])),
        Err(e) => lines.push(Line::from(vec![
            Span::styled("Set your preferences first (", t.muted()),
            key("F3"),
            Span::styled(format!("): {e}"), t.muted()),
        ])),
    }
    lines.push(Line::from(vec![
        Span::styled("Press ", t.muted()),
        key("F1"),
        Span::styled(" for keys, or type ", t.muted()),
        key("/help"),
        Span::styled(" for commands.", t.muted()),
    ]));
    lines
}

fn draw_transcript(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme.clone();
    let title = match &app.partner {
        PartnerState::Connected(_) => "Chat",
        PartnerState::Searching => "Chat · searching",
        PartnerState::None => "Chat",
    };
    let block = pane(app, format!(" {title} "), false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 4 || inner.height == 0 {
        return;
    }
    let content = Rect::new(inner.x + 1, inner.y, inner.width - 2, inner.height);

    if app.chat.entries.is_empty() {
        frame.render_widget(Paragraph::new(welcome(app)).wrap(Wrap { trim: false }), content);
        app.chat.last_total = 0;
        app.chat.last_height = content.height as usize;
        return;
    }

    let width = content.width as usize;
    let mut chunks = Vec::new();
    let cozy = app.config.settings.chat_style == ChatStyle::Cozy;
    for (i, entry) in app.chat.entries.iter().enumerate() {
        if cozy && i > 0 {
            chunks.push(Chunk::Lines(vec![Line::default()]));
        }
        entry_chunks(app, entry, width, &mut chunks);
    }
    if app.chat.partner_typing {
        if cozy {
            chunks.push(Chunk::Lines(vec![Line::default()]));
        }
        chunks.push(Chunk::Lines(vec![Line::from(Span::styled(
            "Your partner is typing…",
            t.system().add_modifier(Modifier::ITALIC),
        ))]));
    }

    let total: usize = chunks.iter().map(Chunk::height).sum();
    let height = content.height as usize;
    let top = app.chat.top_line(total, height);
    app.chat.last_total = total;
    app.chat.last_height = height;

    let mut y = 0usize;
    for chunk in &chunks {
        let h = chunk.height();
        if y + h > top && y < top + height {
            match chunk {
                Chunk::Lines(lines) => {
                    for (i, line) in lines.iter().enumerate() {
                        let row = y + i;
                        if row >= top && row < top + height {
                            let rect = Rect::new(content.x, content.y + (row - top) as u16, content.width, 1);
                            frame.render_widget(Paragraph::new(line.clone()), rect);
                        }
                    }
                }
                Chunk::Image { loaded, indent } => {
                    let pos = SignedPosition { x: *indent as i16, y: (y as i64 - top as i64) as i16 };
                    frame.render_widget(SlicedImage::new(&loaded.inline, pos), content);
                }
            }
        }
        y += h;
        if y >= top + height {
            break;
        }
    }

    if !app.chat.is_following() {
        let label = match app.chat.unread {
            0 => " ↓ scrolled up · Esc to jump down ".to_owned(),
            n => format!(" ↓ {n} new message{} · Esc ", if n == 1 { "" } else { "s" }),
        };
        let w = (label.chars().count() as u16).min(inner.width);
        let rect = Rect::new(inner.right() - w, inner.bottom() - 1, w, 1);
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new(label).style(Style::new().fg(t.bg).bg(t.accent)), rect);
    }
}

// ----- sidebar ------------------------------------------------------------------

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let [you, partner] = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let prefs = &app.config.active().preferences;
    let row = |label: &str, value: String| {
        Line::from(vec![Span::styled(format!("{label:<9}"), t.muted()), Span::styled(value, Style::new().fg(t.fg))])
    };
    let mut lines = vec![
        row("Gender", prefs.summary(Field::Gender)),
        row("Species", prefs.summary(Field::Species)),
        row("Role", prefs.summary(Field::Role)),
        row("Language", prefs.summary(Field::Language)),
        Line::from(Span::styled("Seeking", Style::new().fg(t.accent).bold())),
        row("Gender", prefs.summary(Field::PartnerGender)),
        row("Species", prefs.summary(Field::PartnerSpecies)),
        row("Role", prefs.summary(Field::PartnerRole)),
        row("Kinks", prefs.summary(Field::Kinks)),
    ];
    if let Some(n) = app.users_online {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(format!("{} users online", thousands(n)), t.muted())));
    }
    let title = format!(" You · {} ", truncate(&app.config.active_profile, 16));
    frame.render_widget(Paragraph::new(lines).block(pane(app, title, false)), you);

    let lines = match &app.partner {
        PartnerState::None => vec![Line::from(Span::styled("Not connected. Ctrl-F to search.", t.muted()))],
        PartnerState::Searching => vec![Line::from(Span::styled("Looking for a match…", Style::new().fg(t.warning)))],
        PartnerState::Connected(info) => {
            let mut lines = vec![
                row("Gender", info.gender.clone()),
                row("Species", info.species.clone()),
                row("Role", info.role.clone()),
            ];
            if let Some(lang) = &info.language {
                lines.push(row("Language", lang.clone()));
            }
            lines.push(Line::from(Span::styled("Kinks", Style::new().fg(t.accent).bold())));
            let mine = &prefs.kinks;
            let spans: Vec<Span> = info
                .kink_list()
                .into_iter()
                .enumerate()
                .flat_map(|(i, k)| {
                    let shown = if k == crate::catalog::ANY { "Any / All" } else { k };
                    let style = if mine.iter().any(|m| m == k) {
                        Style::new().fg(t.highlight).bold()
                    } else {
                        Style::new().fg(t.fg)
                    };
                    let sep = (i > 0).then(|| Span::styled(", ", t.muted()));
                    sep.into_iter().chain(std::iter::once(Span::styled(shown.to_owned(), style)))
                })
                .collect();
            lines.extend(wrap(&spans, area.width.saturating_sub(2) as usize, 0));
            lines
        }
    };
    frame.render_widget(Paragraph::new(lines).block(pane(app, " Partner ", false)).wrap(Wrap { trim: false }), partner);
}

// ----- drawer panel -------------------------------------------------------------

fn draw_drawer_panel(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.chat_focus == ChatFocus::Drawer;
    let block = pane(app, " Drawer ", focused);
    if app.drawer.items.is_empty() {
        let text = vec![
            Line::from(Span::styled("Nothing saved yet.", t.muted())),
            Line::default(),
            Line::from(Span::styled("/save <url> [label]", Style::new().fg(t.accent))),
            Line::from(Span::styled("or Ctrl-O → s on a chat link.", t.muted())),
        ];
        frame.render_widget(Paragraph::new(text).block(block).wrap(Wrap { trim: false }), area);
        return;
    }
    let width = area.width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = app
        .drawer
        .items
        .iter()
        .map(|i| {
            let host = url::Url::parse(&i.url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
            ListItem::new(vec![
                Line::from(Span::styled(truncate(&i.label, width), Style::new().fg(t.fg).bold())),
                Line::from(Span::styled(truncate(&host, width), t.muted())),
            ])
        })
        .collect();
    let mut state = ListState::default().with_selected(focused.then_some(app.drawer_ui.selected));
    let list = List::new(items).block(block).highlight_style(t.selected()).highlight_symbol("▍");
    frame.render_stateful_widget(list, area, &mut state);
}

// ----- input --------------------------------------------------------------------

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.chat_focus == ChatFocus::Input && app.modal.is_none() && app.viewer.is_none();
    let mut block = pane(app, " Message ", focused);
    let count = app.input.text().encode_utf16().count();
    if count > MAX_MESSAGE_LEN * 4 / 5 {
        let style = if count >= MAX_MESSAGE_LEN { Style::new().fg(t.error).bold() } else { Style::new().fg(t.warning) };
        block =
            block.title_top(Line::from(Span::styled(format!(" {count}/{MAX_MESSAGE_LEN} "), style)).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block.style(Style::new().bg(t.surface)), area);

    if app.input.is_empty() {
        let hint =
            if app.has_partner() { "Say hi… (/help for commands)" } else { "Type a message, or /help for commands" };
        frame.render_widget(Paragraph::new(Span::styled(hint, t.muted())), inner);
    }
    let (rows, (crow, ccol)) = app.input.layout(inner.width.max(1) as usize);
    let visible = inner.height as usize;
    let first = (crow + 1).saturating_sub(visible);
    if !app.input.is_empty() {
        let lines: Vec<Line> = rows.iter().skip(first).take(visible).map(|r| Line::from(r.clone())).collect();
        frame.render_widget(Paragraph::new(lines).style(Style::new().fg(t.fg)), inner);
    }
    if focused && visible > 0 && inner.width > 0 {
        let col = (ccol as u16).min(inner.width - 1);
        frame.set_cursor_position(Position::new(inner.x + col, inner.y + (crow - first) as u16));
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
    let width = transcript.width.saturating_sub(4).min(64);
    let h = matches.len() as u16;
    if transcript.height < h + 2 {
        return;
    }
    let rect = Rect::new(transcript.x + 2, transcript.bottom() - h - 1, width, h);
    let lines: Vec<Line> = matches
        .iter()
        .map(|(cmd, what)| {
            Line::from(vec![
                Span::styled(format!("{cmd:<24}"), Style::new().fg(t.accent).bold()),
                Span::styled(*what, t.muted()),
            ])
        })
        .collect();
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).style(Style::new().bg(t.surface)), rect);
}
