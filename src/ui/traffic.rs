//! The raw websocket traffic viewer.

use super::pane;
use crate::app::App;
use crate::text::truncate;
use crate::traffic::{Direction, Entry, FrameKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

fn kib(bytes: u64) -> String {
    if bytes < 1024 { format!("{bytes} B") } else { format!("{:.1} KiB", bytes as f64 / 1024.0) }
}

/// One-line summary: the message type for JSON frames, else the body.
fn summary(e: &Entry) -> String {
    match e.message_type() {
        Some(kind) => {
            let data = serde_json::from_str::<serde_json::Value>(&e.body)
                .ok()
                .and_then(|v| v.get("data").cloned())
                .map(|d| match d {
                    serde_json::Value::Bool(true) => String::new(),
                    serde_json::Value::String(s) => format!(" {s:?}"),
                    other => format!(" {other}"),
                })
                .unwrap_or_default();
            format!("{kind}{data}")
        }
        None => e.body.replace('\n', " ⏎ "),
    }
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let wide = area.width >= 110;
    let [list_area, detail_area] = if wide {
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(area)
    } else {
        Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area)
    };

    let hide = app.config.settings.traffic.hide_heartbeat;
    let visible: Vec<&Entry> = app.traffic.visible(&app.traffic_ui.list.filter, hide).collect();
    let (bytes_in, bytes_out) = app.traffic.totals();
    let mut title = vec![
        Span::raw(format!(" Traffic · {} ", visible.len())),
        Span::styled(format!("↓{} ↑{} ", kib(bytes_in), kib(bytes_out)), t.muted()),
    ];
    if app.traffic_ui.follow {
        title.push(Span::styled("● follow ", Style::new().fg(t.success)));
    }
    if hide {
        title.push(Span::styled("heartbeats hidden ", t.muted()));
    }
    let mut block = pane(app, Line::from(title), true);
    if app.traffic_ui.list.filtering || !app.traffic_ui.list.filter.is_empty() {
        let cursor = if app.traffic_ui.list.filtering { "▏" } else { "" };
        block = block.title_bottom(Line::from(Span::styled(
            format!(" / {}{cursor} ", app.traffic_ui.list.filter),
            Style::new().fg(t.accent),
        )));
    }

    let width = list_area.width.saturating_sub(34) as usize;
    let items: Vec<ListItem> = visible
        .iter()
        .map(|e| {
            let color = match e.dir {
                Direction::In => t.traffic_in,
                Direction::Out => t.traffic_out,
                Direction::Meta => t.muted,
            };
            let kind_style = match e.kind {
                FrameKind::Error => Style::new().fg(t.error).bold(),
                FrameKind::Close => Style::new().fg(t.warning).bold(),
                _ => Style::new().fg(color),
            };
            ListItem::new(Line::from(vec![
                Span::styled(e.at.format("%H:%M:%S%.3f ").to_string(), t.muted()),
                Span::styled(format!("{} ", e.dir.arrow()), Style::new().fg(color).bold()),
                Span::styled(format!("{:<5} ", e.kind.label()), kind_style),
                Span::styled(format!("{:>6} ", e.size), t.muted()),
                Span::styled(truncate(&summary(e), width), Style::new().fg(t.fg)),
            ]))
        })
        .collect();

    let selected = app.traffic_selected();
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(List::new(items).block(block).highlight_style(t.selected()), list_area, &mut state);

    let detail = pane(app, " Frame ", false);
    let Some(entry) = selected.and_then(|i| visible.get(i)) else {
        let hint = if app.traffic.is_empty() {
            "No traffic yet. Frames appear here as they're sent and received."
        } else {
            "No frames match."
        };
        frame.render_widget(
            Paragraph::new(Span::styled(hint, t.muted())).block(detail).wrap(Wrap { trim: false }),
            detail_area,
        );
        return;
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("#{} ", entry.seq), t.muted()),
        Span::styled(format!("{} {} ", entry.dir.arrow(), entry.kind.label()), Style::new().fg(t.accent).bold()),
        Span::styled(format!("{} bytes · {}", entry.size, entry.at.format("%Y-%m-%d %H:%M:%S%.3f")), t.muted()),
    ])];
    lines.push(Line::default());
    let body = if app.traffic_ui.pretty { entry.pretty_body() } else { entry.body.clone() };
    lines.extend(body.lines().map(|l| Line::from(Span::styled(l.to_owned(), Style::new().fg(t.fg)))));
    frame.render_widget(Paragraph::new(lines).block(detail).wrap(Wrap { trim: false }), detail_area);
}
