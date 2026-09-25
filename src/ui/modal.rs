//! Popups and the full-screen image viewer.

use super::centered;
use crate::app::App;
use crate::app::modal::Modal;
use crate::commands::HELP;
use crate::links::{Trust, check_trust, looks_like_image};
use crate::text::truncate;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::{Resize, StatefulImage};

const KEYS: &[(&str, &str)] = &[
    ("F1", "this help"),
    ("F2–F7 / Alt-1–6", "chat, preferences, drawer, logs, traffic, settings"),
    ("Ctrl-F", "find a partner (asks if you already have one)"),
    ("Ctrl-D", "leave partner / stop searching"),
    ("Ctrl-B", "block current or previous partner"),
    ("Ctrl-O", "links in this chat: open, save, preview, trust"),
    ("Ctrl-E", "drawer panel (Tab to focus, Enter inserts link)"),
    ("Ctrl-P", "switch profile"),
    ("Ctrl-T", "switch theme (live preview)"),
    ("Ctrl-S", "toggle sidebar"),
    ("Ctrl-R", "reconnect"),
    ("Ctrl-Q / Ctrl-C", "quit"),
    ("PgUp/PgDn, Shift-↑/↓", "scroll chat; Esc jumps to newest"),
    ("↑/↓", "message history"),
    ("Ctrl-W / Ctrl-U", "delete word / line"),
];

fn frame_block<'a>(app: &App, title: &'a str) -> Block<'a> {
    let t = &app.theme;
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.muted))
        .title(Span::styled(format!(" {} ", title.to_lowercase()), Style::new().fg(t.accent).bold()))
        .padding(ratatui::widgets::Padding::horizontal(1))
        .style(Style::new().bg(t.surface).fg(t.fg))
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let Some(modal) = &app.modal else { return };
    let key = |k: &str| Span::styled(k.to_owned(), Style::new().fg(t.accent).bold());
    match modal {
        Modal::Confirm { text, .. } => {
            let w = 60.min(area.width.saturating_sub(4));
            let lines = crate::text::wrap(&[Span::raw(text.clone())], w.saturating_sub(6) as usize, 0);
            let h = lines.len() as u16 + 4;
            let rect = centered(area, w, h);
            let mut body = lines;
            body.push(Line::default());
            body.push(Line::from(vec![
                key("y"),
                Span::styled(" yes  ·  ", Style::new().fg(t.muted)),
                key("n"),
                Span::styled(" no", Style::new().fg(t.muted)),
            ]));
            frame.render_widget(Clear, rect);
            frame.render_widget(Paragraph::new(body).block(frame_block(app, "Confirm")), rect);
        }
        Modal::Prompt(prompt) => {
            let w = 72.min(area.width.saturating_sub(4));
            let rect = centered(area, w, 5);
            frame.render_widget(Clear, rect);
            let block = frame_block(app, &prompt.title);
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            // Keep the cursor visible in long paths by scrolling horizontally.
            let (rows, (_, col)) = prompt.editor.layout(usize::MAX / 2);
            let text = rows.concat();
            let avail = inner.width.saturating_sub(1) as usize;
            let skip = col.saturating_sub(avail);
            let shown: String = text.chars().skip(skip).collect();
            frame.render_widget(Paragraph::new(shown), Rect::new(inner.x, inner.y, inner.width, 1));
            frame.render_widget(
                Paragraph::new(Line::from(vec![key("Enter"), Span::raw(" ok   "), key("Esc"), Span::raw(" cancel")]))
                    .style(Style::new().fg(t.muted)),
                Rect::new(inner.x, inner.y + 2, inner.width, 1),
            );
            frame.set_cursor_position(Position::new(inner.x + (col - skip) as u16, inner.y));
        }
        Modal::Help { scroll } => {
            let rect = centered(area, 84, area.height.saturating_sub(2));
            let mut lines = vec![Line::from(Span::styled("keys", Style::new().fg(t.accent).bold()))];
            for (k, what) in KEYS {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {k:<24}"), Style::new().fg(t.fg).bold()),
                    Span::raw(*what),
                ]));
            }
            lines.push(Line::default());
            lines
                .push(Line::from(Span::styled("commands (type in the message box)", Style::new().fg(t.accent).bold())));
            for (cmd, what) in HELP {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {cmd:<24}"), Style::new().fg(t.fg).bold()),
                    Span::raw(*what),
                ]));
            }
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "Import from the website: run copy(JSON.stringify(localStorage)) in the browser console on \
                 yiffspot.com, paste into a .json file, then /import it.",
                Style::new().fg(t.muted),
            )));
            frame.render_widget(Clear, rect);
            frame.render_widget(
                Paragraph::new(lines)
                    .block(frame_block(app, "Help · ↑/↓ scroll"))
                    .wrap(Wrap { trim: false })
                    .scroll((*scroll, 0)),
                rect,
            );
        }
        Modal::Links { links, selected } => {
            let rect = centered(area, 90, (links.len() as u16 + 4).min(area.height.saturating_sub(2)));
            let s = &app.config.settings.images;
            let w = rect.width.saturating_sub(16) as usize;
            let items: Vec<ListItem> = links
                .iter()
                .map(|l| {
                    let who = if l.from_partner {
                        Span::styled("partner ", Style::new().fg(t.partner))
                    } else {
                        Span::styled("you     ", Style::new().fg(t.you))
                    };
                    let tag = url::Url::parse(&l.url)
                        .ok()
                        .filter(looks_like_image)
                        .map(|u| match check_trust(&u, &s.trusted_domains, s.https_only) {
                            Trust::Trusted => Span::styled("img ", Style::new().fg(t.success)),
                            _ => Span::styled("img?", Style::new().fg(t.warning)),
                        })
                        .unwrap_or_else(|| Span::raw("    "));
                    ListItem::new(Line::from(vec![
                        who,
                        tag,
                        Span::raw(" "),
                        Span::styled(truncate(&l.url, w), t.link()),
                    ]))
                })
                .collect();
            let mut state = ListState::default().with_selected(Some(*selected));
            let block = frame_block(app, "Links").title_bottom(Line::from(vec![
                Span::raw(" "),
                key("Enter"),
                Span::raw(" open  "),
                key("p"),
                Span::raw(" preview  "),
                key("s"),
                Span::raw(" save  "),
                key("i"),
                Span::raw(" insert  "),
                key("y"),
                Span::raw(" copy  "),
                key("t"),
                Span::raw(" trust host "),
            ]));
            frame.render_widget(Clear, rect);
            frame.render_stateful_widget(super::list(app, items, true).block(block), rect, &mut state);
        }
        Modal::Profiles { selected } => {
            let rect = centered(area, 40, (app.config.profiles.len() as u16 + 2).min(area.height));
            let items: Vec<ListItem> = app
                .config
                .profiles
                .iter()
                .map(|p| {
                    let active = p.name == app.config.active_profile;
                    ListItem::new(format!("{} {}", if active { "●" } else { " " }, p.name))
                })
                .collect();
            let mut state = ListState::default().with_selected(Some(*selected));
            frame.render_widget(Clear, rect);
            frame.render_stateful_widget(
                super::list(app, items, true).block(frame_block(app, "Profile")),
                rect,
                &mut state,
            );
        }
        Modal::Themes { selected, .. } => {
            let rect = centered(area, 44, (app.themes.len() as u16 + 2).min(area.height));
            let swatch = |c: Color| Span::styled("██", Style::new().fg(c));
            let items: Vec<ListItem> = app
                .themes
                .iter()
                .map(|th| {
                    ListItem::new(Line::from(vec![
                        Span::styled("  ", Style::new().bg(th.bg)),
                        swatch(th.accent),
                        swatch(th.you),
                        swatch(th.partner),
                        swatch(th.highlight),
                        Span::raw(format!("  {}", th.name)),
                    ]))
                })
                .collect();
            let mut state = ListState::default().with_selected(Some(*selected));
            frame.render_widget(Clear, rect);
            frame.render_stateful_widget(
                super::list(app, items, true).block(frame_block(app, "Theme")),
                rect,
                &mut state,
            );
        }
    }
}

pub fn viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(viewer) = app.viewer.as_mut() else { return };
    let t = app.theme.clone();
    let rect = area.inner(ratatui::layout::Margin::new(2, 1));
    frame.render_widget(Clear, rect);
    let title = truncate(&viewer.url, rect.width.saturating_sub(6) as usize);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.muted))
        .title(Span::styled(format!(" {title} "), Style::new().fg(t.muted)))
        .style(Style::new().bg(t.bg));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    match viewer.protocol.as_mut() {
        Some(protocol) => {
            frame.render_stateful_widget(StatefulImage::default().resize(Resize::Fit(None)), inner, protocol)
        }
        None => frame.render_widget(
            Paragraph::new(Span::styled("Loading…", Style::new().fg(t.muted))).centered(),
            centered(inner, inner.width, 1),
        ),
    }
}
