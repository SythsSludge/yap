//! The drawer tab: saved links and their details.

use super::pane;
use crate::app::App;
use crate::images::ImageState;
use crate::links::{Trust, check_trust, looks_like_image};
use crate::text::truncate;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::sliced::{SignedPosition, SlicedImage};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
    let visible = app.drawer.filtered(&app.drawer_ui.filter);

    let mut block = pane(app, format!(" Drawer · {} ", app.drawer.items.len()), true);
    if app.drawer_ui.filtering || !app.drawer_ui.filter.is_empty() {
        let cursor = if app.drawer_ui.filtering { "▏" } else { "" };
        block = block.title_bottom(Line::from(Span::styled(
            format!(" / {}{cursor} ", app.drawer_ui.filter),
            Style::new().fg(t.accent),
        )));
    }

    if app.drawer.items.is_empty() {
        let text = vec![
            Line::from(Span::styled("Your drawer is empty.", Style::new().fg(t.fg).bold())),
            Line::default(),
            Line::from(Span::styled("Keep ref sheets, galleries and other links here to share quickly.", t.muted())),
            Line::default(),
            Line::from(vec![
                Span::styled("a", Style::new().fg(t.accent).bold()),
                Span::styled(" add a link", t.muted()),
            ]),
            Line::from(vec![
                Span::styled("/save <url> [label]", Style::new().fg(t.accent).bold()),
                Span::styled(" from the chat box", t.muted()),
            ]),
            Line::from(vec![
                Span::styled("Ctrl-O", Style::new().fg(t.accent).bold()),
                Span::styled(" then ", t.muted()),
                Span::styled("s", Style::new().fg(t.accent).bold()),
                Span::styled(" to save a link from the chat", t.muted()),
            ]),
        ];
        frame.render_widget(Paragraph::new(text).block(block).wrap(Wrap { trim: false }), list_area);
        frame.render_widget(pane(app, " Details ", false), detail_area);
        return;
    }

    let width = list_area.width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let item = &app.drawer.items[i];
            let host =
                url::Url::parse(&item.url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
            ListItem::new(vec![
                Line::from(Span::styled(truncate(&item.label, width), Style::new().fg(t.fg).bold())),
                Line::from(Span::styled(truncate(&host, width), t.muted())),
            ])
        })
        .collect();
    let selected = (!visible.is_empty()).then(|| app.drawer_ui.selected.min(visible.len() - 1));
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(t.selected()).highlight_symbol("▍"),
        list_area,
        &mut state,
    );

    let block = pane(app, " Details ", false);
    let inner = block.inner(detail_area);
    frame.render_widget(block, detail_area);
    let Some(item) = app.selected_drawer_item().map(|i| &app.drawer.items[i]) else { return };

    let label = |l: &str| Span::styled(format!("{l:<7}"), t.muted());
    let mut lines = vec![
        Line::from(vec![label("Label"), Span::styled(item.label.clone(), Style::new().fg(t.fg).bold())]),
        Line::from(vec![label("Link"), Span::styled(item.url.clone(), t.link())]),
        Line::from(vec![
            label("Added"),
            Span::styled(
                item.added.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
                Style::new().fg(t.fg),
            ),
        ]),
    ];
    if !item.note.is_empty() {
        lines.push(Line::from(vec![label("Note"), Span::styled(item.note.clone(), Style::new().fg(t.fg))]));
    }
    let s = &app.config.settings.images;
    let preview = url::Url::parse(&item.url)
        .ok()
        .filter(looks_like_image)
        .map(|u| check_trust(&u, &s.trusted_domains, s.https_only));
    let status = match (preview, app.images.get(&item.url)) {
        (None, _) => "Not an image link.",
        (Some(_), Some(ImageState::Loading)) => "Loading preview…",
        (Some(_), Some(ImageState::Failed(_))) => "Preview failed.",
        (Some(_), Some(ImageState::Ready(_))) => "",
        (Some(Trust::Trusted), None) => "p to load a preview.",
        (Some(_), None) => "Untrusted host — p asks before loading.",
    };
    if !status.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(status, t.muted())));
    }
    let wrapped: Vec<Line> = lines.iter().flat_map(|l| crate::text::wrap(&l.spans, inner.width as usize, 0)).collect();
    let text_height = wrapped.len() as u16;
    frame.render_widget(Paragraph::new(wrapped), inner);

    if let Some(ImageState::Ready(loaded)) = app.images.get(&item.url) {
        let area =
            Rect::new(inner.x, inner.y + text_height + 1, inner.width, inner.height.saturating_sub(text_height + 1));
        frame.render_widget(SlicedImage::new(&loaded.inline, SignedPosition { x: 0, y: 0 }), area);
    }
}
