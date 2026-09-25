//! Popups and the full-screen image viewer.

use super::centered;
use crate::app::modal::Modal;
use crate::app::{App, Hit, ListId, ViewerButton};
use crate::catalog::ANY;
use crate::commands::HELP;
use crate::links::{Trust, check_trust, looks_like_image};
use crate::text::truncate;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, ListItem, Paragraph, Wrap};
use ratatui_image::{Resize, StatefulImage};

/// Keys that aren't rebindable, shown under the rebindable ones.
const FIXED_KEYS: &[(&str, &str)] = &[
    ("ctrl+c", "quit (always works; closes a popup first)"),
    ("esc", "back / cancel; in chat, jump to the newest message"),
    ("↑/↓", "message history"),
    ("ctrl+w / ctrl+u", "delete word / line"),
    ("tab", "switch pane or focus"),
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

/// The kinks popup: the partner's list against yours, each with its definition.
fn kink_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let t = &app.theme;
    let mut lines = Vec::new();
    let mut group = |heading: String, kinks: &[String], name_style: Style| {
        if kinks.is_empty() {
            return;
        }
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(heading, Style::new().fg(t.accent).bold())));
        for kink in kinks {
            let (name, meaning) = if kink == ANY {
                ("Any / All", "Open to anything on the list.")
            } else {
                (kink.as_str(), crate::glossary::define(kink).unwrap_or(""))
            };
            let spans = [
                Span::styled(format!("{name}  "), name_style),
                Span::styled(meaning.to_owned(), Style::new().fg(t.muted)),
            ];
            // Hanging indent: definitions that wrap line up under the name.
            lines.extend(crate::text::wrap(&spans, width.saturating_sub(2), 2).into_iter().enumerate().map(
                |(i, mut l)| {
                    if i > 0 {
                        l.spans.insert(0, Span::raw("  "));
                    }
                    l
                },
            ));
        }
    };
    let strong = Style::new().fg(t.fg).bold();
    match app.kink_groups() {
        Some(g) => {
            let shared = Style::new().fg(t.highlight).bold();
            group(format!("shared · {}", g.shared.len()), &g.shared, shared);
            group(format!("theirs only · {}", g.theirs.len()), &g.theirs, strong);
            group(format!("yours, not on their list · {}", g.mine.len()), &g.mine, strong);
        }
        None => {
            let mine = &app.config.active().preferences.kinks;
            group(format!("your kinks · {}", app.config.active_profile), mine, strong);
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "Once you're matched, this compares your partner's list with yours.",
                Style::new().fg(t.muted),
            )));
        }
    }
    lines
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
        Modal::Stats => {
            use crate::stats::human_duration;
            let rect = centered(area, 60, 14);
            frame.render_widget(Clear, rect);
            let block = frame_block(app, "Stats");
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            let (run, all) = (&app.run_stats, &app.stats);
            let row = |label: &str, now: String, ever: String| {
                Line::from(vec![
                    Span::styled(format!("{label:<22}"), Style::new().fg(t.muted)),
                    Span::styled(format!("{now:>12}"), Style::new().fg(t.fg)),
                    Span::styled(format!("{ever:>14}"), Style::new().fg(t.fg).bold()),
                ])
            };
            let since = all.since.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_else(|| "—".into());
            let lines = vec![
                Line::from(vec![
                    Span::raw(" ".repeat(22)),
                    Span::styled(format!("{:>12}", "this session"), Style::new().fg(t.muted)),
                    Span::styled(format!("{:>14}", format!("since {since}")), Style::new().fg(t.muted)),
                ]),
                Line::default(),
                row("partners met", run.partners.to_string(), all.partners.to_string()),
                row("auto-skipped", run.skipped.to_string(), all.skipped.to_string()),
                row("messages sent", run.sent.to_string(), all.sent.to_string()),
                row("messages received", run.received.to_string(), all.received.to_string()),
                row("time chatting", human_duration(run.chat_secs), human_duration(all.chat_secs)),
                row("average chat", human_duration(run.average_secs()), human_duration(all.average_secs())),
                row("longest chat", human_duration(run.longest_secs), human_duration(all.longest_secs)),
                Line::default(),
                Line::from(Span::styled("kept on this machine only", Style::new().fg(t.muted))),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Kinks { scroll } => {
            let rect = centered(area, 80, area.height.saturating_sub(2));
            let block = frame_block(app, "Kinks · ↑/↓ scroll");
            let width = block.inner(rect).width as usize;
            let lines = kink_lines(app, width);
            frame.render_widget(Clear, rect);
            frame.render_widget(Paragraph::new(lines).block(block).scroll((*scroll, 0)), rect);
        }
        Modal::Palette { query, selected } => {
            let matches = app.palette_matches(query);
            let w = 84.min(area.width.saturating_sub(4));
            let h = (matches.len() as u16 + 4).clamp(6, 20).min(area.height.saturating_sub(2));
            // Sit in the upper third, like Claude Code's.
            let rect = Rect::new(area.x + (area.width - w) / 2, area.y + area.height / 6, w, h);
            frame.render_widget(Clear, rect);
            let block = frame_block(app, "Palette");
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            if inner.height < 2 {
                return;
            }
            let input = Line::from(vec![
                Span::styled("> ", Style::new().fg(t.accent).bold()),
                Span::styled(query.clone(), Style::new().fg(t.fg)),
                Span::styled(
                    if query.is_empty() { "search actions, commands, settings, snippets…" } else { "" },
                    Style::new().fg(t.muted),
                ),
            ]);
            frame.render_widget(Paragraph::new(input), Rect::new(inner.x, inner.y, inner.width, 1));
            frame.set_cursor_position(ratatui::layout::Position::new(
                inner.x + 2 + (crate::text::width(query) as u16).min(inner.width.saturating_sub(3)),
                inner.y,
            ));
            let list_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(2));
            let label_w = list_area.width.saturating_sub(24) as usize;
            let items: Vec<ListItem> = matches
                .iter()
                .map(|e| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:<w$}", truncate(&e.label, label_w), w = label_w),
                            Style::new().fg(t.fg),
                        ),
                        Span::styled(format!(" {:>8}", e.kind), Style::new().fg(t.muted)),
                        Span::styled(format!(" {:>10}", truncate(&e.hint, 10)), Style::new().fg(t.accent)),
                    ]))
                })
                .collect();
            if items.is_empty() {
                frame.render_widget(
                    Paragraph::new(Span::styled("nothing matches", Style::new().fg(t.muted))),
                    list_area,
                );
            } else {
                let selected = Some((*selected).min(items.len() - 1));
                super::render_list(frame, app, list_area, items, super::Rows::new(ListId::Modal, selected, true));
            }
        }
        Modal::Snippets { filter, selected } => {
            let visible = app.drawer.filtered_snippets(filter);
            let h = (visible.len() as u16 + 5).clamp(7, area.height.saturating_sub(2));
            let rect = centered(area, 80, h);
            frame.render_widget(Clear, rect);
            let block = frame_block(app, "Snippets").title_bottom(Line::from(vec![
                Span::raw(" "),
                key("enter"),
                Span::styled(" insert  ", Style::new().fg(t.muted)),
                key("esc"),
                Span::styled(" close ", Style::new().fg(t.muted)),
            ]));
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            if inner.height < 2 {
                return;
            }
            let filter_line = Line::from(vec![
                Span::styled("/ ", Style::new().fg(t.muted)),
                Span::styled(format!("{filter}▏"), Style::new().fg(t.fg)),
            ]);
            frame.render_widget(Paragraph::new(filter_line), Rect::new(inner.x, inner.y, inner.width, 1));
            let list_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(2));
            let w = list_area.width.saturating_sub(18) as usize;
            let items: Vec<ListItem> = visible
                .iter()
                .map(|&i| {
                    let s = &app.drawer.snippets[i];
                    let preview = truncate(&s.text.replace('\n', " ↵ "), w);
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<14}", truncate(&s.name, 14)), Style::new().fg(t.fg).bold()),
                        Span::styled(preview, Style::new().fg(t.muted)),
                    ]))
                })
                .collect();
            if items.is_empty() {
                frame.render_widget(Paragraph::new(Span::styled("no matches", Style::new().fg(t.muted))), list_area);
            } else {
                let selected = Some((*selected).min(items.len() - 1));
                super::render_list(frame, app, list_area, items, super::Rows::new(ListId::Modal, selected, true));
            }
        }
        Modal::CaptureKey { action } => {
            let rect = centered(area, 56, 6);
            let body = vec![
                Line::from(vec![
                    Span::raw("Press the new key for "),
                    Span::styled(action.describe().to_lowercase(), Style::new().fg(t.fg).bold()),
                ]),
                Line::from(Span::styled(
                    format!("currently {}", app.keymap.hint(*action).unwrap_or_else(|| "unbound".into())),
                    Style::new().fg(t.muted),
                )),
                Line::default(),
                Line::from(vec![key("esc"), Span::styled(" cancel", Style::new().fg(t.muted))]),
            ];
            frame.render_widget(Clear, rect);
            frame.render_widget(Paragraph::new(body).block(frame_block(app, "Rebind")), rect);
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
            let mut lines = vec![Line::from(vec![
                Span::styled("keys", Style::new().fg(t.accent).bold()),
                Span::styled("  (change them in settings)", Style::new().fg(t.muted)),
            ])];
            for &action in crate::keymap::Action::ALL {
                let keys = app.keymap.keys(action);
                let shown = if keys.is_empty() {
                    "unbound".to_owned()
                } else {
                    keys.iter().map(ToString::to_string).collect::<Vec<_>>().join(" / ")
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {shown:<24}"), Style::new().fg(t.fg).bold()),
                    Span::raw(action.describe().to_lowercase()),
                ]));
            }
            for (k, what) in FIXED_KEYS {
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
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            super::render_list(frame, app, inner, items, super::Rows::new(ListId::Modal, Some(*selected), true));
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
            frame.render_widget(Clear, rect);
            let block = frame_block(app, "Profile");
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            super::render_list(frame, app, inner, items, super::Rows::new(ListId::Modal, Some(*selected), true));
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
            frame.render_widget(Clear, rect);
            let block = frame_block(app, "Theme");
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            super::render_list(frame, app, inner, items, super::Rows::new(ListId::Modal, Some(*selected), true));
        }
    }
}

pub fn viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(viewer) = app.viewer.as_ref() else { return };
    let t = app.theme.clone();
    let rect = area.inner(ratatui::layout::Margin::new(2, 1));
    frame.render_widget(Clear, rect);
    let title = truncate(&viewer.url, rect.width.saturating_sub(6) as usize);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.muted))
        .title(Span::styled(format!(" {title} "), Style::new().fg(t.muted)))
        .style(Style::new().bg(t.bg));
    let mut inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.height > 3 {
        // A row of buttons along the bottom.
        let bar = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        inner.height -= 2;
        let buttons = [
            ("open", "o", ViewerButton::Open),
            ("copy link", "y", ViewerButton::CopyLink),
            ("save to drawer", "s", ViewerButton::SaveToDrawer),
            ("save image", "d", ViewerButton::SaveImage),
            ("close", "esc", ViewerButton::Close),
        ];
        let mut x = bar.x;
        let mut spans = Vec::new();
        for (label, key, button) in buttons {
            let text = format!(" {label} ");
            let w = text.chars().count() as u16;
            if x + w + key.len() as u16 + 2 > bar.right() {
                break;
            }
            app.hit(Rect::new(x, bar.y, w, 1), Hit::Viewer(button));
            spans.push(Span::styled(text, Style::new().fg(t.fg).bg(t.surface)));
            spans.push(Span::styled(format!(" {key}  "), Style::new().fg(t.muted)));
            x += w + key.len() as u16 + 3;
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), bar);
    }
    let Some(viewer) = app.viewer.as_mut() else { return };
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
