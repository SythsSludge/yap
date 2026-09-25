//! The raw websocket traffic viewer.

use super::{Rows, columns, render_list, section};
use crate::app::App;
use crate::app::ListId;
use crate::text::truncate;
use crate::traffic::{Direction, Entry, FrameKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};

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
        None => e.body.replace('\n', " ↵ "),
    }
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let (list_area, detail_area) = if area.width >= 110 {
        let cols = columns(frame, app, area, &[Constraint::Percentage(58), Constraint::Percentage(42)]);
        (cols[0], cols[1])
    } else {
        let [a, b] = Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).spacing(1).areas(area);
        (a, b)
    };

    let hide = app.config.settings.traffic.hide_heartbeat;
    let visible: Vec<&Entry> = app.traffic.visible(&app.traffic_ui.list.filter, hide).collect();
    let (bytes_in, bytes_out) = app.traffic.totals();
    let mut title = vec![
        Span::raw("traffic "),
        Span::styled(format!("· {} frames · ↓{} ↑{}", visible.len(), kib(bytes_in), kib(bytes_out)), t.muted()),
    ];
    if app.traffic_ui.follow {
        title.push(Span::styled(" · following", Style::new().fg(t.success)));
    }
    if hide {
        title.push(Span::styled(" · heartbeats hidden", t.muted()));
    }
    let inner = section(frame, app, list_area, Line::from(title), true);
    let mut list_rect = inner;
    if app.traffic_ui.list.filtering || !app.traffic_ui.list.filter.is_empty() {
        let cursor = if app.traffic_ui.list.filtering { "▏" } else { "" };
        let line = Line::from(vec![
            Span::styled("/ ", t.muted()),
            Span::styled(format!("{}{cursor}", app.traffic_ui.list.filter), Style::new().fg(t.fg)),
        ]);
        if inner.height > 1 {
            frame.render_widget(Paragraph::new(line), Rect::new(inner.x, inner.y, inner.width, 1));
            list_rect = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
        }
    }

    let width = list_rect.width.saturating_sub(36) as usize;
    let items: Vec<ListItem> = visible
        .iter()
        .map(|e| {
            let color = match e.dir {
                Direction::In => t.traffic_in,
                Direction::Out => t.traffic_out,
                Direction::Meta => t.muted,
            };
            let kind_style = match e.kind {
                FrameKind::Error => Style::new().fg(t.error),
                FrameKind::Close => Style::new().fg(t.warning),
                _ => Style::new().fg(color),
            };
            ListItem::new(Line::from(vec![
                Span::styled(e.at.format("%H:%M:%S%.3f ").to_string(), t.muted()),
                Span::styled(format!("{} ", e.dir.arrow()), Style::new().fg(color)),
                Span::styled(format!("{:<5} ", e.kind.label().to_lowercase()), kind_style),
                Span::styled(format!("{:>6} ", e.size), t.muted().add_modifier(Modifier::DIM)),
                Span::styled(truncate(&summary(e), width), Style::new().fg(t.fg)),
            ]))
        })
        .collect();
    let selected = app.traffic_selected();
    render_list(frame, app, list_rect, items, Rows::new(ListId::Traffic, selected, true));

    let Some(entry) = selected.and_then(|i| visible.get(i)) else {
        let inner = section(frame, app, detail_area, "frame", false);
        let hint = if app.traffic.is_empty() {
            "no traffic yet. frames appear here as they're sent and received."
        } else {
            "no frames match"
        };
        frame.render_widget(Paragraph::new(Span::styled(hint, t.muted())).wrap(Wrap { trim: false }), inner);
        return;
    };
    let title = Line::from(vec![
        Span::raw(format!("frame #{} ", entry.seq)),
        Span::styled(
            format!(
                "· {} {} · {} bytes · {}",
                entry.dir.arrow(),
                entry.kind.label().to_lowercase(),
                entry.size,
                entry.at.format("%H:%M:%S%.3f")
            ),
            t.muted(),
        ),
    ]);
    let inner = section(frame, app, detail_area, title, false);
    let body = if app.traffic_ui.pretty { entry.pretty_body() } else { entry.body.clone() };
    let lines: Vec<Line> =
        body.lines().map(|l| Line::from(Span::styled(l.to_owned(), Style::new().fg(t.fg)))).collect();
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inner.inner(ratatui::layout::Margin::new(0, 1)),
    );
}
