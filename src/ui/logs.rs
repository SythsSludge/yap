//! The logs tab: earlier chats, one per partner.

use super::chat::{View, layout_entries, render_chunks};
use super::{Rows, columns, render_list, section};
use crate::app::App;
use crate::app::ListId;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};

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
    let partners = match app.stats.partners {
        1 => "1 partner".to_owned(),
        n => format!("{n} partners"),
    };
    let saving = format!(
        "{} · {partners} · {} chatting",
        if app.config.settings.save_logs { "saving to disk" } else { "saving off" },
        crate::stats::human_duration(app.stats.chat_secs)
    );
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
            let mut title = Vec::new();
            if conv.pinned {
                title.push(Span::styled("◆ ", Style::new().fg(t.accent)));
            }
            title.push(Span::styled(conv.title(), Style::new().fg(t.fg)));
            if conv.name.is_some() {
                title.push(Span::styled(format!("  {}", conv.partner_title()), t.muted()));
            }
            let mut lines = vec![Line::from(title), Line::from(meta)];
            // When searching, show where the words turned up.
            let needle = app.logs_ui.list.filter.to_lowercase();
            if let Some(excerpt) = conv.text_match(&needle) {
                let width = area.width.saturating_sub(6) as usize;
                lines.push(Line::from(Span::styled(
                    format!("\u{201c}{}\u{201d}", crate::text::truncate(&excerpt, width)),
                    t.muted().add_modifier(Modifier::ITALIC),
                )));
            }
            ListItem::new(lines)
        })
        .collect();
    let selected = app.logs_ui.list.selected.min(visible.len() - 1);
    render_list(frame, app, list_area, items, Rows::new(ListId::Logs, Some(selected), !app.logs_ui.reading));
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
    let (you, partner) = conv.names();
    let view = View {
        you: you.to_lowercase(),
        partner: partner.to_lowercase(),
        typing: false,
        focus: None,
        search: None,
        unsure: &[],
        new_from: None,
        labels: Vec::new(),
        dots: 3,
    };
    let laid = layout_entries(app, entries, inner.width as usize, &view);
    let total = laid.total();
    let max_top = total.saturating_sub(inner.height as usize);
    app.logs_ui.scroll = app.logs_ui.scroll.min(max_top);
    render_chunks(frame, app, inner, &laid.chunks, app.logs_ui.scroll, None);
}
