//! The settings tab.

use super::pane;
use crate::app::App;
use crate::app::settings::{Row, rows};
use crate::text::{truncate, width};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let s = &app.config.settings;
    let rows = rows(s);
    let [list_area, help_area] = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);
    let block = pane(app, " Settings ", true);
    let inner_w = block.inner(list_area).width as usize;

    let mut items = Vec::new();
    let mut selected_item = 0;
    let mut section = "";
    for (i, row) in rows.iter().enumerate() {
        if row.section() != section {
            section = row.section();
            if !items.is_empty() {
                items.push(ListItem::new(Line::default()));
            }
            items.push(ListItem::new(Line::from(Span::styled(section, Style::new().fg(t.accent).bold()))));
        }
        if i == app.settings_ui.selected {
            selected_item = items.len();
        }
        let label = format!("  {}", row.label(s));
        let value = row.value(s);
        let value_style = match value.as_str() {
            "on" => Style::new().fg(t.success),
            "off" => t.muted(),
            _ => Style::new().fg(t.fg),
        };
        let pad = inner_w.saturating_sub(width(&label) + width(&value) + 1);
        let value = truncate(&value, inner_w.saturating_sub(width(&label) + 2));
        items.push(ListItem::new(Line::from(vec![
            Span::styled(label, Style::new().fg(t.fg)),
            Span::raw(" ".repeat(pad)),
            Span::styled(value, value_style),
        ])));
    }
    let mut state = ListState::default().with_selected(Some(selected_item));
    frame.render_stateful_widget(List::new(items).block(block).highlight_style(t.selected()), list_area, &mut state);

    let help = rows.get(app.settings_ui.selected).map(|r: &Row| r.help()).unwrap_or_default();
    let config = format!("Saved to {}", app.paths.config_file.display());
    let lines =
        vec![Line::from(Span::styled(help, Style::new().fg(t.fg))), Line::from(Span::styled(config, t.muted()))];
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        help_area.inner(ratatui::layout::Margin::new(1, 0)),
    );
}
