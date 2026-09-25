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
    DrawerExport,
    DrawerImport,
    LogRename(usize),
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
