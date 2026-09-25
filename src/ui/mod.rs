//! Rendering. Everything here reads `App` state; the only writes are layout
//! measurements the chat view needs for scrolling.

mod chat;
mod drawer;
mod modal;
mod prefs;
mod settings;
mod traffic;

use crate::app::{App, ConnStatus, Level, PartnerState, Tab};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(app.theme.base()), area);

    let [top, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(area);
    header(frame, app, top);
    match app.tab {
        Tab::Chat => chat::draw(frame, app, body),
        Tab::Preferences => prefs::draw(frame, app, body),
        Tab::Drawer => drawer::draw(frame, app, body),
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
    let tabs = |short: bool| {
        let mut spans = vec![
            Span::styled(" yap ", Style::new().fg(t.bg).bg(t.accent).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ];
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let title = if short { tab.short_title() } else { tab.title() };
            let label = format!(" {} {} ", i + 1, title);
            spans.push(if *tab == app.tab {
                Span::styled(label, t.selected())
            } else {
                Span::styled(label, t.muted())
            });
        }
        Line::from(spans)
    };

    let (dot, status) = match &app.status {
        ConnStatus::Online => (Span::styled("●", Style::new().fg(t.success)), "online".to_owned()),
        ConnStatus::Connecting => (Span::styled("●", Style::new().fg(t.warning)), "connecting".to_owned()),
        ConnStatus::Idle => (Span::styled("○", t.muted()), "offline".to_owned()),
        ConnStatus::Offline { retry_at: Some(at), .. } => (
            Span::styled("●", Style::new().fg(t.error)),
            format!("retry in {}s", at.saturating_duration_since(app.now).as_secs() + 1),
        ),
        ConnStatus::Offline { .. } => (Span::styled("●", Style::new().fg(t.error)), "offline · Ctrl-R".to_owned()),
    };
    let users = app.users_online.map(|n| Span::styled(format!(" · {} online", thousands(n)), t.muted()));
    let partner = match &app.partner {
        PartnerState::None => None,
        PartnerState::Searching => Some(Span::styled(" · searching…", Style::new().fg(t.warning))),
        PartnerState::Connected(info) => {
            Some(Span::styled(format!(" · with {} {}", info.gender, info.species), Style::new().fg(t.partner)))
        }
    };
    let profile = Span::styled(format!(" · {}", app.config.active_profile), t.muted());
    // Most to least detailed; the first that fits wins.
    let status_line = |detail: usize| {
        let mut spans = vec![Span::raw("  "), dot.clone(), Span::raw(format!(" {status}"))];
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
            if l.width() + r.width() <= area.width as usize {
                chosen = (l, r);
                break 'fit;
            }
        }
    }
    let (left, right) = chosen;
    let right_w = (right.width() as u16).min(area.width);
    let [l, r] = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);
    frame.render_widget(Paragraph::new(left), l);
    frame.render_widget(Paragraph::new(right).right_aligned(), r);
}

/// Key hints for the footer: `(key, action)` pairs for the current context.
pub fn hint_pairs(app: &App) -> Vec<(&'static str, &'static str)> {
    use crate::app::{ChatFocus, PrefsPane};
    if app.viewer.is_some() {
        return vec![("o", "open"), ("y", "copy"), ("s", "save"), ("any", "close")];
    }
    if app.modal.is_some() {
        return vec![("Esc", "cancel")];
    }
    match app.tab {
        Tab::Chat if app.chat_focus == ChatFocus::Drawer && app.drawer_panel => {
            vec![("Enter", "insert"), ("o", "open"), ("p", "preview"), ("y", "copy"), ("Tab", "back")]
        }
        Tab::Chat => vec![
            ("^F", "find"),
            ("^D", "leave"),
            ("^B", "block"),
            ("^O", "links"),
            ("^E", "drawer"),
            ("^P", "profile"),
            ("PgUp", "scroll"),
            ("F1", "help"),
        ],
        Tab::Preferences => match app.prefs_ui.pane {
            PrefsPane::Profiles => vec![
                ("Enter", "use"),
                ("n", "new"),
                ("c", "copy"),
                ("r", "rename"),
                ("d", "delete"),
                ("e/E", "export"),
                ("i/I", "import"),
                ("Tab", "pane"),
            ],
            PrefsPane::Fields => vec![("Enter", "edit"), ("x", "reset"), ("Tab", "pane"), ("^F", "find partner")],
            PrefsPane::Options => vec![("type", "filter"), ("Enter/Space", "toggle"), ("Esc", "back")],
        },
        Tab::Drawer => vec![
            ("a", "add"),
            ("Enter", "open"),
            ("i", "insert"),
            ("p", "preview"),
            ("e", "label"),
            ("n", "note"),
            ("y", "copy"),
            ("d", "delete"),
            ("J/K", "move"),
            ("/", "filter"),
        ],
        Tab::Traffic => vec![
            ("f", "follow"),
            ("h", "heartbeats"),
            ("p", "pretty"),
            ("/", "filter"),
            ("s", "send raw"),
            ("y", "copy"),
            ("e", "export"),
            ("c", "clear"),
        ],
        Tab::Settings => vec![("Enter", "toggle/edit"), ("←/→", "change"), ("d", "remove domain")],
    }
}

fn hints(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let mut spans = vec![Span::raw(" ")];
    for (key, action) in hint_pairs(app) {
        spans.push(Span::styled(key, Style::new().fg(t.accent).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {action}  "), t.muted()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn toasts(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let width = area.width.saturating_sub(4).min(70);
    let mut y = area.bottom();
    for toast in app.toasts.iter().rev() {
        let color = match toast.level {
            Level::Info => t.accent,
            Level::Success => t.success,
            Level::Warning => t.warning,
            Level::Error => t.error,
        };
        let lines = crate::text::wrap(&[Span::raw(toast.text.clone())], width.saturating_sub(3) as usize, 0);
        let h = lines.len() as u16;
        if y < area.y + h + 4 {
            break;
        }
        y -= h;
        let rect = Rect::new(area.x + 1, y - 1, width, h);
        let body: Vec<Line> = lines
            .into_iter()
            .map(|l| {
                let mut spans = vec![Span::styled("▌ ", Style::new().fg(color))];
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

/// A bordered pane whose border lights up when focused.
pub fn pane<'a>(app: &App, title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    let t = &app.theme;
    let title: Line = title.into();
    let title = if focused { title.style(Style::new().fg(t.accent).bold()) } else { title.style(t.muted()) };
    Block::bordered()
        .border_type(if focused { ratatui::widgets::BorderType::Thick } else { ratatui::widgets::BorderType::Rounded })
        .border_style(t.border(focused))
        .title(title)
}

#[cfg(test)]
mod tests;
