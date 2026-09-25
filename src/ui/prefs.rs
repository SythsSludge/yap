//! The preferences tab: profiles | fields | options.

use super::{Rows, columns, render_list, section};
use crate::app::ListId;
use crate::app::{App, PrefsPane};
use crate::catalog::ANY;
use crate::keymap::Action;
use crate::prefs::Field;
use crate::text::truncate;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let cols = columns(frame, app, area, &[Constraint::Length(22), Constraint::Percentage(42), Constraint::Min(24)]);
    draw_profiles(frame, app, cols[0]);
    draw_fields(frame, app, cols[1]);
    draw_options(frame, app, cols[2]);
}

fn draw_profiles(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.prefs_ui.pane == PrefsPane::Profiles;
    let inner = section(frame, app, area, "profiles", focused);
    let width = inner.width.saturating_sub(5) as usize;
    let items: Vec<ListItem> = app
        .config
        .profiles
        .iter()
        .map(|p| {
            let active = p.name == app.config.active_profile;
            let ready = p.preferences.validate().is_ok();
            ListItem::new(Line::from(vec![
                Span::styled(truncate(&p.name, width), Style::new().fg(if active { t.fg } else { t.muted })),
                Span::styled(if active { " ●" } else { "" }, Style::new().fg(t.success)),
                Span::styled(if ready { "" } else { " !" }, Style::new().fg(t.warning)),
            ]))
        })
        .collect();
    let selected = app.prefs_ui.profile.min(app.config.profiles.len() - 1);
    render_list(frame, app, inner, items, Rows::new(ListId::PrefsProfiles, Some(selected), focused));
}

fn draw_fields(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.prefs_ui.pane == PrefsPane::Fields;
    let prefs = &app.config.active().preferences;
    let inner = section(frame, app, area, truncate(&app.config.active_profile, 40), focused);

    let [list_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(4)]).areas(inner);
    let label_w = 19;
    let value_w = list_area.width.saturating_sub(label_w + 3) as usize;
    let items: Vec<ListItem> = Field::ALL
        .iter()
        .map(|&f| {
            let missing = prefs.values(f).is_empty();
            let mut value = prefs.summary(f);
            if f == Field::Language && !app.config.settings.send_language {
                value.push_str(" (not sent)");
            }
            let value_style = if missing { Style::new().fg(t.warning) } else { Style::new().fg(t.fg) };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<w$}", f.label().to_lowercase(), w = label_w as usize), t.muted()),
                Span::styled(truncate(&value, value_w), value_style),
            ]))
        })
        .collect();
    render_list(frame, app, list_area, items, Rows::new(ListId::PrefsFields, Some(app.prefs_ui.field), focused));

    let status = match prefs.validate() {
        Ok(()) => vec![Line::from(vec![
            Span::styled("● ", Style::new().fg(t.success)),
            Span::styled("ready · ", t.muted()),
            Span::styled(app.keymap.label(Action::Find), Style::new().fg(t.fg)),
            Span::styled(" finds a partner", t.muted()),
        ])],
        Err(e) => vec![Line::from(vec![
            Span::styled("! ", Style::new().fg(t.warning)),
            Span::styled(e.to_string(), t.muted()),
        ])],
    };
    let mut status = status;
    status.push(Line::from(Span::styled("changes save automatically", t.muted().add_modifier(Modifier::DIM))));
    frame.render_widget(Paragraph::new(status).wrap(Wrap { trim: false }), status_area);
}

fn draw_options(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.prefs_ui.pane == PrefsPane::Options;
    let field = Field::ALL[app.prefs_ui.field.min(Field::ALL.len() - 1)];
    let prefs = &app.config.active().preferences;
    let options = crate::app::prefs_options(field, if focused { &app.prefs_ui.filter } else { "" });

    let mut title = vec![Span::raw(field.label().to_lowercase())];
    if field.is_multi() {
        let n = prefs.values(field).len();
        title.push(Span::styled(format!(" · {n} selected"), t.muted()));
    }
    let inner = section(frame, app, area, Line::from(title), focused);
    let [list_area, filter_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    if focused {
        let filter = if app.prefs_ui.filter.is_empty() {
            Span::styled("type to filter", t.muted().add_modifier(Modifier::DIM))
        } else {
            Span::styled(format!("{}▏", app.prefs_ui.filter), Style::new().fg(t.fg))
        };
        frame.render_widget(Paragraph::new(Line::from(vec![Span::styled("/ ", t.muted()), filter])), filter_area);
    }

    if options.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled("no matches", t.muted())), list_area);
        return;
    }
    let items: Vec<ListItem> = options
        .iter()
        .map(|&o| {
            let on = prefs.is_selected(field, o);
            let mark = match (field.is_multi(), on) {
                (true, true) => "■ ",
                (true, false) => "□ ",
                (false, true) => "● ",
                (false, false) => "○ ",
            };
            let label = if o == ANY { "Any / All" } else { o };
            let style = if on { Style::new().fg(t.accent) } else { Style::new().fg(t.fg) };
            ListItem::new(Line::from(vec![Span::styled(mark, style), Span::styled(label, style)]))
        })
        .collect();
    let selected = focused.then(|| app.prefs_ui.option.min(options.len() - 1));
    render_list(frame, app, list_area, items, Rows::new(ListId::PrefsOptions, selected, focused));
}
