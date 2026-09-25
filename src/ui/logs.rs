//! The logs tab: earlier chats, one per partner.

use super::chat::{layout_entries, render_chunks, total_height};
use super::{columns, list, section};
use crate::app::App;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, ListState, Paragraph, Wrap};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let cols = columns(frame, app, area, &[Constraint::Length(40), Constraint::Min(20)]);
    draw_list(frame, app, cols[0]);
    draw_transcript(frame, app, cols[1]);
}

fn draw_list(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let visible = app.visible_logs();
    let title = Line::from(vec![Span::raw("chats "), Span::styled(format!("· {}", app.logs.items.len()), t.muted())]);
    let inner = section(frame, app, area, title, !app.logs_ui.reading);
    if inner.height == 0 {
        return;
    }
    let saving = if app.config.settings.save_logs { "saving to disk" } else { "session only · saving off" };
    let filter_h = u16::from(app.logs_ui.list.filtering || !app.logs_ui.list.filter.is_empty());
    frame.render_widget(
        Paragraph::new(Span::styled(saving, t.muted().add_modifier(Modifier::DIM))),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if filter_h == 1 && inner.height > 1 {
        let cursor = if app.logs_ui.list.filtering { "▏" } else { "" };
        let line = Line::from(vec![
            Span::styled("/ ", t.muted()),
            Span::styled(format!("{}{cursor}", app.logs_ui.list.filter), Style::new().fg(t.fg)),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, inner.y + 1, inner.width, 1));
    }
    let top = inner.y + 2 + filter_h;
    let list_area = Rect::new(inner.x, top, inner.width, inner.bottom().saturating_sub(top));

    if visible.is_empty() {
        let text = if app.logs.items.is_empty() {
            "no chats yet. each partner you're matched with gets their own log here."
        } else {
            "nothing matches"
        };
        frame.render_widget(Paragraph::new(Span::styled(text, t.muted())).wrap(Wrap { trim: false }), list_area);
        return;
    }
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let conv = &app.logs.items[i];
            let when = conv.started.format("%b %d %H:%M").to_string();
            let mut meta = vec![Span::styled(when, t.muted())];
            if conv.is_live() {
                meta.push(Span::styled("  ● live", Style::new().fg(t.success)));
            } else {
                meta.push(Span::styled(format!("  {} msgs", conv.messages), t.muted()));
                if let Some(ended) = conv.ended {
                    let mins = (ended - conv.started).num_minutes();
                    if mins > 0 {
                        meta.push(Span::styled(format!(" · {mins}m"), t.muted()));
                    }
                }
            }
            ListItem::new(vec![Line::from(Span::styled(conv.title(), Style::new().fg(t.fg))), Line::from(meta)])
        })
        .collect();
    let selected = app.logs_ui.list.selected.min(visible.len() - 1);
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list(app, items, !app.logs_ui.reading), list_area, &mut state);
}

fn draw_transcript(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme.clone();
    let Some(index) = app.selected_log() else {
        section(frame, app, area, "transcript", false);
        return;
    };
    // Entries of older chats load when the chat is opened (enter).
    let conv = &app.logs.items[index];
    let title = Line::from(vec![
        Span::raw(conv.title().to_lowercase()),
        Span::styled(format!(" · {}", conv.started.format("%Y-%m-%d %H:%M")), t.muted()),
    ]);
    let inner = section(frame, app, area, title, app.logs_ui.reading);
    let Some(entries) = conv.loaded_entries() else {
        frame.render_widget(Paragraph::new(Span::styled("enter to open this chat", t.muted())), inner);
        return;
    };
    if inner.height == 0 || inner.width < 2 {
        return;
    }
    let chunks = layout_entries(app, entries, inner.width as usize, false);
    let total = total_height(&chunks);
    let max_top = total.saturating_sub(inner.height as usize);
    app.logs_ui.scroll = app.logs_ui.scroll.min(max_top);
    render_chunks(frame, inner, &chunks, app.logs_ui.scroll);
}
