//! The drawer, links in the chat, and image previews.

use super::*;

impl App {
    pub fn add_to_drawer(&mut self, url: &str, label: &str) {
        match self.drawer.add(url, label, chrono::Utc::now()) {
            Ok(i) => {
                let label = self.drawer.items[i].label.clone();
                self.drawer_ui.list.selected = 0;
                self.drawer_ui.tag = None;
                self.drawer_changed();
                self.toast(Level::Success, format!("Saved `{label}` to the drawer."));
            }
            Err(e) => self.toast(Level::Error, e.to_string()),
        }
    }

    pub fn toggle_drawer_panel(&mut self) {
        if self.tab != Tab::Chat {
            self.tab = Tab::Chat;
            self.drawer_panel = true;
        } else {
            self.drawer_panel = !self.drawer_panel;
        }
        self.chat_focus = if self.drawer_panel { ChatFocus::Drawer } else { ChatFocus::Input };
    }

    pub fn open_links(&mut self) {
        let links = self.chat.links();
        if links.is_empty() {
            self.toast(Level::Info, "No links in this chat yet.");
        } else {
            self.modal = Some(Modal::Links { links, selected: 0 });
        }
    }

    /// Put a link into the chat box, e.g. to share a ref sheet from the drawer.
    pub fn insert_into_input(&mut self, text: &str) {
        if !self.input.is_empty() && !self.input.text().ends_with(' ') {
            self.input.insert_char(' ');
        }
        self.input.insert_str(text);
        self.tab = Tab::Chat;
        self.chat_focus = ChatFocus::Input;
        self.input_changed();
    }

    pub fn open_url(&mut self, url: &str) {
        match url::Url::parse(url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => self.effect(Effect::OpenUrl(u.to_string())),
            _ => self.toast(Level::Error, "Only http(s) links can be opened."),
        }
    }

    pub fn copy(&mut self, text: &str) {
        self.effect(Effect::Copy(text.to_owned()));
        self.toast(Level::Info, "Copied to clipboard.");
    }

    pub fn trust_domain(&mut self, input: &str) {
        let Some(domain) = normalize_domain(input) else {
            return self.toast(Level::Error, format!("`{input}` isn't a domain."));
        };
        let list = &mut self.config.settings.images.trusted_domains;
        if list.contains(&domain) {
            return self.toast(Level::Info, format!("{domain} is already trusted."));
        }
        list.push(domain.clone());
        self.config_changed();
        self.toast(Level::Success, format!("Image previews enabled for {domain}."));
    }

    pub fn untrust_domain(&mut self, input: &str) {
        let domain = normalize_domain(input).unwrap_or_else(|| input.trim().to_owned());
        let list = &mut self.config.settings.images.trusted_domains;
        let before = list.len();
        list.retain(|d| *d != domain);
        if list.len() == before {
            self.toast(Level::Info, format!("{domain} wasn't trusted."));
        } else {
            self.config_changed();
            self.toast(Level::Success, format!("{domain} is no longer trusted."));
        }
    }

    /// Trusted, image-looking links in `text` that should get inline previews.
    pub fn preview_urls(&self, text: &str) -> Vec<String> {
        let s = &self.config.settings.images;
        if !s.enabled {
            return Vec::new();
        }
        find_links(text)
            .into_iter()
            .filter_map(|l| url::Url::parse(&l.url).ok().map(|u| (l.url, u)))
            .filter(|(_, u)| looks_like_image(u) && check_trust(u, &s.trusted_domains, s.https_only) == Trust::Trusted)
            .map(|(raw, _)| raw)
            .collect()
    }

    pub(super) fn request_images(&mut self, text: &str) {
        if !self.config.settings.images.auto_load {
            return;
        }
        for url in self.preview_urls(text) {
            if !self.images.contains_key(&url) {
                self.images.insert(url.clone(), ImageState::Loading);
                self.effect(Effect::FetchImage { url, allow_host: None });
            }
        }
    }

    /// Show an image full-screen, fetching it if needed. Untrusted hosts need consent.
    pub fn preview(&mut self, url: &str) {
        let s = &self.config.settings.images;
        if !s.enabled {
            return self.toast(Level::Info, "Image previews are turned off in Settings.");
        }
        let Ok(parsed) = url::Url::parse(url) else {
            return self.toast(Level::Error, "That isn't a valid link.");
        };
        match check_trust(&parsed, &s.trusted_domains, s.https_only) {
            Trust::Trusted => self.open_viewer(url, None),
            Trust::UntrustedHost => {
                let host = parsed.host_str().unwrap_or("?");
                self.modal = Some(Modal::Confirm {
                    text: format!(
                        "{host} isn't a trusted image host. Loading it reveals your IP address to them. Load anyway?"
                    ),
                    action: Confirm::LoadUntrusted(url.to_owned()),
                });
            }
            Trust::InsecureScheme => self.toast(Level::Error, "Refusing to load an image over plain http."),
            Trust::NotHttp => self.toast(Level::Error, "Only http(s) images can be previewed."),
        }
    }

    pub(super) fn open_viewer(&mut self, url: &str, allow_host: Option<String>) {
        let protocol = match self.images.get(url) {
            Some(ImageState::Ready(loaded)) => Some(self.picker.new_resize_protocol(loaded.image.clone())),
            Some(ImageState::Loading) => None,
            Some(ImageState::Failed(_)) | None => {
                self.images.insert(url.to_owned(), ImageState::Loading);
                self.effect(Effect::FetchImage { url: url.to_owned(), allow_host });
                None
            }
        };
        self.viewer = Some(Viewer { url: url.to_owned(), protocol });
    }

    pub fn on_image(&mut self, url: String, result: Result<Loaded, String>) {
        let state = match result {
            Ok(loaded) => {
                let loaded = Arc::new(loaded);
                if let Some(v) = self.viewer.as_mut().filter(|v| v.url == url && v.protocol.is_none()) {
                    v.protocol = Some(self.picker.new_resize_protocol(loaded.image.clone()));
                }
                ImageState::Ready(loaded)
            }
            Err(e) => {
                if self.viewer.as_ref().is_some_and(|v| v.url == url) {
                    self.viewer = None;
                    self.toast(Level::Error, format!("Couldn't load image: {e}"));
                }
                ImageState::Failed(e)
            }
        };
        self.images.insert(url, state);
    }

    /// Record a note in the traffic log (e.g. "you pressed reconnect").
    pub fn traffic_note(&mut self, text: &str) {
        self.traffic.push(Local::now(), Direction::Meta, FrameKind::Info, 0, text.into());
    }

    pub fn open_prompt(&mut self, title: &str, initial: &str, action: PromptAction) {
        self.modal =
            Some(Modal::Prompt(Prompt { title: title.into(), editor: LineEditor::with_text(initial), action }));
    }
}
