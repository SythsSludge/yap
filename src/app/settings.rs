//! Rows of the Settings tab and what editing each one does.

use super::modal::PromptAction;
use super::{App, Level};
use crate::config::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Theme,
    ChatStyle,
    Timestamps,
    Sidebar,
    ConfirmActions,
    ServerUrl,
    AutoReconnect,
    SendLanguage,
    Bell,
    TitleFlash,
    Desktop,
    NotifyMessages,
    Images,
    ImagesAuto,
    HttpsOnly,
    MaxRows,
    MaxCols,
    HideHeartbeat,
    TrafficCapacity,
    ExportAll,
    ImportProfiles,
    ImportAll,
    AddDomain,
    Domain(usize),
}

impl Row {
    pub fn section(self) -> &'static str {
        use Row::*;
        match self {
            Theme | ChatStyle | Timestamps | Sidebar | ConfirmActions => "Appearance & behaviour",
            ServerUrl | AutoReconnect | SendLanguage => "Connection",
            Bell | TitleFlash | Desktop | NotifyMessages => "Notifications (while unfocused)",
            Images | ImagesAuto | HttpsOnly | MaxRows | MaxCols => "Image previews",
            HideHeartbeat | TrafficCapacity => "Traffic viewer",
            ExportAll | ImportProfiles | ImportAll => "Backup",
            AddDomain | Domain(_) => "Trusted image domains",
        }
    }

    pub fn label(self, s: &Settings) -> String {
        use Row::*;
        match self {
            Theme => "Theme".into(),
            ChatStyle => "Chat layout".into(),
            Timestamps => "Timestamps".into(),
            Sidebar => "Partner sidebar".into(),
            ConfirmActions => "Confirm leave / block / re-roll".into(),
            ServerUrl => "Server".into(),
            AutoReconnect => "Reconnect automatically".into(),
            SendLanguage => "Send language preference".into(),
            Bell => "Terminal bell".into(),
            TitleFlash => "Flash window title".into(),
            Desktop => "Desktop notification (OSC 99/777)".into(),
            NotifyMessages => "Also notify on every message".into(),
            Images => "Enabled".into(),
            ImagesAuto => "Load trusted previews automatically".into(),
            HttpsOnly => "HTTPS only".into(),
            MaxRows => "Inline height (rows)".into(),
            MaxCols => "Inline width (columns)".into(),
            HideHeartbeat => "Hide heartbeat frames".into(),
            TrafficCapacity => "Frames kept".into(),
            ExportAll => "Export profiles & settings…".into(),
            ImportProfiles => "Import profiles…".into(),
            ImportAll => "Import profiles & settings…".into(),
            AddDomain => "+ Add domain…".into(),
            Domain(i) => s.images.trusted_domains.get(i).cloned().unwrap_or_default(),
        }
    }

    pub fn value(self, s: &Settings) -> String {
        use Row::*;
        let flag = |b: bool| if b { "on" } else { "off" }.to_owned();
        match self {
            Theme => format!("‹ {} ›", s.theme),
            ChatStyle => match s.chat_style {
                crate::config::ChatStyle::Cozy => "‹ cozy ›".into(),
                crate::config::ChatStyle::Compact => "‹ compact ›".into(),
            },
            Timestamps => flag(s.timestamps),
            Sidebar => flag(s.show_sidebar),
            ConfirmActions => flag(s.confirm_actions),
            ServerUrl => s.server_url.clone(),
            AutoReconnect => flag(s.auto_reconnect),
            SendLanguage => flag(s.send_language),
            Bell => flag(s.notify.bell),
            TitleFlash => flag(s.notify.title),
            Desktop => flag(s.notify.desktop),
            NotifyMessages => flag(s.notify.on_message),
            Images => flag(s.images.enabled),
            ImagesAuto => flag(s.images.auto_load),
            HttpsOnly => flag(s.images.https_only),
            MaxRows => format!("‹ {} ›", s.images.max_rows),
            MaxCols => format!("‹ {} ›", s.images.max_cols),
            HideHeartbeat => flag(s.traffic.hide_heartbeat),
            TrafficCapacity => format!("‹ {} ›", s.traffic.capacity),
            ExportAll | ImportProfiles | ImportAll | AddDomain => String::new(),
            Domain(_) => "d to remove".into(),
        }
    }

    pub fn help(self) -> &'static str {
        use Row::*;
        match self {
            SendLanguage => "The live site predates the language field. Turn off if it starts rejecting preferences.",
            ServerUrl => "wss:// address of a YiffSpot server. Changing it reconnects.",
            ImagesAuto => "Loading an image tells its host your IP address; only trusted domains are ever auto-loaded.",
            HttpsOnly => "Plain http images could be tampered with in transit.",
            Desktop => "kitty shows OSC 99 notifications; many other terminals understand OSC 777.",
            MaxRows | MaxCols => "Applies to newly loaded previews.",
            Domain(_) | AddDomain => "Subdomains are included: e621.net also trusts static1.e621.net.",
            _ => "",
        }
    }
}

pub fn rows(s: &Settings) -> Vec<Row> {
    use Row::*;
    let mut rows = vec![
        Theme,
        ChatStyle,
        Timestamps,
        Sidebar,
        ConfirmActions,
        ServerUrl,
        AutoReconnect,
        SendLanguage,
        Bell,
        TitleFlash,
        Desktop,
        NotifyMessages,
        Images,
        ImagesAuto,
        HttpsOnly,
        MaxRows,
        MaxCols,
        HideHeartbeat,
        TrafficCapacity,
        ExportAll,
        ImportProfiles,
        ImportAll,
        AddDomain,
    ];
    rows.extend((0..s.images.trusted_domains.len()).map(Domain));
    rows
}

impl App {
    /// Enter/Space on a settings row.
    pub fn activate_setting(&mut self, row: Row) {
        use Row::*;
        let s = &mut self.config.settings;
        match row {
            Theme => return self.open_theme_picker(),
            ChatStyle => return self.adjust_setting(row, 1),
            Timestamps => s.timestamps ^= true,
            Sidebar => s.show_sidebar ^= true,
            ConfirmActions => s.confirm_actions ^= true,
            AutoReconnect => s.auto_reconnect ^= true,
            SendLanguage => s.send_language ^= true,
            Bell => s.notify.bell ^= true,
            TitleFlash => s.notify.title ^= true,
            Desktop => s.notify.desktop ^= true,
            NotifyMessages => s.notify.on_message ^= true,
            Images => s.images.enabled ^= true,
            ImagesAuto => s.images.auto_load ^= true,
            HttpsOnly => s.images.https_only ^= true,
            HideHeartbeat => s.traffic.hide_heartbeat ^= true,
            MaxRows | MaxCols | TrafficCapacity => return self.adjust_setting(row, 1),
            ServerUrl => {
                let url = s.server_url.clone();
                return self.open_prompt("Server URL", &url, PromptAction::ServerUrl);
            }
            ExportAll => {
                let path = self.default_path("yap-export.toml");
                return self.open_prompt(
                    "Export profiles & settings to (.toml or .json)",
                    &path,
                    PromptAction::ExportAll,
                );
            }
            ImportProfiles => {
                return self.open_prompt("Import profiles from", "", PromptAction::Import { with_settings: false });
            }
            ImportAll => {
                return self.open_prompt(
                    "Import profiles & settings from",
                    "",
                    PromptAction::Import { with_settings: true },
                );
            }
            AddDomain => return self.open_prompt("Trust image domain", "", PromptAction::AddTrustedDomain),
            Domain(_) => return,
        }
        self.config_changed();
    }

    /// Left/Right on a settings row.
    pub fn adjust_setting(&mut self, row: Row, delta: i32) {
        use Row::*;
        let s = &mut self.config.settings;
        match row {
            Theme => {
                let i = self.themes.iter().position(|t| t.name == self.theme.name).unwrap_or(0) as i32;
                let n = self.themes.len() as i32;
                let name = self.themes[(i + delta).rem_euclid(n) as usize].name.clone();
                return self.set_theme(&name, true);
            }
            ChatStyle => {
                s.chat_style = match s.chat_style {
                    crate::config::ChatStyle::Cozy => crate::config::ChatStyle::Compact,
                    crate::config::ChatStyle::Compact => crate::config::ChatStyle::Cozy,
                };
            }
            MaxRows => s.images.max_rows = (s.images.max_rows as i32 + delta).clamp(2, 60) as u16,
            MaxCols => s.images.max_cols = (s.images.max_cols as i32 + 4 * delta).clamp(8, 200) as u16,
            TrafficCapacity => {
                s.traffic.capacity = (s.traffic.capacity as i64 + 1000 * delta as i64).clamp(500, 100_000) as usize;
                let cap = s.traffic.capacity;
                self.traffic.set_capacity(cap);
            }
            _ => return self.activate_setting(row),
        }
        self.config_changed();
    }

    pub fn remove_domain(&mut self, index: usize) {
        let list = &mut self.config.settings.images.trusted_domains;
        if index < list.len() {
            let d = list.remove(index);
            self.config_changed();
            self.toast(Level::Info, format!("{d} is no longer trusted."));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ChatStyle;

    #[test]
    fn every_row_has_a_label_and_section() {
        let s = Settings::default();
        for row in rows(&s) {
            assert!(!row.label(&s).is_empty(), "{row:?}");
            assert!(!row.section().is_empty());
        }
        assert_eq!(rows(&s).iter().filter(|r| matches!(r, Row::Domain(_))).count(), s.images.trusted_domains.len());
    }

    #[test]
    fn compact_style_value() {
        let s = Settings { chat_style: ChatStyle::Compact, ..Settings::default() };
        assert_eq!(Row::ChatStyle.value(&s), "‹ compact ›");
    }
}
