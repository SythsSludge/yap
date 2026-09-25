//! Popups that take over the keyboard until dismissed.

use super::chat::ChatLink;
use crate::input::LineEditor;

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
    /// `original` is restored if the picker is cancelled after live-previewing.
    Themes {
        selected: usize,
        original: String,
    },
}
