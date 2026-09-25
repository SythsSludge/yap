//! Popups that take over the keyboard until dismissed.

use super::chat::ChatLink;
use crate::input::LineEditor;
use crate::keymap::Action;

/// Actions that need a yes/no first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    FindNew,
    Leave,
    Block,
    Quit,
    DeleteProfile(String),
    DeleteDrawerItem(usize),
    DeleteLog(usize),
    DeleteSnippet(usize),
    CloseSession,
    LoadUntrusted(String),
    ClearHistory,
}

/// What to do with the text typed into a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAction {
    NewProfile,
    CloneProfile(String),
    RenameProfile(String),
    ExportProfile(String),
    ExportAll,
    Import { with_settings: bool },
    DrawerAddUrl,
    DrawerAddLabel { url: String },
    DrawerEditLabel(usize),
    DrawerEditNote(usize),
    DrawerEditTags(usize),
    SnippetName,
    SnippetText { name: String },
    SnippetRename(usize),
    SnippetFromMessage { text: String },
    DrawerExport,
    DrawerImport,
    LogRename(usize),
    CharacterName(String),
    EditorCommand,
    ParagraphBreak,
    RequeueDelay,
    MinSharedKinks,
    Keywords,
    SoundCommand,
    ExportLog(usize),
    AddTrustedDomain,
    ServerUrl,
    RawFrame,
    TrafficExport,
}

#[derive(Debug)]
pub struct Prompt {
    pub title: String,
    pub editor: LineEditor,
    pub action: PromptAction,
}

#[derive(Debug)]
pub enum Modal {
    Confirm {
        text: String,
        action: Confirm,
    },
    Prompt(Prompt),
    /// Waiting for the user to press a new key for `action`.
    CaptureKey {
        action: Action,
    },
    Help {
        scroll: u16,
    },
    Links {
        links: Vec<ChatLink>,
        selected: usize,
    },
    Profiles {
        selected: usize,
    },
    /// Fuzzy search over every action, command, setting, profile, theme, snippet and chat.
    Palette {
        query: String,
        selected: usize,
    },
    /// Chat statistics.
    Stats,
    /// A message the AI wants to send; y sends it.
    AiSend(super::AiSend),
    /// Shown on the very first start.
    Welcome,
    /// Browse and search emoji; typing filters.
    Emoji {
        query: String,
        selected: usize,
    },
    /// Fixes for a misspelled word in the message box (bytes `start..end`). The row
    /// after the suggestions adds the word to your dictionary.
    Spelling {
        word: String,
        start: usize,
        end: usize,
        suggestions: Vec<String>,
        selected: usize,
    },
    /// Everyone you've met, with patterns worth knowing.
    History {
        scroll: u16,
    },
    /// The partner's kinks (or yours, with no partner), with definitions.
    Kinks {
        scroll: u16,
    },
    /// Pick a snippet to insert; typing filters.
    Snippets {
        filter: String,
        selected: usize,
    },
    /// `original` is restored if the picker is cancelled after live-previewing.
    Themes {
        selected: usize,
        original: String,
    },
}
