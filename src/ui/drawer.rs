//! The drawer tab: saved links, tag filter and details with a small preview.

use super::{Rows, columns, render_list, section};
use crate::app::ListId;
use crate::app::{App, Hit, Shelf};
use crate::images::ImageState;
use crate::keymap::Action;
use crate::links::{Trust, check_trust, looks_like_image};
use crate::text::{truncate, wrap};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};
use ratatui_image::{Resize, StatefulImage};

/// The details pane's image preview is capped at this many cells.
const PREVIEW_COLS: u16 = 36;
const PREVIEW_ROWS: u16 = 9;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let cols = columns(frame, app, area, &[Constraint::Percentage(48), Constraint::Percentage(52)]);
    match app.drawer_ui.shelf {
        Shelf::Links => {
            draw_list(frame, app, cols[0]);
            draw_details(frame, app, cols[1]);
        }
        Shelf::Snippets => {
            draw_snippets(frame, app, cols[0]);
            draw_snippet_details(frame, app, cols[1]);
        }
    }
}

/// The title row doubles as the shelf switcher: `links 3 · snippets 2`.
fn shelf_title(frame: &mut Frame, app: &App, area: Rect) -> Rect {
    let t = &app.theme;
    let shelves = [
        (Shelf::Links, format!("links {}", app.drawer.items.len())),
        (Shelf::Snippets, format!("snippets {}", app.drawer.snippets.len())),
    ];
    let mut x = area.x;
    let mut spans = Vec::new();
    for (i, (shelf, label)) in shelves.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", t.muted()));
            x += 3;
        }
        let style = if app.drawer_ui.shelf == shelf {
            Style::new().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            t.muted()
        };
        let w = label.chars().count() as u16;
        app.hit(Rect::new(x, area.y, w.min(area.right().saturating_sub(x)), 1), Hit::Shelf(shelf));
        x += w;
        spans.push(Span::styled(label, style));
    }
    spans.push(Span::styled("   s switches", t.muted().add_modifier(Modifier::DIM)));
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect::new(area.x, area.y, area.width, 1));
    Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1))
}

fn draw_list(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let visible = app.drawer_items();
    let inner = shelf_title(frame, app, area);
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
            Line::from(vec![
                key(&app.keymap.label(Action::Links)),
                dim(" then "),
                key("s"),
                dim("  save a link from the chat"),
            ]),
        ];
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
        return;
    }

    // Tag chips (clickable, laid out by hand so each knows where it is), then an
    // optional search line.
    let mut chips = vec![(chip(app, "all", None, app.drawer_ui.tag.is_none()), None)];
    for (tag, n) in app.drawer.tags() {
        let active = app.drawer_ui.tag.as_deref() == Some(tag.as_str());
        chips.push((chip(app, &format!("#{tag}"), Some(n), active), Some(tag)));
    }
    let (mut cx, mut cy) = (inner.x, inner.y);
    let max_rows = 3;
    for (span, tag) in chips {
        let w = span.width() as u16;
        if cx > inner.x && cx + w > inner.right() {
            cx = inner.x;
            cy += 1;
        }
        if cy >= inner.y + max_rows || w > inner.width {
            break;
        }
        let rect = Rect::new(cx, cy, w, 1);
        frame.render_widget(Paragraph::new(span), rect);
        app.hit(rect, Hit::DrawerTag(tag));
        cx += w + 1;
    }
    let mut y = cy + 1;
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
    render_list(frame, app, list_area, items, Rows::new(ListId::Drawer, Some(selected), true));
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

fn draw_snippets(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let inner = shelf_title(frame, app, area);
    if inner.height < 3 {
        return;
    }
    if app.drawer.snippets.is_empty() {
        let key = |k: &str| Span::styled(k.to_owned(), Style::new().fg(t.fg));
        let dim = |s: &str| Span::styled(s.to_owned(), t.muted());
        let text = vec![
            Line::default(),
            Line::from(Span::styled("no snippets yet", Style::new().fg(t.fg))),
            Line::from(dim("save text you send often: an intro, your limits, a polite goodbye")),
            Line::default(),
            Line::from(vec![key("a"), dim("  add a snippet")]),
            Line::from(vec![key("/snip-add <name> <text>"), dim("  from the chat box")]),
            Line::from(vec![key(&app.keymap.label(Action::Snippets)), dim("  insert one while chatting")]),
            Line::default(),
            Line::from(dim("{species}, {partner_species} and friends are filled in when inserted")),
        ];
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
        return;
    }
    let mut y = inner.y + 1;
    if app.drawer_ui.snippets.filtering || !app.drawer_ui.snippets.filter.is_empty() {
        let cursor = if app.drawer_ui.snippets.filtering { "▏" } else { "" };
        let line = Line::from(vec![
            Span::styled("/ ", t.muted()),
            Span::styled(format!("{}{cursor}", app.drawer_ui.snippets.filter), Style::new().fg(t.fg)),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, inner.y, inner.width, 1));
        y += 1;
    }
    let list_area = Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y));
    let visible = app.visible_snippets();
    if visible.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled("nothing matches", t.muted())), list_area);
        return;
    }
    let width = list_area.width.saturating_sub(3) as usize;
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let s = &app.drawer.snippets[i];
            ListItem::new(vec![
                Line::from(Span::styled(truncate(&s.name, width), Style::new().fg(t.fg))),
                Line::from(Span::styled(truncate(&s.text.replace('\n', " ↵ "), width), t.muted())),
            ])
        })
        .collect();
    let selected = app.drawer_ui.snippets.selected.min(visible.len() - 1);
    render_list(frame, app, list_area, items, Rows::new(ListId::Snippets, Some(selected), true));
}

fn draw_snippet_details(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let inner = section(frame, app, area, "snippet", false);
    let visible = app.visible_snippets();
    let Some(snippet) = visible.get(app.drawer_ui.snippets.selected).map(|&i| &app.drawer.snippets[i]) else {
        return;
    };
    let w = inner.width as usize;
    let mut lines = vec![
        Line::from(vec![
            Span::styled("/snip ", t.muted()),
            Span::styled(snippet.name.clone(), Style::new().fg(t.fg).add_modifier(Modifier::BOLD)),
        ]),
        Line::default(),
    ];
    for line in snippet.text.lines() {
        lines.extend(wrap(&[Span::styled(line.to_owned(), Style::new().fg(t.fg))], w, 0));
    }
    let expanded = crate::drawer::expand_placeholders(&snippet.text, &app.snippet_vars());
    if expanded != snippet.text {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled("right now this would send:", t.muted())));
        let joined = crate::app::join_paragraphs(&expanded, &app.config.settings.paragraph_break);
        lines.extend(wrap(&[Span::styled(joined, t.muted())], w, 2));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "enter insert · e edit in your editor · r rename · d delete",
        t.muted().add_modifier(Modifier::DIM),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}
