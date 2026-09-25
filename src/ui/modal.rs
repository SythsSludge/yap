//! Popups and the full-screen image viewer.

use super::centered;
use crate::app::modal::Modal;
use crate::app::{App, Hit, ListId, ViewerButton};
use crate::catalog::ANY;
use crate::commands::HELP;
use crate::history::Outcome;
use crate::links::{Trust, check_trust, looks_like_image};
use crate::text::{truncate, width};
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

/// The history popup: patterns first, then everyone you've met, newest first.
fn history_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    use crate::stats::human_duration as dur;
    let t = &app.theme;
    let heading = |s: &str| Line::from(Span::styled(s.to_owned(), Style::new().fg(t.accent).bold()));
    let row = |label: &str, value: String| {
        let spans = [Span::styled(format!("{label:<18}"), Style::new().fg(t.muted)), Span::raw(value)];
        crate::text::wrap(&spans, width, 0)
    };
    let history = &app.history;
    if history.records.is_empty() {
        let why = if app.config.settings.keep_history {
            "Nobody yet. Everyone you're matched with shows up here: who they were, how it ended and how long it lasted."
        } else {
            "Partner history is off (Settings → Keep partner history)."
        };
        return crate::text::wrap(&[Span::styled(why, Style::new().fg(t.muted))], width, 0);
    }
    let s = history.summary();
    let mut lines = vec![heading("patterns")];
    let mut overall = format!("met {} · average chat {}", s.met, dur(s.average_secs));
    if s.skipped > 0 {
        overall.push_str(&format!(" · auto-skipped {}", s.skipped));
    }
    if s.silent > 0 {
        overall.push_str(&format!(" · {} ended before anyone spoke", s.silent));
    }
    lines.extend(row("overall", overall));
    let ended: Vec<String> = s
        .outcomes
        .iter()
        .filter(|(o, _)| *o != Outcome::Skipped)
        .map(|(o, n)| format!("{} {n}", o.describe()))
        .collect();
    if !ended.is_empty() {
        lines.extend(row("how chats ended", ended.join(" · ")));
    }
    let species: Vec<String> =
        s.species.iter().map(|(name, n, secs)| format!("{name} ({n}, avg {})", dur(*secs))).collect();
    if !species.is_empty() {
        lines.extend(row("most met", species.join(" · ")));
    }
    let buckets: Vec<String> = ["none", "1–2", "3+"]
        .iter()
        .zip(s.by_shared)
        .filter(|(_, (n, _))| *n > 0)
        .map(|(label, (n, secs))| format!("{label}: avg {} ({n})", dur(secs)))
        .collect();
    if !buckets.is_empty() {
        lines.extend(row("by shared kinks", buckets.join(" · ")));
    }
    let rules: Vec<String> = s.skip_rules.iter().map(|(r, n)| format!("{} {n}", r.describe())).collect();
    if !rules.is_empty() {
        lines.extend(row("auto-skips for", rules.join(" · ")));
    }

    lines.push(Line::default());
    lines.push(heading("everyone, newest first"));
    let who_w = width.saturating_sub(13 + 8 + 10 + 28).clamp(12, 40);
    for r in history.records.iter().rev().take(1000) {
        let skipped = r.outcome == Outcome::Skipped;
        let outcome = match r.skip {
            Some(rule) if skipped => format!("skipped: {}", rule.short()),
            _ => r.outcome.describe().to_owned(),
        };
        let style = Style::new().fg(if skipped { t.muted } else { t.fg });
        let length = if skipped { String::new() } else { dur(r.secs) };
        let talk = if skipped { String::new() } else { format!("{}↑ {}↓", r.sent, r.received) };
        lines.push(Line::from(vec![
            Span::styled(format!("{}  ", r.at.format("%m-%d %H:%M")), Style::new().fg(t.muted)),
            Span::styled(format!("{:<who_w$}", truncate(&r.partner(), who_w)), Style::new().fg(t.fg)),
            Span::styled(format!("{length:>7} "), Style::new().fg(t.fg)),
            Span::styled(format!("{talk:>8}  "), Style::new().fg(t.muted)),
            Span::styled(outcome, style),
        ]));
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
                Line::from(vec![
                    Span::styled("kept on this machine only · ", Style::new().fg(t.muted)),
                    key("h"),
                    Span::styled(" partner history", Style::new().fg(t.muted)),
                ]),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Spelling { word, suggestions, selected, .. } => {
            let rows = suggestions.len() + 1;
            let w = 44.min(area.width.saturating_sub(4));
            let rect = centered(area, w, (rows as u16 + 4).min(area.height.saturating_sub(2)));
            frame.render_widget(Clear, rect);
            let block = frame_block(app, "Spelling");
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            let head = Line::from(vec![
                Span::styled(truncate(word, inner.width as usize - 2), Style::new().fg(t.error).underlined()),
                Span::styled(if suggestions.is_empty() { "  no suggestions" } else { "" }, Style::new().fg(t.muted)),
            ]);
            frame.render_widget(Paragraph::new(head), Rect::new(inner.x, inner.y, inner.width, 1));
            let mut items: Vec<ListItem> = suggestions.iter().map(|s| ListItem::new(s.clone())).collect();
            items.push(ListItem::new(Span::styled(
                format!("+ add “{word}” to your dictionary"),
                Style::new().fg(t.muted),
            )));
            let list = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(2));
            super::render_list(frame, app, list, items, super::Rows::new(ListId::Modal, Some(*selected), true));
        }
        Modal::History { scroll } => {
            let rect = centered(area, 96, area.height.saturating_sub(2));
            let block = frame_block(app, "History · ↑/↓ scroll · x forget");
            let width = block.inner(rect).width as usize;
            let lines = history_lines(app, width);
            frame.render_widget(Clear, rect);
            frame.render_widget(Paragraph::new(lines).block(block).scroll((*scroll, 0)), rect);
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

/// `1.2 MB`, `340 KB`.
fn human_bytes(n: usize) -> String {
    match n {
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1 << 20) as f64),
        n if n >= 1 << 10 => format!("{} KB", n >> 10),
        n => format!("{n} B"),
    }
}

/// The full-screen image viewer: the picture centred, where it came from along the
/// top, and ←/→ through the chat's other images.
pub fn viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::images::{ImageState, ViewerNote};
    let Some(viewer) = app.viewer.as_ref() else { return };
    let t = app.theme.clone();
    let url = viewer.url.clone();
    let note = viewer.note.clone();
    let ready = viewer.protocol.is_some();
    let position = app.viewer_position();
    let parsed = url::Url::parse(&url).ok();
    let host = parsed.as_ref().and_then(|u| u.host_str()).unwrap_or_default().to_owned();

    let rect = area.inner(ratatui::layout::Margin::new(2, 1));
    frame.render_widget(Clear, rect);
    let mut title = vec![Span::raw(" ")];
    if let Some((i, len)) = position {
        title.push(Span::styled(format!("{} / {len}", i + 1), Style::new().fg(t.accent).bold()));
        let entry = &app.chat.entries[app.chat_images()[i].1];
        let who = if matches!(entry.kind, crate::app::chat::EntryKind::You(_)) {
            app.my_label()
        } else {
            app.partner_label()
        };
        title.push(Span::styled(format!(" · from {who} · {} ", entry.at.format("%H:%M")), Style::new().fg(t.muted)));
    } else {
        title.push(Span::styled("image ", Style::new().fg(t.muted)));
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.muted))
        .title(Line::from(title))
        .title(Line::from(Span::styled(format!(" {host} "), Style::new().fg(t.muted))).right_aligned())
        .style(Style::new().bg(t.bg));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.height < 4 || inner.width < 12 {
        return;
    }

    // Bottom: a line about the file, then the buttons. Sides: arrows to step.
    let info_y = inner.bottom() - 2;
    let bar_y = inner.bottom() - 1;
    let pic = Rect::new(inner.x + 2, inner.y, inner.width - 4, inner.height.saturating_sub(3));
    let (has_prev, has_next) = position.map_or((false, false), |(i, len)| (i > 0, i + 1 < len));
    let mid = pic.y + pic.height / 2;
    for (show, x, glyph, button) in
        [(has_prev, inner.x, "‹", ViewerButton::Prev), (has_next, inner.right() - 1, "›", ViewerButton::Next)]
    {
        if show {
            frame.render_widget(
                Paragraph::new(Span::styled(glyph, Style::new().fg(t.accent).bold())),
                Rect::new(x, mid, 1, 1),
            );
            app.hit(Rect::new(x.saturating_sub(u16::from(x > inner.x)), pic.y, 2, pic.height), Hit::Viewer(button));
        }
    }

    let info = match app.images.get(&url) {
        Some(ImageState::Ready(loaded)) => {
            let name = crate::images::file_name_for(&url, &loaded.bytes);
            format!("{name} · {}×{} · {}", loaded.image.width(), loaded.image.height(), human_bytes(loaded.bytes.len()))
        }
        _ => truncate(&url, inner.width as usize),
    };
    frame.render_widget(
        Paragraph::new(Span::styled(truncate(&info, inner.width as usize), Style::new().fg(t.muted))).centered(),
        Rect::new(inner.x, info_y, inner.width, 1),
    );

    let untrusted = matches!(note, Some(ViewerNote::Untrusted { .. }));
    let mut buttons: Vec<(&str, &str, ViewerButton)> = Vec::new();
    if has_prev {
        buttons.push(("←", "prev", ViewerButton::Prev));
    }
    if has_next {
        buttons.push(("→", "next", ViewerButton::Next));
    }
    if untrusted {
        buttons.push(("enter", "load once", ViewerButton::LoadOnce));
        buttons.push(("t", "trust host", ViewerButton::Trust));
    }
    buttons.push(("o", "open", ViewerButton::Open));
    buttons.push(("y", "copy link", ViewerButton::CopyLink));
    buttons.push(("s", "save to drawer", ViewerButton::SaveToDrawer));
    if ready {
        buttons.push(("d", "save image", ViewerButton::SaveImage));
    }
    buttons.push(("esc", "close", ViewerButton::Close));
    // Drop buttons from the middle of the list until the bar fits.
    let chip_w = |(key, label, _): &(&str, &str, ViewerButton)| (width(key) + 1 + width(label)) as u16;
    while buttons.len() > 2 && buttons.iter().map(|b| chip_w(b) + 3).sum::<u16>() > inner.width {
        buttons.remove(buttons.len() - 2);
    }
    let total: u16 = buttons.iter().map(|b| chip_w(b) + 3).sum::<u16>().saturating_sub(3);
    let mut x = inner.x + inner.width.saturating_sub(total) / 2;
    let mut spans = vec![Span::raw(" ".repeat((x - inner.x) as usize))];
    for (i, b) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
            x += 3;
        }
        let (key, label, button) = *b;
        app.hit(Rect::new(x, bar_y, chip_w(b), 1), Hit::Viewer(button));
        spans.push(Span::styled(key, Style::new().fg(t.accent).bold()));
        spans.push(Span::styled(format!(" {label}"), Style::new().fg(t.fg)));
        x += chip_w(b);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect::new(inner.x, bar_y, inner.width, 1));

    let message = |lines: Vec<Line<'static>>| {
        let h = lines.len() as u16;
        (
            Paragraph::new(lines).centered().wrap(Wrap { trim: true }),
            Rect::new(pic.x, mid.saturating_sub(h / 2), pic.width, h),
        )
    };
    let Some(viewer) = app.viewer.as_mut() else { return };
    match (viewer.protocol.as_mut(), note) {
        (Some(protocol), _) => {
            let size = protocol.size_for(Resize::Fit(None), ratatui::layout::Size::new(pic.width, pic.height));
            let at = Rect::new(
                pic.x + pic.width.saturating_sub(size.width) / 2,
                pic.y + pic.height.saturating_sub(size.height) / 2,
                size.width.min(pic.width),
                size.height.min(pic.height),
            );
            frame.render_stateful_widget(StatefulImage::default().resize(Resize::Fit(None)), at, protocol)
        }
        (None, Some(ViewerNote::Untrusted { host })) => {
            let (text, at) = message(vec![
                Line::from(Span::styled(format!("{host} isn't a trusted image host"), Style::new().fg(t.fg).bold())),
                Line::from(Span::styled("Loading it shows them your IP address.", Style::new().fg(t.muted))),
            ]);
            frame.render_widget(text, at);
        }
        (None, Some(ViewerNote::Failed(e))) => {
            let (text, at) = message(vec![
                Line::from(Span::styled("Couldn't load this image", Style::new().fg(t.error).bold())),
                Line::from(Span::styled(e, Style::new().fg(t.muted))),
            ]);
            frame.render_widget(text, at);
        }
        (None, None) => {
            let (text, at) =
                message(vec![Line::from(Span::styled(format!("loading from {host}…"), Style::new().fg(t.muted)))]);
            frame.render_widget(text, at);
        }
    }
}
