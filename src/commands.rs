//! Slash commands typed into the chat input. `//text` sends a literal `/text`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Find,
    Leave,
    Block,
    Reconnect,
    Clear,
    Help,
    Quit,
    Links,
    Drawer,
    /// Open the chat logs tab.
    Logs,
    /// Switch profile, or open the picker when no name is given.
    Profile(Option<String>),
    /// Switch theme, or open the picker when no name is given.
    Theme(Option<String>),
    Save {
        url: String,
        label: String,
    },
    /// Export every profile plus settings.
    ExportAll(String),
    /// Export only the active profile.
    ExportProfile(String),
    Import {
        path: String,
        with_settings: bool,
    },
    Trust(String),
    Untrust(String),
    /// Send a raw text frame (debugging).
    Raw(String),
    /// Save the chat transcript to a file.
    SaveLog(String),
}

/// What the input line turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Message(String),
    Command(Command),
    Error(String),
}

pub const HELP: &[(&str, &str)] = &[
    ("/find", "find a (new) partner with the active profile"),
    ("/leave", "disconnect from your partner"),
    ("/block", "block and leave your partner"),
    ("/save <url> [label] [#tags]", "add a link to the drawer"),
    ("/profile [name]", "switch preference profile"),
    ("/theme [name]", "switch theme"),
    ("/export <path>", "export all profiles and settings (.toml/.json)"),
    ("/export-profile <path>", "export the active profile"),
    ("/import <path>", "import profiles (also yiffspot localStorage JSON)"),
    ("/import-all <path>", "import profiles and settings"),
    ("/trust <domain>", "allow image previews from a domain"),
    ("/untrust <domain>", "stop image previews from a domain"),
    ("/log <path>", "save this chat's transcript"),
    ("/raw <json>", "send a raw websocket frame"),
    ("/links", "pick a link from the chat"),
    ("/logs", "browse earlier chats"),
    ("/drawer", "toggle the drawer"),
    ("/clear", "clear the chat view"),
    ("/reconnect", "reconnect to the server"),
    ("/quit", "exit"),
    ("//text", "send a message starting with /"),
];

pub fn parse(input: &str) -> Parsed {
    let Some(rest) = input.strip_prefix('/') else {
        return Parsed::Message(input.to_owned());
    };
    if rest.starts_with('/') {
        return Parsed::Message(rest.to_owned());
    }
    let (name, arg) = match rest.split_once(char::is_whitespace) {
        Some((n, a)) => (n, a.trim()),
        None => (rest, ""),
    };
    let opt = || (!arg.is_empty()).then(|| arg.to_owned());
    let need = |what: &str, make: fn(String) -> Command| match opt() {
        Some(a) => Parsed::Command(make(a)),
        None => Parsed::Error(format!("/{name} needs {what}")),
    };
    match name.to_ascii_lowercase().as_str() {
        "find" | "next" | "new" => Parsed::Command(Command::Find),
        "leave" | "disconnect" | "dc" => Parsed::Command(Command::Leave),
        "block" => Parsed::Command(Command::Block),
        "reconnect" => Parsed::Command(Command::Reconnect),
        "clear" => Parsed::Command(Command::Clear),
        "help" | "?" => Parsed::Command(Command::Help),
        "quit" | "exit" | "q" => Parsed::Command(Command::Quit),
        "links" => Parsed::Command(Command::Links),
        "drawer" => Parsed::Command(Command::Drawer),
        "logs" | "history" => Parsed::Command(Command::Logs),
        "profile" | "p" => Parsed::Command(Command::Profile(opt())),
        "theme" => Parsed::Command(Command::Theme(opt())),
        "save" => match arg.split_once(char::is_whitespace) {
            Some((url, label)) => Parsed::Command(Command::Save { url: url.into(), label: label.trim().into() }),
            None if !arg.is_empty() => Parsed::Command(Command::Save { url: arg.into(), label: String::new() }),
            None => Parsed::Error("/save needs a link".into()),
        },
        "export" => need("a file path", Command::ExportAll),
        "export-profile" => need("a file path", Command::ExportProfile),
        "import" => need("a file path", |path| Command::Import { path, with_settings: false }),
        "import-all" => need("a file path", |path| Command::Import { path, with_settings: true }),
        "trust" => need("a domain", Command::Trust),
        "untrust" => need("a domain", Command::Untrust),
        "raw" => need("a frame to send", Command::Raw),
        "log" => need("a file path", Command::SaveLog),
        "" => Parsed::Error("type a command after /, or // to send a literal /".into()),
        other => Parsed::Error(format!("unknown command /{other} — try /help")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> Command {
        match parse(s) {
            Parsed::Command(c) => c,
            other => panic!("{s} parsed as {other:?}"),
        }
    }

    #[test]
    fn plain_text_is_a_message() {
        assert_eq!(parse("hello /find"), Parsed::Message("hello /find".into()));
        assert_eq!(parse("//shrug"), Parsed::Message("/shrug".into()));
    }

    #[test]
    fn parses_simple_commands_and_aliases() {
        assert_eq!(cmd("/find"), Command::Find);
        assert_eq!(cmd("/NEXT"), Command::Find);
        assert_eq!(cmd("/dc"), Command::Leave);
        assert_eq!(cmd("/profile"), Command::Profile(None));
        assert_eq!(cmd("/profile  my fox "), Command::Profile(Some("my fox".into())));
    }

    #[test]
    fn save_splits_url_from_label() {
        assert_eq!(
            cmd("/save https://e621.net/posts/1 my ref sheet"),
            Command::Save { url: "https://e621.net/posts/1".into(), label: "my ref sheet".into() }
        );
        assert_eq!(cmd("/save https://x.y/"), Command::Save { url: "https://x.y/".into(), label: String::new() });
        assert!(matches!(parse("/save"), Parsed::Error(_)));
    }

    #[test]
    fn paths_keep_spaces() {
        assert_eq!(cmd("/export ~/My Stuff/yap.toml"), Command::ExportAll("~/My Stuff/yap.toml".into()));
        assert_eq!(cmd("/import-all a.json"), Command::Import { path: "a.json".into(), with_settings: true });
    }

    #[test]
    fn reports_errors() {
        assert_eq!(parse("/export"), Parsed::Error("/export needs a file path".into()));
        assert_eq!(parse("/wat"), Parsed::Error("unknown command /wat — try /help".into()));
        assert!(matches!(parse("/"), Parsed::Error(_)));
    }

    #[test]
    fn help_lists_every_command_once() {
        let mut names: Vec<_> = HELP.iter().map(|(c, _)| c.split(' ').next().unwrap()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
    }
}
