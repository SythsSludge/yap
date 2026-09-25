//! Working with the chat transcript itself: selecting messages and searching.

use super::chat::{ChatMode, Search};
use super::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// How much of a message a quote keeps.
const QUOTE_CHARS: usize = 80;

/// `> "the start of their message…" ` for replying to something specific.
pub fn quote(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let excerpt = if flat.chars().count() > QUOTE_CHARS {
        let cut: String = flat.chars().take(QUOTE_CHARS).collect();
        // Don't end mid-word if there's a space to break at.
        let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head).to_owned();
        format!("{cut}…")
    } else {
        flat
    };
    format!("> \"{excerpt}\" ")
}

impl App {
    /// Start selecting messages, from `entry` or the newest message.
    pub fn select_message(&mut self, entry: Option<usize>) {
        let Some(entry) = entry.or_else(|| self.chat.message_indices().last().copied()) else {
            return self.toast(Level::Info, "No messages to select yet.");
        };
        self.tab = Tab::Chat;
        self.chat.mode = ChatMode::Select(entry);
    }

    /// Start (or resume) searching this chat.
    pub fn start_search(&mut self, query: Option<&str>) {
        self.tab = Tab::Chat;
        let mut search = match &self.chat.mode {
            ChatMode::Search(s) => s.clone(),
            _ => Search::default(),
        };
        if let Some(q) = query {
            search.query = q.to_owned();
            search.editing = false;
        } else {
            search.editing = true;
        }
        self.chat.mode = ChatMode::Search(search);
        self.refresh_search();
        if let ChatMode::Search(s) = &self.chat.mode
            && !s.editing
            && s.hits.is_empty()
        {
            let q = s.query.clone();
            self.toast(Level::Info, format!("Nothing in this chat matches \"{q}\"."));
        }
    }

    /// Recompute hits after the query or the chat changed, landing on the newest.
    fn refresh_search(&mut self) {
        if let ChatMode::Search(s) = &self.chat.mode {
            let hits = self.chat.search(&s.query);
            if let ChatMode::Search(s) = &mut self.chat.mode {
                s.current = hits.len().saturating_sub(1);
                s.hits = hits;
            }
        }
    }

    /// Keys while selecting or searching. Returns false if the key wasn't used, so the
    /// normal chat handling (the message box) gets it.
    pub(super) fn transcript_key(&mut self, key: KeyEvent) -> bool {
        match self.chat.mode.clone() {
            ChatMode::Normal => false,
            ChatMode::Select(entry) => {
                self.select_key(entry, key);
                true
            }
            ChatMode::Search(search) => {
                self.search_key(search, key);
                true
            }
        }
    }

    fn select_key(&mut self, entry: usize, key: KeyEvent) {
        let messages = self.chat.message_indices();
        let pos = messages.iter().position(|&i| i == entry).unwrap_or(messages.len().saturating_sub(1));
        let text = self.chat.entries.get(entry).and_then(|e| e.message_text()).unwrap_or_default().to_owned();
        let links: Vec<String> = find_links(&text).into_iter().map(|l| l.url).collect();
        let mut move_to = |to: usize| {
            if let Some(&i) = messages.get(to) {
                self.chat.mode = ChatMode::Select(i);
            }
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => move_to(pos.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => move_to(pos + 1),
            KeyCode::Home | KeyCode::Char('g') => move_to(0),
            KeyCode::End | KeyCode::Char('G') => move_to(messages.len().saturating_sub(1)),
            KeyCode::Enter | KeyCode::Char('q' | 'r') => {
                let quoted = format!("{}{}", quote(&text), self.input.text());
                self.chat.mode = ChatMode::Normal;
                self.chat_focus = ChatFocus::Input;
                self.input.set(&quoted);
                self.input_changed();
            }
            KeyCode::Char('y') => self.copy(&text),
            KeyCode::Char('o') => match links.as_slice() {
                [] => self.toast(Level::Info, "That message has no links."),
                [url] => self.open_link(url),
                _ => self.pick_links(links),
            },
            KeyCode::Char('s') => match links.as_slice() {
                [] => self.toast(Level::Info, "That message has no links. n saves it as a snippet."),
                [url] => {
                    let label = crate::drawer::suggest_label(url);
                    self.open_prompt("Label for the drawer", &label, PromptAction::DrawerAddLabel { url: url.clone() });
                }
                _ => self.pick_links(links),
            },
            KeyCode::Char('n') => {
                self.open_prompt("Save as snippet named", "", PromptAction::SnippetFromMessage { text });
            }
            KeyCode::Esc => self.chat.mode = ChatMode::Normal,
            _ => {}
        }
    }

    fn pick_links(&mut self, links: Vec<String>) {
        let links = links.into_iter().map(|url| chat::ChatLink { url, from_partner: true }).collect();
        self.modal = Some(Modal::Links { links, selected: 0 });
    }

    fn search_key(&mut self, mut search: Search, key: KeyEvent) {
        if search.editing {
            match key.code {
                KeyCode::Esc => return self.chat.mode = ChatMode::Normal,
                KeyCode::Enter => search.editing = false,
                KeyCode::Backspace => {
                    search.query.pop();
                }
                KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                    search.query.push(c);
                }
                KeyCode::Up => search.current = search.current.saturating_sub(1),
                KeyCode::Down => search.current = (search.current + 1).min(search.hits.len().saturating_sub(1)),
                _ => return,
            }
            let query_changed = matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace);
            self.chat.mode = ChatMode::Search(search);
            if query_changed {
                self.refresh_search();
            }
            return;
        }
        let last = search.hits.len().saturating_sub(1);
        match key.code {
            // Searching runs back from the newest message, so "next" is older.
            KeyCode::Char('n') | KeyCode::Up => search.current = search.current.saturating_sub(1),
            KeyCode::Char('N') | KeyCode::Down => search.current = (search.current + 1).min(last),
            KeyCode::Char('/') => search.editing = true,
            KeyCode::Enter => {
                if let Some(entry) = search.current_entry().filter(|&e| self.chat.entries[e].is_message()) {
                    return self.chat.mode = ChatMode::Select(entry);
                }
            }
            KeyCode::Esc => return self.chat.mode = ChatMode::Normal,
            _ => {}
        }
        self.chat.mode = ChatMode::Search(search);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_are_short_and_single_line() {
        assert_eq!(quote("hi there"), "> \"hi there\" ");
        assert_eq!(quote("multi\nline   text"), "> \"multi line text\" ");
        let long = "word ".repeat(40);
        let q = quote(&long);
        assert!(q.ends_with("…\" "), "{q}");
        assert!(q.chars().count() < QUOTE_CHARS + 10);
    }
}
