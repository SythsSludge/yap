//! Clicking. The UI records a hitbox for everything clickable while it draws; a click
//! finds the topmost one under the pointer. Double-clicks reuse the keyboard's Enter
//! handling so mouse and keys always agree.

use super::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Which list a clicked row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListId {
    PrefsProfiles,
    PrefsFields,
    PrefsOptions,
    Drawer,
    Snippets,
    ChatDrawer,
    Logs,
    Traffic,
    Settings,
    /// The list inside whatever popup is open.
    Modal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerButton {
    Prev,
    Next,
    /// Load an untrusted image this once.
    LoadOnce,
    /// Trust the image's host, then load it.
    Trust,
    Open,
    CopyLink,
    SaveToDrawer,
    SaveImage,
    Close,
}

/// Something clickable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Tab(Tab),
    Session(u64),
    NewSession,
    /// An inline image preview in the chat.
    Image(String),
    /// A chat message (entry index) and the links in it.
    Message {
        entry: usize,
        links: Vec<String>,
    },
    Row {
        list: ListId,
        index: usize,
    },
    Viewer(ViewerButton),
    Input,
    DrawerTag(Option<String>),
    Shelf(Shelf),
    /// The partner's kinks in the sidebar.
    Kinks,
}

impl App {
    /// Record a clickable area. Called by the renderer.
    pub fn hit(&self, area: Rect, hit: Hit) {
        if area.width > 0 && area.height > 0 {
            self.hits.borrow_mut().push((area, hit));
        }
    }

    pub fn clear_hits(&self) {
        self.hits.borrow_mut().clear();
    }

    /// The topmost hitbox at a screen position.
    pub fn hit_at(&self, column: u16, row: u16) -> Option<Hit> {
        let pos = Position::new(column, row);
        self.hits.borrow().iter().rev().find(|(r, _)| r.contains(pos)).map(|(_, h)| h.clone())
    }

    pub fn on_click(&mut self, column: u16, row: u16) {
        let Some(hit) = self.hit_at(column, row) else { return };
        let double =
            self.last_click.as_ref().is_some_and(|(h, at)| *h == hit && self.now.duration_since(*at) < DOUBLE_CLICK);
        self.last_click = if double { None } else { Some((hit.clone(), self.now)) };
        self.click(hit, double);
    }

    fn enter(&mut self) {
        self.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }

    fn click(&mut self, hit: Hit, double: bool) {
        // With a popup open, only its own list and the viewer's buttons respond.
        if self.viewer.is_some() && !matches!(hit, Hit::Viewer(_)) {
            return;
        }
        if self.modal.is_some() && !matches!(hit, Hit::Row { list: ListId::Modal, .. }) {
            return;
        }
        match hit {
            Hit::Tab(tab) => self.tab = tab,
            Hit::Session(id) => self.switch_session(id),
            Hit::NewSession => self.new_session(),
            Hit::Image(url) => self.preview(&url),
            Hit::Message { entry, links } => match links.as_slice() {
                // Messages without links get selected (in the live chat, not the logs).
                [] if self.tab == Tab::Chat => self.select_message(Some(entry)),
                [] => {}
                [url] => self.open_link(url),
                _ => {
                    let links = links.into_iter().map(|url| chat::ChatLink { url, from_partner: true }).collect();
                    self.modal = Some(Modal::Links { links, selected: 0 });
                }
            },
            Hit::Input => self.chat_focus = ChatFocus::Input,
            Hit::DrawerTag(tag) => {
                self.drawer_ui.tag = tag;
                self.drawer_ui.list.selected = 0;
            }
            Hit::Shelf(shelf) => self.drawer_ui.shelf = shelf,
            Hit::Kinks => self.open_kinks(),
            Hit::Viewer(button) => self.viewer_button(button),
            Hit::Row { list, index } => self.click_row(list, index, double),
        }
    }

    /// Images open in the viewer; everything else in the browser.
    pub fn open_link(&mut self, url: &str) {
        match url::Url::parse(url) {
            Ok(u) if looks_like_image(&u) => self.preview(url),
            _ => self.open_url(url),
        }
    }

    fn click_row(&mut self, list: ListId, index: usize, double: bool) {
        match list {
            ListId::PrefsProfiles => {
                self.prefs_ui.pane = PrefsPane::Profiles;
                self.prefs_ui.profile = index;
            }
            ListId::PrefsFields => {
                self.prefs_ui.pane = PrefsPane::Fields;
                self.prefs_ui.field = index;
            }
            ListId::PrefsOptions => {
                // Options are checkboxes: every click toggles.
                self.prefs_ui.pane = PrefsPane::Options;
                self.prefs_ui.option = index;
                return self.enter();
            }
            ListId::Drawer => self.drawer_ui.list.selected = index,
            ListId::Snippets => self.drawer_ui.snippets.selected = index,
            ListId::ChatDrawer => {
                self.chat_focus = ChatFocus::Drawer;
                self.drawer_ui.list.selected = index;
            }
            ListId::Logs => {
                self.logs_ui.reading = false;
                if self.logs_ui.list.selected != index {
                    self.logs_ui.scroll = 0;
                }
                self.logs_ui.list.selected = index;
            }
            ListId::Traffic => {
                self.traffic_ui.follow = false;
                self.traffic_ui.list.selected = index;
            }
            ListId::Settings => self.settings_ui.selected = index,
            ListId::Modal => match &mut self.modal {
                Some(
                    Modal::Links { selected, .. }
                    | Modal::Profiles { selected }
                    | Modal::Themes { selected, .. }
                    | Modal::Snippets { selected, .. }
                    | Modal::Palette { selected, .. }
                    | Modal::Spelling { selected, .. },
                ) => {
                    *selected = index;
                    // The theme picker previews whatever is selected.
                    if let Some(Modal::Themes { selected, .. }) = &self.modal {
                        let name = self.themes[*selected].name.clone();
                        self.set_theme(&name, false);
                    }
                }
                _ => return,
            },
        }
        if double {
            self.enter();
        }
    }

    fn viewer_button(&mut self, button: ViewerButton) {
        let Some(url) = self.viewer.as_ref().map(|v| v.url.clone()) else { return };
        match button {
            ViewerButton::Open => self.open_url(&url),
            ViewerButton::CopyLink => self.copy(&url),
            ViewerButton::SaveToDrawer => {
                self.viewer = None;
                let label = crate::drawer::suggest_label(&url);
                self.open_prompt("Label", &label, PromptAction::DrawerAddLabel { url });
            }
            ViewerButton::SaveImage => self.save_image(&url),
            ViewerButton::Close => self.viewer = None,
            ViewerButton::Prev => self.viewer_step(-1),
            ViewerButton::Next => self.viewer_step(1),
            ViewerButton::LoadOnce => self.viewer_load_once(),
            ViewerButton::Trust => self.viewer_trust(),
        }
    }

    /// Save the original image file to the Downloads folder.
    pub fn save_image(&mut self, url: &str) {
        let Some(ImageState::Ready(loaded)) = self.images.get(url) else {
            return self.toast(Level::Info, "The image is still loading.");
        };
        match crate::images::save_original(&loaded.bytes, url, &self.paths.downloads_dir) {
            Ok(path) => self.toast(Level::Success, format!("Saved {}", path.display())),
            Err(e) => self.toast(Level::Error, format!("Couldn't save the image: {e}")),
        }
    }
}
