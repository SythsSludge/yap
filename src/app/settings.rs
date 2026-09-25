//! Rows of the Settings tab and what editing each one does.

use super::modal::PromptAction;
use super::{App, Level};
use crate::config::Settings;
use crate::keymap::{Action, Keymap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Theme,
    Transparent,
    PopupStyle,
    ChatStyle,
    RpFormatting,
    Emoji,
    Spellcheck,
    Timestamps,
    Sidebar,
    Buddy,
    SplitChats,
    SaveLogs,
    KeepHistory,
    ReopenTabs,
    AiAccess,
    ConfirmActions,
    AutoRequeue,
    RequeueDelay,
    Editor,
    ParagraphBreak,
    SkipEnabled,
    SkipMinShared,
    SkipLanguage,
    SkipMax,
    ServerUrl,
    AutoReconnect,
    SendLanguage,
    Bell,
    TitleFlash,
    Desktop,
    NotifyMessages,
    Sound,
    SoundCommand,
    Keywords,
    Images,
    ImagesAuto,
    HttpsOnly,
    MaxRows,
    MaxCols,
    HideHeartbeat,
    TrafficCapacity,
    ResetKeys,
    Key(Action),
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
            Theme | Transparent | PopupStyle | ChatStyle | RpFormatting | Timestamps | Sidebar | Buddy => "Appearance",
            SplitChats | SaveLogs | KeepHistory | ReopenTabs | ConfirmActions | AutoRequeue | RequeueDelay | Editor
            | ParagraphBreak | Emoji | Spellcheck => "Chats",
            SkipEnabled | SkipMinShared | SkipLanguage | SkipMax => {
                "Auto-skip (limits are set per profile in Preferences)"
            }
            ServerUrl | AutoReconnect | SendLanguage | AiAccess => "Connection",
            Bell | TitleFlash | Desktop | NotifyMessages | Sound | SoundCommand | Keywords => {
                "Notifications (while unfocused)"
            }
            Images | ImagesAuto | HttpsOnly | MaxRows | MaxCols => "Image previews",
            HideHeartbeat | TrafficCapacity => "Traffic viewer",
            ResetKeys | Key(_) => "Keys",
            ExportAll | ImportProfiles | ImportAll => "Backup",
            AddDomain | Domain(_) => "Trusted image domains",
        }
    }

    pub fn label(self, s: &Settings) -> String {
        use Row::*;
        match self {
            Theme => "Theme".into(),
            Transparent => "Transparent background".into(),
            PopupStyle => "Popup style".into(),
            ChatStyle => "Chat layout".into(),
            RpFormatting => "Roleplay formatting".into(),
            Timestamps => "Timestamps".into(),
            Sidebar => "Partner sidebar".into(),
            Buddy => "Buddy".into(),
            SplitChats => "Fresh chat view for each partner".into(),
            SaveLogs => "Save chat logs to disk".into(),
            KeepHistory => "Keep partner history".into(),
            ReopenTabs => "Reopen chat tabs at startup".into(),
            ConfirmActions => "Confirm leave / block / re-roll".into(),
            AutoRequeue => "Search again when a partner leaves".into(),
            RequeueDelay => "Seconds before searching again".into(),
            Editor => "Editor command".into(),
            ParagraphBreak => "Paragraph separator for editor posts".into(),
            Emoji => "Emoji shortcodes".into(),
            Spellcheck => "Spellcheck".into(),
            SkipEnabled => "Skip partners that break my rules".into(),
            SkipMinShared => "Minimum shared kinks".into(),
            SkipLanguage => "Skip a different language".into(),
            SkipMax => "Stop after this many skips in a row".into(),
            ServerUrl => "Server".into(),
            AutoReconnect => "Reconnect automatically".into(),
            SendLanguage => "Send language preference".into(),
            AiAccess => "AI tools (yap mcp)".into(),
            Bell => "Terminal bell".into(),
            TitleFlash => "Flash window title".into(),
            Desktop => "Desktop notification (OSC 99/777)".into(),
            NotifyMessages => "Also notify on every message".into(),
            Sound => "Play a sound".into(),
            SoundCommand => "Sound command".into(),
            Keywords => "Keywords that always notify".into(),
            Images => "Enabled".into(),
            ImagesAuto => "Load trusted previews automatically".into(),
            HttpsOnly => "HTTPS only".into(),
            MaxRows => "Inline height (rows)".into(),
            MaxCols => "Inline width (columns)".into(),
            HideHeartbeat => "Hide heartbeat frames".into(),
            TrafficCapacity => "Frames kept".into(),
            ResetKeys => "Reset all keys to defaults".into(),
            Key(a) => a.describe().into(),
            ExportAll => "Export profiles & settings…".into(),
            ImportProfiles => "Import profiles…".into(),
            ImportAll => "Import profiles & settings…".into(),
            AddDomain => "Add domain…".into(),
            Domain(i) => s.images.trusted_domains.get(i).cloned().unwrap_or_default(),
        }
    }

    /// Keys are shown from the live keymap rather than the saved overrides.
    pub fn key_value(self, keymap: &Keymap) -> Option<String> {
        match self {
            Row::Key(a) => Some(match keymap.keys(a) {
                [] => "unbound".into(),
                keys => keys.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "),
            }),
            _ => None,
        }
    }

    pub fn value(self, s: &Settings) -> String {
        use Row::*;
        let flag = |b: bool| if b { "on" } else { "off" }.to_owned();
        match self {
            Theme => format!("‹ {} ›", s.theme),
            Transparent => flag(s.transparent_background),
            PopupStyle => format!("‹ {} ›", s.popup_style.name()),
            ChatStyle => format!("‹ {} ›", s.chat_style.name()),
            RpFormatting => flag(s.rp_formatting),
            Emoji => flag(s.emoji_shortcodes),
            Spellcheck => format!("{} · {}", flag(s.spellcheck), s.spell_language),
            Timestamps => flag(s.timestamps),
            Sidebar => flag(s.show_sidebar),
            Buddy => format!("‹ {} ›", if s.buddy.is_empty() { "off" } else { s.buddy.as_str() }),
            SplitChats => flag(s.split_chats),
            SaveLogs => flag(s.save_logs),
            KeepHistory => flag(s.keep_history),
            ReopenTabs => flag(s.reopen_tabs),
            AiAccess => format!("‹ {} ›", s.ai_access.name()),
            ConfirmActions => flag(s.confirm_actions),
            AutoRequeue => flag(s.auto_requeue),
            RequeueDelay => format!("‹ {} ›", s.requeue_delay_secs),
            Editor if s.editor.trim().is_empty() => "auto ($VISUAL / $EDITOR)".into(),
            Editor => s.editor.clone(),
            ParagraphBreak => format!("\"{}\"", s.paragraph_break),
            SkipEnabled => flag(s.skip.enabled),
            SkipMinShared if s.skip.min_shared_kinks == 0 => "‹ off ›".into(),
            SkipMinShared => format!("‹ {} ›", s.skip.min_shared_kinks),
            SkipLanguage => flag(s.skip.language_mismatch),
            SkipMax => format!("‹ {} ›", s.skip.max_in_a_row),
            ServerUrl => s.server_url.clone(),
            AutoReconnect => flag(s.auto_reconnect),
            SendLanguage => flag(s.send_language),
            Bell => flag(s.notify.bell),
            TitleFlash => flag(s.notify.title),
            Desktop => flag(s.notify.desktop),
            NotifyMessages => flag(s.notify.on_message),
            Sound => flag(s.notify.sound),
            SoundCommand if s.notify.sound_command.trim().is_empty() => "auto".into(),
            SoundCommand => s.notify.sound_command.clone(),
            Keywords if s.notify.keywords.is_empty() => "none".into(),
            Keywords => s.notify.keywords.join(", "),
            Images => flag(s.images.enabled),
            ImagesAuto => flag(s.images.auto_load),
            HttpsOnly => flag(s.images.https_only),
            MaxRows => format!("‹ {} ›", s.images.max_rows),
            MaxCols => format!("‹ {} ›", s.images.max_cols),
            HideHeartbeat => flag(s.traffic.hide_heartbeat),
            TrafficCapacity => format!("‹ {} ›", s.traffic.capacity),
            ExportAll | ImportProfiles | ImportAll | AddDomain | ResetKeys | Key(_) => String::new(),
            Domain(_) => "d to remove".into(),
        }
    }

    pub fn help(self) -> &'static str {
        use Row::*;
        match self {
            Transparent => "Leave the background unpainted so a transparent terminal shows through.",
            PopupStyle => {
                "outline: a border with the fill inside it · solid: a filled card, no line · clear: see-through."
            }
            RpFormatting => {
                "Show *actions* in italics and ((out of character)) asides dimmed. Messages are sent as typed."
            }
            ChatStyle => {
                "cozy: name above each message · compact: one line each · messages: bubbles, yours on the right."
            }
            SplitChats => {
                "Clear the chat view when you're matched with someone new. Earlier chats stay in the Logs tab."
            }
            SaveLogs => {
                "Write every chat to ~/.local/share/yap/logs (private files). Includes this session's chats so far."
            }
            Spellcheck => {
                "Underline misspelled words as you type; alt+s offers fixes. Other languages: set spell_language in config.toml and install its hunspell dictionary."
            }
            Buddy => {
                "A little companion by the message box that reacts to your chats. Make your own in ~/.config/yap/buddies (see the README)."
            }
            Emoji => "Type :smile: and it's sent as the emoji. While typing :smi… Tab completes the first suggestion.",
            AiAccess => {
                "Lets an AI app use yap through `yap mcp` (see the README). read: chats, logs, history. full: also drafts, finds, skips, profiles; it asks you before sending anything. What it reads goes to that AI's provider, partner messages included."
            }
            ReopenTabs => "Open the same chat tabs, each with its profile, next time. Partners start fresh.",
            KeepHistory => {
                "Remember who you met and how each chat went (no messages) for /history. Stays on this machine."
            }
            AutoRequeue => "After a partner leaves or drops, search again automatically. Esc in the chat cancels.",
            Editor => "Used by ^X or /edit. Anything your shell can run, e.g. `nvim` or `code --wait`.",
            ParagraphBreak => "The site only takes one line per message, so blank lines in the editor become this.",
            SkipEnabled => "Skipping happens after the server matches you; the partner just sees you leave.",
            SkipMinShared => "Partners (or profiles) set to Any / All always pass this rule.",
            SkipLanguage => "Only applies when the server tells you their language.",
            SkipMax => "So strict rules can't keep skipping forever.",
            Sound => "Uses the sound command, or pw-play/paplay with a desktop sound if left on auto.",
            Keywords => "Comma separated, e.g. your character's name. Whole words, any case.",
            SendLanguage => "The live site predates the language field. Turn off if it starts rejecting preferences.",
            ServerUrl => "wss:// address of a YiffSpot server. Changing it reconnects.",
            ImagesAuto => "Loading an image tells its host your IP address; only trusted domains are ever auto-loaded.",
            HttpsOnly => "Plain http images could be tampered with in transit.",
            Desktop => "kitty shows OSC 99 notifications; many other terminals understand OSC 777.",
            MaxRows | MaxCols => "Applies to newly loaded previews.",
            Domain(_) | AddDomain => "Subdomains are included: e621.net also trusts static1.e621.net.",
            Key(_) => {
                "enter: press a new key · backspace: back to default · x: unbind. Saved as [settings.keys] in the config."
            }
            ResetKeys => "Put every key back to its default binding.",
            _ => "",
        }
    }
}

pub fn rows(s: &Settings) -> Vec<Row> {
    use Row::*;
    let mut rows = vec![
        Theme,
        Transparent,
        PopupStyle,
        ChatStyle,
        RpFormatting,
        Timestamps,
        Sidebar,
        Buddy,
        SplitChats,
        SaveLogs,
        KeepHistory,
        ReopenTabs,
        ConfirmActions,
        AutoRequeue,
        RequeueDelay,
        Editor,
        ParagraphBreak,
        Emoji,
        Spellcheck,
        SkipEnabled,
        SkipMinShared,
        SkipLanguage,
        SkipMax,
        ServerUrl,
        AutoReconnect,
        SendLanguage,
        AiAccess,
        Bell,
        TitleFlash,
        Desktop,
        NotifyMessages,
        Sound,
        SoundCommand,
        Keywords,
        Images,
        ImagesAuto,
        HttpsOnly,
        MaxRows,
        MaxCols,
        HideHeartbeat,
        TrafficCapacity,
        ResetKeys,
    ];
    rows.extend(Action::ALL.iter().map(|&a| Key(a)));
    rows.extend([ExportAll, ImportProfiles, ImportAll, AddDomain]);
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
            Transparent => {
                s.transparent_background ^= true;
                self.refresh_theme();
            }
            ChatStyle | PopupStyle | AiAccess | MaxRows | MaxCols | TrafficCapacity | Buddy => {
                return self.adjust_setting(row, 1);
            }
            Timestamps => s.timestamps ^= true,
            RpFormatting => s.rp_formatting ^= true,
            Emoji => s.emoji_shortcodes ^= true,
            Spellcheck => {
                s.spellcheck ^= true;
                self.load_speller();
            }
            Sidebar => s.show_sidebar ^= true,
            SplitChats => s.split_chats ^= true,
            KeepHistory => s.keep_history ^= true,
            ReopenTabs => s.reopen_tabs ^= true,
            SaveLogs => {
                s.save_logs ^= true;
                if s.save_logs {
                    let dir = self.paths.logs_dir.display().to_string();
                    self.toast(Level::Info, format!("Chats will be saved to {dir}"));
                }
            }
            ConfirmActions => s.confirm_actions ^= true,
            AutoRequeue => s.auto_requeue ^= true,
            SkipEnabled => s.skip.enabled ^= true,
            SkipLanguage => s.skip.language_mismatch ^= true,
            Sound => s.notify.sound ^= true,
            RequeueDelay | SkipMinShared | SkipMax => return self.adjust_setting(row, 1),
            Editor => {
                let v = s.editor.clone();
                return self.open_prompt("Editor command (empty: $VISUAL / $EDITOR)", &v, PromptAction::EditorCommand);
            }
            ParagraphBreak => {
                let v = s.paragraph_break.clone();
                return self.open_prompt("Paragraph separator (spaces count)", &v, PromptAction::ParagraphBreak);
            }
            SoundCommand => {
                let v = s.notify.sound_command.clone();
                return self.open_prompt("Sound command (empty: automatic)", &v, PromptAction::SoundCommand);
            }
            Keywords => {
                let v = s.notify.keywords.join(", ");
                return self.open_prompt("Keywords (comma separated)", &v, PromptAction::Keywords);
            }
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
            Key(action) => return self.modal = Some(super::modal::Modal::CaptureKey { action }),
            ResetKeys => {
                self.keymap = Keymap::default();
                self.keymap_changed();
                return self.toast(Level::Info, "All keys are back to their defaults.");
            }
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
            ChatStyle => s.chat_style = if delta < 0 { s.chat_style.prev() } else { s.chat_style.next() },
            PopupStyle => s.popup_style = s.popup_style.step(delta),
            AiAccess => s.ai_access = s.ai_access.step(delta),
            Buddy => {
                let mut names: Vec<String> = self.buddies.iter().map(|b| b.name.clone()).collect();
                names.insert(0, String::new());
                let i = names.iter().position(|n| *n == s.buddy).unwrap_or(0) as i32;
                s.buddy = names[(i + delta).rem_euclid(names.len() as i32) as usize].clone();
            }
            MaxRows => s.images.max_rows = (s.images.max_rows as i32 + delta).clamp(2, 60) as u16,
            RequeueDelay => s.requeue_delay_secs = (s.requeue_delay_secs as i64 + delta as i64).clamp(0, 120) as u64,
            SkipMinShared => {
                s.skip.min_shared_kinks = (s.skip.min_shared_kinks as i32 + delta).clamp(0, 20) as u8;
            }
            SkipMax => s.skip.max_in_a_row = (s.skip.max_in_a_row as i64 + 5 * delta as i64).clamp(1, 500) as u32,
            MaxCols => s.images.max_cols = (s.images.max_cols as i32 + 4 * delta).clamp(8, 200) as u16,
            TrafficCapacity => {
                s.traffic.capacity = (s.traffic.capacity as i64 + 1000 * delta as i64).clamp(500, 100_000) as usize;
                let cap = s.traffic.capacity;
                self.traffic.set_capacity(cap);
            }
            Key(_) | ResetKeys | Domain(_) => return,
            _ => return self.activate_setting(row),
        }
        self.config_changed();
    }

    /// Persist the keymap after an edit (only differences from the defaults are saved).
    pub fn keymap_changed(&mut self) {
        self.config.settings.keys = self.keymap.overrides();
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
    fn chat_style_cycles_both_ways() {
        let s = Settings { chat_style: ChatStyle::Sms, ..Settings::default() };
        assert_eq!(Row::ChatStyle.value(&s), "‹ messages ›");
        assert_eq!(ChatStyle::Cozy.next().next().next(), ChatStyle::Cozy);
        assert_eq!(ChatStyle::Cozy.prev(), ChatStyle::Sms);
    }
}
