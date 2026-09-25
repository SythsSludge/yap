//! Rendering. Everything here reads `App` state; the only writes are layout
//! measurements (chat scroll geometry) and cached image protocols.
//!
//! The look is deliberately quiet: no boxed panels, dim section titles, hairline
//! dividers between columns, and a single rounded input box.

mod chat;
mod drawer;
mod logs;
mod modal;
mod prefs;
mod settings;
mod traffic;

use crate::app::{App, ConnStatus, Hit, Level, ListId, PartnerState, Tab};
use crate::keymap::Action;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.clear_hits();
    let area = frame.area();
    frame.render_widget(Block::new().style(app.theme.base()), area);

    let [top, rule, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
            .areas(area);
    header(frame, app, top);
    hrule(frame, app, rule);
    let body = body.inner(ratatui::layout::Margin::new(1, 0));
    match app.tab {
        Tab::Chat => chat::draw(frame, app, body),
        Tab::Preferences => prefs::draw(frame, app, body),
        Tab::Drawer => drawer::draw(frame, app, body),
        Tab::Logs => logs::draw(frame, app, body),
        Tab::Traffic => traffic::draw(frame, app, body),
        Tab::Settings => settings::draw(frame, app, body),
    }
    hints(frame, app, footer);
    toasts(frame, app, body);
    modal::draw(frame, app, area);
    modal::viewer(frame, app, area);
}

fn header(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    // Messages waiting in chats you aren't looking at.
    let unread: usize = app.unseen + app.others.iter().map(|s| s.unseen).sum::<usize>();
    // Returns the line plus each tab's (offset, width) for click targets.
    let tabs = |short: bool| {
        let mut spans =
            vec![Span::styled(" yap", Style::new().fg(t.accent).add_modifier(Modifier::BOLD)), Span::raw("   ")];
        let mut places = Vec::new();
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let start: usize = spans.iter().map(Span::width).sum();
            let title = if short { tab.short_title() } else { tab.title() }.to_lowercase();
            if *tab == app.tab {
                spans.push(Span::styled(
                    title,
                    Style::new().fg(t.fg).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ));
            } else {
                spans.push(Span::styled(format!("{} ", i + 1), t.muted().add_modifier(Modifier::DIM)));
                spans.push(Span::styled(title, t.muted()));
            }
            if *tab == Tab::Chat && unread > 0 {
                spans.push(Span::styled(format!(" {unread}"), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)));
            }
            let end: usize = spans.iter().map(Span::width).sum();
            places.push((start as u16, (end - start) as u16, *tab));
            spans.push(Span::raw("  "));
        }
        (Line::from(spans), places)
    };

    let (dot, status) = match &app.status {
        ConnStatus::Online => (Span::styled("●", Style::new().fg(t.success)), "online".to_owned()),
        ConnStatus::Connecting => (Span::styled("●", Style::new().fg(t.warning)), "connecting".to_owned()),
        ConnStatus::Idle => (Span::styled("○", t.muted()), "offline".to_owned()),
        ConnStatus::Offline { retry_at: Some(at), .. } => (
            Span::styled("●", Style::new().fg(t.error)),
            format!("retry in {}s", at.saturating_duration_since(app.now).as_secs() + 1),
        ),
        ConnStatus::Offline { .. } => (
            Span::styled("●", Style::new().fg(t.error)),
            match app.keymap.hint(Action::Reconnect) {
                Some(k) => format!("offline · {k}"),
                None => "offline".to_owned(),
            },
        ),
    };
    let users = app.users_online.map(|n| Span::styled(format!(" · {} online", thousands(n)), t.muted()));
    let partner = match &app.partner {
        PartnerState::None => None,
        PartnerState::Searching => Some(Span::styled(" · searching…", Style::new().fg(t.warning))),
        PartnerState::Connected(info) => Some(Span::styled(
            format!(" · {} {}", info.gender, info.species).to_lowercase(),
            Style::new().fg(t.partner),
        )),
    };
    let profile = Span::styled(format!(" · {}", app.config.active_profile), t.muted());
    // Most to least detailed; the first that fits wins.
    let status_line = |detail: usize| {
        let mut spans = vec![Span::raw("  "), dot.clone(), Span::styled(format!(" {status}"), t.muted())];
        if detail >= 2 {
            spans.extend(partner.clone());
        }
        if detail >= 3 {
            spans.extend(users.clone());
        }
        if detail >= 1 {
            spans.push(profile.clone());
        }
        spans.push(Span::raw(" "));
        Line::from(spans)
    };

    let mut chosen = (tabs(true), status_line(0));
    'fit: for short in [false, true] {
        for detail in (0..=3).rev() {
            let (l, r) = (tabs(short), status_line(detail));
            if l.0.width() + r.width() <= area.width as usize {
                chosen = (l, r);
                break 'fit;
            }
        }
    }
    let ((left, places), right) = chosen;
    for (offset, width, tab) in places {
        if offset + width <= area.width {
            app.hit(Rect::new(area.x + offset, area.y, width, 1), Hit::Tab(tab));
        }
    }
    let right_w = (right.width() as u16).min(area.width);
    let [l, r] = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);
    frame.render_widget(Paragraph::new(left), l);
    frame.render_widget(Paragraph::new(right).right_aligned(), r);
}

/// Key hints for the footer: `(key, action)` pairs for the current context. Global
/// actions show whatever they're currently bound to, and are left out if unbound.
pub fn hint_pairs(app: &App) -> Vec<(String, &'static str)> {
    let bound = |actions: &[(Action, &'static str)]| -> Vec<(String, &'static str)> {
        actions.iter().filter_map(|&(a, what)| app.keymap.hint(a).map(|k| (k, what))).collect()
    };
    let fixed = |pairs: &[(&'static str, &'static str)]| -> Vec<(String, &'static str)> {
        pairs.iter().map(|&(k, what)| (k.to_owned(), what)).collect()
    };
    use crate::app::{ChatFocus, PrefsPane};
    if app.viewer.is_some() {
        return fixed(&[("o", "open"), ("y", "copy"), ("s", "save"), ("any key", "close")]);
    }
    if app.modal.is_some() {
        return fixed(&[("esc", "cancel")]);
    }
    match app.tab {
        Tab::Chat if app.chat_focus == ChatFocus::Drawer && app.drawer_panel => {
            fixed(&[("enter", "insert"), ("o", "open"), ("p", "preview"), ("y", "copy"), ("tab", "back")])
        }
        Tab::Chat => bound(&[
            (Action::Find, "find"),
            (Action::Next, "next"),
            (Action::Leave, "leave"),
            (Action::Block, "block"),
            (Action::Links, "links"),
            (Action::Drawer, "drawer"),
            (Action::Snippets, "snippets"),
            (Action::Editor, "editor"),
            (Action::Help, "help"),
        ]),
        Tab::Preferences => match app.prefs_ui.pane {
            PrefsPane::Profiles => fixed(&[
                ("enter", "use"),
                ("n", "new"),
                ("c", "copy"),
                ("r", "rename"),
                ("d", "delete"),
                ("e/E", "export"),
                ("i/I", "import"),
                ("tab", "pane"),
            ]),
            PrefsPane::Fields => {
                let mut hints = fixed(&[("enter", "edit"), ("x", "reset"), ("tab", "pane")]);
                hints.extend(bound(&[(Action::Find, "find partner")]));
                hints
            }
            PrefsPane::Options => fixed(&[("type", "filter"), ("enter/space", "toggle"), ("esc", "back")]),
        },
        Tab::Drawer if app.drawer_ui.shelf == crate::app::Shelf::Snippets => fixed(&[
            ("a", "add"),
            ("enter", "insert"),
            ("e", "edit"),
            ("r", "rename"),
            ("y", "copy"),
            ("d", "delete"),
            ("/", "search"),
            ("s", "links"),
            ("E/I", "export/import"),
        ]),
        Tab::Drawer => fixed(&[
            ("a", "add"),
            ("enter", "open"),
            ("i", "insert"),
            ("p", "preview"),
            ("e", "label"),
            ("t", "tags"),
            ("n", "note"),
            ("[ ]", "tag filter"),
            ("/", "search"),
            ("d", "delete"),
            ("s", "snippets"),
        ]),
        Tab::Logs if app.logs_ui.reading => {
            fixed(&[("↑↓ pgup pgdn", "scroll"), ("g/G", "top/bottom"), ("esc", "back")])
        }
        Tab::Logs => fixed(&[
            ("enter", "read"),
            ("p", "pin"),
            ("r", "rename"),
            ("e", "export"),
            ("d", "delete"),
            ("/", "search everything"),
        ]),
        Tab::Traffic => fixed(&[
            ("f", "follow"),
            ("h", "heartbeats"),
            ("p", "pretty"),
            ("/", "filter"),
            ("s", "send raw"),
            ("y", "copy"),
            ("e", "export"),
            ("c", "clear"),
        ]),
        Tab::Settings => match crate::app::settings::rows(&app.config.settings).get(app.settings_ui.selected) {
            Some(crate::app::settings::Row::Key(_)) => {
                fixed(&[("enter", "rebind"), ("backspace", "default"), ("x", "unbind")])
            }
            Some(crate::app::settings::Row::Domain(_)) => fixed(&[("d", "remove domain")]),
            _ => fixed(&[("enter", "toggle/edit"), ("←/→", "change")]),
        },
    }
}

fn hints(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let mut spans = vec![Span::raw("  ")];
    for (i, (key, action)) in hint_pairs(app).into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", t.muted().add_modifier(Modifier::DIM)));
        }
        spans.push(Span::styled(key, Style::new().fg(t.fg)));
        spans.push(Span::styled(format!(" {action}"), t.muted()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn toasts(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let width = area.width.saturating_sub(4).min(72);
    let mut y = area.bottom();
    for toast in app.toasts.iter().rev() {
        let color = match toast.level {
            Level::Info => t.accent,
            Level::Success => t.success,
            Level::Warning => t.warning,
            Level::Error => t.error,
        };
        let lines = crate::text::wrap(&[Span::raw(toast.text.clone())], width.saturating_sub(5) as usize, 0);
        let h = lines.len() as u16;
        if y < area.y + h + 4 {
            break;
        }
        y -= h;
        let rect = Rect::new(area.x + 1, y - 1, width, h);
        let body: Vec<Line> = lines
            .into_iter()
            .enumerate()
            .map(|(i, l)| {
                let lead = if i == 0 { " ● " } else { "   " };
                let mut spans = vec![Span::styled(lead, Style::new().fg(color))];
                spans.extend(l.spans);
                Line::from(spans)
            })
            .collect();
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new(body).style(Style::new().fg(t.fg).bg(t.surface)), rect);
        y -= 1;
    }
}

/// `1234567` → `1,234,567`, like the website's user counter.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A rectangle of at most `w`×`h` centred in `area`.
pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

/// The dim style for rules and dividers.
fn rule_style(app: &App) -> Style {
    app.theme.muted().add_modifier(Modifier::DIM)
}

fn hrule(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new("─".repeat(area.width as usize)).style(rule_style(app)), area);
}

/// Split `area` into columns with a hairline between each. Columns given
/// `Length(0)` are hidden entirely (no gap or divider) and come back as empty rects.
pub fn columns(frame: &mut Frame, app: &App, area: Rect, widths: &[Constraint]) -> Vec<Rect> {
    let shown: Vec<usize> = (0..widths.len()).filter(|&i| widths[i] != Constraint::Length(0)).collect();
    let laid = Layout::horizontal(shown.iter().map(|&i| widths[i])).spacing(3).split(area);
    let mut result = vec![Rect::new(area.x, area.y, 0, area.height); widths.len()];
    for (n, &i) in shown.iter().enumerate() {
        result[i] = laid[n];
    }
    let areas = laid;
    for pair in areas.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a.width == 0 || b.width == 0 {
            continue;
        }
        let x = a.right() + 1;
        if x < b.x {
            frame.render_widget(
                Block::new().borders(Borders::LEFT).border_style(rule_style(app)),
                Rect::new(x, area.y, 1, area.height),
            );
        }
    }
    result
}

/// A titled section: a small title line, then content. Returns the content area.
pub fn section<'a>(frame: &mut Frame, app: &App, area: Rect, title: impl Into<Line<'a>>, focused: bool) -> Rect {
    if area.height == 0 {
        return area;
    }
    let t = &app.theme;
    let style = if focused {
        Style::new().fg(t.accent).add_modifier(Modifier::BOLD)
    } else {
        t.muted().add_modifier(Modifier::BOLD)
    };
    let title: Line = title.into();
    frame.render_widget(Paragraph::new(title.style(style)), Rect::new(area.x, area.y, area.width, 1));
    Rect::new(area.x, area.y + 1, area.width, area.height - 1)
}

/// How a list is shown and what clicking its rows reports.
pub struct Rows<'t> {
    pub id: ListId,
    pub selected: Option<usize>,
    pub focused: bool,
    /// Maps item positions to the index reported on click (`None` for rows like
    /// section headers). Without it, item N reports N.
    pub targets: Option<&'t [Option<usize>]>,
}

impl<'t> Rows<'t> {
    pub fn new(id: ListId, selected: Option<usize>, focused: bool) -> Self {
        Rows { id, selected, focused, targets: None }
    }

    pub fn targets(mut self, targets: &'t [Option<usize>]) -> Self {
        self.targets = Some(targets);
        self
    }
}

/// Render a list and record a click target for each visible row.
pub fn render_list(frame: &mut Frame, app: &App, area: Rect, items: Vec<ListItem<'_>>, rows: Rows<'_>) {
    let heights: Vec<u16> = items.iter().map(|i| i.height() as u16).collect();
    let mut state = ListState::default().with_selected(rows.selected);
    frame.render_stateful_widget(list(app, items, rows.focused), area, &mut state);
    let mut y = area.y;
    for (i, &h) in heights.iter().enumerate().skip(state.offset()) {
        if y >= area.bottom() {
            break;
        }
        let h = h.min(area.bottom() - y);
        let target = match rows.targets {
            Some(t) => t.get(i).copied().flatten(),
            None => Some(i),
        };
        if let Some(index) = target {
            app.hit(Rect::new(area.x, y, area.width, h), Hit::Row { list: rows.id, index });
        }
        y += h;
    }
}

/// A list styled the quiet way: a `›` pointer and accent text for the selection
/// when focused, plain bold when not.
pub fn list<'a>(app: &App, items: Vec<ListItem<'a>>, focused: bool) -> List<'a> {
    let t = &app.theme;
    let (symbol, style) = if focused {
        ("› ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))
    } else {
        ("  ", Style::new().fg(t.fg).add_modifier(Modifier::BOLD))
    };
    List::new(items)
        .highlight_symbol(symbol)
        .highlight_style(style)
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
}

#[cfg(test)]
mod tests;
