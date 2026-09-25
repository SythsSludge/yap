//! The drawer tab: saved links, tag filter and details with a small preview.

use super::{columns, list, section};
use crate::app::App;
use crate::images::ImageState;
use crate::links::{Trust, check_trust, looks_like_image};
use crate::text::{truncate, wrap};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, ListState, Paragraph, Wrap};
use ratatui_image::{Resize, StatefulImage};

/// The details pane's image preview is capped at this many cells.
const PREVIEW_COLS: u16 = 36;
const PREVIEW_ROWS: u16 = 9;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let cols = columns(frame, app, area, &[Constraint::Percentage(48), Constraint::Percentage(52)]);
    draw_list(frame, app, cols[0]);
    draw_details(frame, app, cols[1]);
}

fn draw_list(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let visible = app.drawer_items();
    let title =
        Line::from(vec![Span::raw("drawer "), Span::styled(format!("· {}", app.drawer.items.len()), t.muted())]);
    let inner = section(frame, app, area, title, true);
    if inner.height < 3 {
        return;
    }

    if app.drawer.items.is_empty() {
        let key = |k: &str| Span::styled(k.to_owned(), Style::new().fg(t.fg));
        let dim = |s: &str| Span::styled(s.to_owned(), t.muted());
        let text = vec![
            Line::default(),
            Line::from(Span::styled("your drawer is empty", Style::new().fg(t.fg))),
            Line::from(dim("keep ref sheets, galleries and other links here to share quickly")),
            Line::default(),
            Line::from(vec![key("a"), dim("  add a link")]),
            Line::from(vec![key("/save <url> [label] [#tags]"), dim("  from the chat box")]),
            Line::from(vec![key("^O"), dim(" then "), key("s"), dim("  save a link from the chat")]),
        ];
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
        return;
    }

    // Tag chips, then an optional search line.
    let mut chips = vec![chip(app, "all", None, app.drawer_ui.tag.is_none())];
    for (tag, n) in app.drawer.tags() {
        let active = app.drawer_ui.tag.as_deref() == Some(tag.as_str());
        chips.push(Span::raw(" "));
        chips.push(chip(app, &format!("#{tag}"), Some(n), active));
    }
    let chip_lines = wrap(&chips, inner.width as usize, 0);
    let chip_h = (chip_lines.len() as u16).min(3);
    frame.render_widget(Paragraph::new(chip_lines), Rect::new(inner.x, inner.y, inner.width, chip_h));
    let mut y = inner.y + chip_h;
    if app.drawer_ui.list.filtering || !app.drawer_ui.list.filter.is_empty() {
        let cursor = if app.drawer_ui.list.filtering { "▏" } else { "" };
        let line = Line::from(vec![
            Span::styled("/ ", t.muted()),
            Span::styled(format!("{}{cursor}", app.drawer_ui.list.filter), Style::new().fg(t.fg)),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
        y += 1;
    }
    let list_area = Rect::new(inner.x, y + 1, inner.width, inner.bottom().saturating_sub(y + 1));

    if visible.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled("nothing matches", t.muted())), list_area);
        return;
    }
    let width = list_area.width.saturating_sub(3) as usize;
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let item = &app.drawer.items[i];
            let host =
                url::Url::parse(&item.url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
            let mut second = vec![Span::styled(host, t.muted())];
            for tag in &item.tags {
                second.push(Span::styled(format!("  #{tag}"), Style::new().fg(t.accent).add_modifier(Modifier::DIM)));
            }
            ListItem::new(vec![
                Line::from(Span::styled(truncate(&item.label, width), Style::new().fg(t.fg))),
                Line::from(second),
            ])
        })
        .collect();
    let selected = app.drawer_ui.list.selected.min(visible.len() - 1);
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list(app, items, true), list_area, &mut state);
}

fn chip(app: &App, label: &str, count: Option<usize>, active: bool) -> Span<'static> {
    let t = &app.theme;
    let text = match count {
        Some(n) => format!(" {label} {n} "),
        None => format!(" {label} "),
    };
    if active {
        Span::styled(text, Style::new().fg(t.selection_fg).bg(t.accent))
    } else {
        Span::styled(text, Style::new().fg(t.muted).bg(t.surface))
    }
}

fn draw_details(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = section(frame, app, area, "details", false);
    let Some(index) = app.selected_drawer_item() else {
        app.drawer_preview = None;
        return;
    };
    let item = app.drawer.items[index].clone();
    let t = app.theme.clone();

    let label = |l: &str| Span::styled(format!("{l:<7}"), t.muted());
    let mut lines = vec![
        Line::from(vec![
            label("label"),
            Span::styled(item.label.clone(), Style::new().fg(t.fg).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![label("link"), Span::styled(item.url.clone(), t.link())]),
        Line::from(vec![
            label("tags"),
            if item.tags.is_empty() {
                Span::styled("none · t to add", t.muted())
            } else {
                Span::styled(
                    item.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "),
                    Style::new().fg(t.accent),
                )
            },
        ]),
        Line::from(vec![
            label("added"),
            Span::styled(
                item.added.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
                Style::new().fg(t.fg),
            ),
        ]),
    ];
    if !item.note.is_empty() {
        lines.push(Line::from(vec![label("note"), Span::styled(item.note.clone(), Style::new().fg(t.fg))]));
    }
    let s = &app.config.settings.images;
    let trust = url::Url::parse(&item.url)
        .ok()
        .filter(looks_like_image)
        .map(|u| check_trust(&u, &s.trusted_domains, s.https_only));
    let status = match (trust, app.images.get(&item.url)) {
        (None, _) => "",
        (Some(_), Some(ImageState::Loading)) => "loading preview…",
        (Some(_), Some(ImageState::Failed(_))) => "preview failed",
        (Some(_), Some(ImageState::Ready(_))) => "",
        (Some(Trust::Trusted), None) => "p to load a preview",
        (Some(_), None) => "untrusted host · p asks before loading",
    };
    if !status.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(status, t.muted())));
    }
    let wrapped: Vec<Line> = lines.iter().flat_map(|l| wrap(&l.spans, inner.width as usize, 0)).collect();
    let text_height = wrapped.len() as u16;
    frame.render_widget(Paragraph::new(wrapped), inner);

    // A small, resizable copy of the image, cached per selected item.
    let image = match app.images.get(&item.url) {
        Some(ImageState::Ready(loaded)) => Some(loaded.image.clone()),
        _ => None,
    };
    let Some(image) = image else {
        app.drawer_preview = None;
        return;
    };
    if app.drawer_preview.as_ref().is_none_or(|(url, _)| *url != item.url) {
        app.drawer_preview = Some((item.url.clone(), app.picker.new_resize_protocol(image)));
    }
    let top = inner.y + text_height + 1;
    let avail_h = inner.bottom().saturating_sub(top);
    let rect = Rect::new(inner.x, top, inner.width.min(PREVIEW_COLS), avail_h.min(PREVIEW_ROWS));
    if rect.width > 0
        && rect.height > 0
        && let Some((_, protocol)) = app.drawer_preview.as_mut()
    {
        frame.render_stateful_widget(StatefulImage::default().resize(Resize::Fit(None)), rect, protocol);
    }
}
