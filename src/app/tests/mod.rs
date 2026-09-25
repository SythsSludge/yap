use super::modal::{Confirm, Modal};
use super::*;
use super::{EditorPurpose, Hit, ListId, PaletteItem, Shelf, ViewerButton};
use crate::config::Paths;
use crate::prefs::Preferences;
use crate::protocol::WirePreferences;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

pub(crate) struct Harness {
    pub app: App,
    pub _dir: tempfile::TempDir,
}

impl std::ops::Deref for Harness {
    type Target = App;
    fn deref(&self) -> &App {
        &self.app
    }
}

impl std::ops::DerefMut for Harness {
    fn deref_mut(&mut self) -> &mut App {
        &mut self.app
    }
}

pub(crate) fn complete_prefs() -> Preferences {
    let mut p = Preferences::default();
    p.toggle(Field::Gender, "Male");
    p.toggle(Field::Species, "Wolf");
    p.toggle(Field::Role, "Switch");
    p.toggle(Field::PartnerRole, "Dominant");
    p.toggle(Field::Kinks, "Biting");
    p.toggle(Field::Kinks, "Musk");
    p
}

pub(crate) fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(
        Paths::in_dir(dir.path()),
        Config::default(),
        Drawer::default(),
        crate::theme::builtins(),
        Picker::halfblocks(),
    );
    Harness { app, _dir: dir }
}

impl Harness {
    pub fn online(mut self) -> Self {
        self.on_net(NetEvent::Open);
        self.on_net(NetEvent::Message(ServerMessage::ConnectionSuccess { token: "tok".into() }));
        self.on_net(NetEvent::Message(ServerMessage::UserCount(42)));
        self.take_effects();
        self
    }

    pub fn with_prefs(mut self) -> Self {
        self.config.active_mut().preferences = complete_prefs();
        self
    }

    pub fn partnered(mut self) -> Self {
        self.server(ServerMessage::PartnerConnected(PartnerInfo {
            gender: "Female".into(),
            species: "Fox".into(),
            kinks: "Musk, Biting, Tickling".into(),
            role: "Dominant".into(),
            language: Some("English".into()),
        }));
        self.take_effects();
        self
    }

    pub fn server(&mut self, msg: ServerMessage) {
        self.on_net(NetEvent::Message(msg));
    }

    pub fn press(&mut self, code: KeyCode) {
        self.on_terminal(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    pub fn ctrl(&mut self, c: char) {
        self.on_terminal(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)));
    }

    pub fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    pub fn sent(&mut self) -> Vec<ClientMessage> {
        self.take_effects()
            .into_iter()
            .filter_map(|e| match e {
                Effect::Send(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    pub fn last_toast(&self) -> Option<&str> {
        self.toasts.last().map(|t| t.text.as_str())
    }

    pub fn last_entry(&self) -> &EntryKind {
        &self.chat.entries.last().expect("chat is empty").kind
    }
}

mod ai;
mod commands;
mod drawer;
mod logs;
mod matching;
mod media;
mod mouse;
mod notify;
mod partner;
mod prefs;
mod sessions;
mod settings_tab;
mod tools;
mod transcript;

fn find_payload(sent: &[ClientMessage]) -> Option<&WirePreferences> {
    sent.iter().find_map(|m| match m {
        ClientMessage::FindPartner(p) => Some(p),
        _ => None,
    })
}
fn select_row(h: &mut Harness, row: settings::Row) {
    h.press(KeyCode::F(7));
    let rows = settings::rows(&h.config.settings);
    h.settings_ui.selected = rows.iter().position(|r| *r == row).unwrap();
}
fn alt_key(h: &mut Harness, code: KeyCode) {
    h.on_terminal(Event::Key(KeyEvent::new(code, KeyModifiers::ALT)));
}

fn alt(h: &mut Harness, c: char) {
    h.on_terminal(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)));
}
fn connected_to(h: &mut Harness, kinks: &str, language: Option<&str>) {
    h.server(ServerMessage::PartnerConnected(PartnerInfo {
        gender: "Male".into(),
        species: "Cat".into(),
        kinks: kinks.into(),
        role: "Dominant".into(),
        language: language.map(Into::into),
    }));
}
// ----- mouse ------------------------------------------------------------------------

fn frame(h: &mut Harness) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 30)).unwrap();
    terminal.draw(|f| crate::ui::draw(f, &mut h.app)).unwrap();
}
fn click_on(h: &mut Harness, target: &Hit) {
    let (rect, _) = h.hits.borrow().iter().find(|(_, hit)| hit == target).cloned().unwrap_or_else(|| {
        panic!(
            "no hitbox for {target:?}; have {:?}",
            h.hits.borrow().iter().map(|(_, h)| h.clone()).collect::<Vec<_>>()
        )
    });
    h.on_terminal(Event::Mouse(ratatui::crossterm::event::MouseEvent {
        kind: ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    }));
    frame(h);
}
