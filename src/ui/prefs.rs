//! The preferences tab: profiles | fields | options.

use super::pane;
use crate::app::{App, PrefsPane};
use crate::catalog::ANY;
use crate::prefs::Field;
use crate::text::truncate;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let [profiles, fields, options] =
        Layout::horizontal([Constraint::Length(24), Constraint::Percentage(42), Constraint::Min(24)]).areas(area);
    draw_profiles(frame, app, profiles);
    draw_fields(frame, app, fields);
    draw_options(frame, app, options);
}

fn draw_profiles(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.prefs_ui.pane == PrefsPane::Profiles;
    let width = area.width.saturating_sub(5) as usize;
    let items: Vec<ListItem> = app
        .config
        .profiles
        .iter()
        .map(|p| {
            let active = p.name == app.config.active_profile;
            let ready = p.preferences.validate().is_ok();
            ListItem::new(Line::from(vec![
                Span::styled(if active { "● " } else { "  " }, Style::new().fg(t.success)),
                Span::styled(truncate(&p.name, width.saturating_sub(2)), Style::new().fg(t.fg)),
                Span::styled(if ready { "" } else { " !" }, Style::new().fg(t.warning)),
            ]))
        })
        .collect();
    let selected = app.prefs_ui.profile.min(app.config.profiles.len() - 1);
    let mut state = ListState::default().with_selected(Some(selected));
    let list = List::new(items).block(pane(app, " Profiles ", focused)).highlight_style(if focused {
        t.selected()
    } else {
        Style::new().bold()
    });
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_fields(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.prefs_ui.pane == PrefsPane::Fields;
    let prefs = &app.config.active().preferences;
    let block = pane(app, format!(" {} ", truncate(&app.config.active_profile, 30)), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [list_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(4)]).areas(inner);
    let invalid = prefs.validate().err();
    let label_w = 18;
    let value_w = list_area.width.saturating_sub(label_w + 3) as usize;
    let items: Vec<ListItem> = Field::ALL
        .iter()
        .map(|&f| {
            let missing = prefs.values(f).is_empty();
            let mut value = prefs.summary(f);
            if f == Field::Language && !app.config.settings.send_language {
                value.push_str(" (not sent)");
            }
            let value_style = if missing { Style::new().fg(t.warning) } else { Style::new().fg(t.fg).bold() };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<w$}", f.label(), w = label_w as usize), t.muted()),
                Span::styled(truncate(&value, value_w), value_style),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.prefs_ui.field));
    let list = List::new(items)
        .highlight_style(if focused { t.selected() } else { Style::new().bold() })
        .highlight_symbol("▍");
    frame.render_stateful_widget(list, list_area, &mut state);

    let status = match invalid {
        None => vec![
            Line::from(Span::styled("✓ Ready to search", Style::new().fg(t.success).bold())),
            Line::from(vec![
                Span::styled("Ctrl-F", Style::new().fg(t.accent).bold()),
                Span::styled(" finds a partner with this profile.", t.muted()),
            ]),
        ],
        Some(e) => vec![Line::from(Span::styled(format!("✗ {e}"), Style::new().fg(t.warning)))],
    };
    let mut status = status;
    status.push(Line::from(Span::styled("Changes save automatically.", t.muted())));
    frame.render_widget(Paragraph::new(status).wrap(Wrap { trim: false }), status_area);
}

fn draw_options(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let focused = app.prefs_ui.pane == PrefsPane::Options;
    let field = Field::ALL[app.prefs_ui.field.min(Field::ALL.len() - 1)];
    let prefs = &app.config.active().preferences;
    let options = crate::app::prefs_options(field, if focused { &app.prefs_ui.filter } else { "" });

    let mut title = vec![Span::raw(format!(" {} ", field.label()))];
    if field.is_multi() {
        let n = prefs.values(field).iter().filter(|v| **v != ANY).count();
        if n > 0 {
            title.push(Span::raw(format!("· {n} selected ")));
        }
    }
    let block = pane(app, Line::from(title), focused);
    let block = if focused {
        let filter = if app.prefs_ui.filter.is_empty() {
            "type to filter".to_owned()
        } else {
            format!("{}▏", app.prefs_ui.filter)
        };
        block.title_bottom(Line::from(Span::styled(format!(" 🔍 {filter} "), Style::new().fg(t.accent))))
    } else {
        block
    };

    if options.is_empty() {
        let msg = Paragraph::new(Span::styled("No matches.", t.muted())).block(block);
        frame.render_widget(msg, area);
        return;
    }
    let items: Vec<ListItem> = options
        .iter()
        .map(|&o| {
            let on = prefs.is_selected(field, o);
            let mark = match (field.is_multi(), on) {
                (true, true) => "[x] ",
                (true, false) => "[ ] ",
                (false, true) => "(•) ",
                (false, false) => "( ) ",
            };
            let label = if o == ANY { "Any / All" } else { o };
            let style = if on { Style::new().fg(t.accent).bold() } else { Style::new().fg(t.fg) };
            ListItem::new(Line::from(vec![Span::styled(mark, style), Span::styled(label, style)]))
        })
        .collect();
    let selected = focused.then(|| app.prefs_ui.option.min(options.len() - 1));
    let mut state = ListState::default().with_selected(selected);
    let list = List::new(items).block(block).highlight_style(t.selected());
    frame.render_stateful_widget(list, area, &mut state);
}
