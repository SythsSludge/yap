//! The settings tab.

use super::{Rows, render_list, section};
use crate::app::App;
use crate::app::ListId;
use crate::app::settings::{Row, rows};
use crate::text::{truncate, width};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let s = &app.config.settings;
    let rows = rows(s);
    let [list_area, help_area] = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);
    let inner = section(frame, app, list_area, "settings", true);
    let inner_w = inner.width.saturating_sub(2).min(96) as usize;

    let mut items = Vec::new();
    // Which settings row each list item is (headers and spacers aren't clickable).
    let mut targets: Vec<Option<usize>> = Vec::new();
    let mut selected_item = 0;
    let mut section_name = "";
    for (i, row) in rows.iter().enumerate() {
        if row.section() != section_name {
            section_name = row.section();
            items.push(ListItem::new(Line::default()));
            items.push(ListItem::new(Line::from(Span::styled(
                section_name.to_lowercase(),
                t.muted().add_modifier(Modifier::BOLD),
            ))));
            targets.extend([None, None]);
        }
        if i == app.settings_ui.selected {
            selected_item = items.len();
        }
        let label = format!("  {}", row.label(s));
        let value = row.key_value(&app.keymap).unwrap_or_else(|| row.value(s));
        let value_style = match value.as_str() {
            "on" => Style::new().fg(t.success),
            "off" | "unbound" => t.muted(),
            _ => Style::new().fg(t.fg),
        };
        let value = truncate(&value, inner_w.saturating_sub(width(&label) + 2));
        let pad = inner_w.saturating_sub(width(&label) + width(&value));
        items.push(ListItem::new(Line::from(vec![
            Span::styled(label, Style::new().fg(t.fg)),
            Span::raw(" ".repeat(pad)),
            Span::styled(value, value_style),
        ])));
        targets.push(Some(i));
    }
    render_list(frame, app, inner, items, Rows::new(ListId::Settings, Some(selected_item), true).targets(&targets));

    let help = rows.get(app.settings_ui.selected).map(|r: &Row| r.help()).unwrap_or_default();
    let config = format!("saved to {}", app.paths.config_file.display());
    let lines = vec![
        Line::from(Span::styled(help, Style::new().fg(t.fg))),
        Line::from(Span::styled(config, t.muted().add_modifier(Modifier::DIM))),
    ];
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        help_area.inner(ratatui::layout::Margin::new(2, 0)),
    );
}
