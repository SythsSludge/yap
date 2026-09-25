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
    /// Open another chat session.
    NewChat,
    /// Close the current chat session.
    CloseChat,
    /// Switch to chat session N (1-based).
    SwitchChat(usize),
    /// Leave and search again without asking.
    Next,
    /// Insert a snippet, or open the picker with no name.
    Snip(Option<String>),
    SnipAdd {
        name: String,
        text: String,
    },
    /// Write the message in the external editor.
    Edit,
    /// Set (or clear) your character name for the active profile.
    Name(Option<String>),
    /// Set (or clear) a nickname for the current partner.
    Nick(Option<String>),
    /// Search this chat (with no text, open the search bar).
    Search(Option<String>),
    /// Select a message to quote, copy or save.
    Select,
    /// Show chat statistics.
    Stats,
    /// Show the partner's kinks with definitions.
    Kinks,
    DrawerExport(String),
    DrawerImport(String),
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
    ("/next", "skip to a new partner without asking"),
    ("/leave", "disconnect from your partner"),
    ("/block", "block and leave your partner"),
    ("/save <url> [label] [#tags]", "add a link to the drawer"),
    ("/snip [name]", "insert a snippet (no name: pick one)"),
    ("/snip-add <name> <text>", "save a snippet; {species} etc. are filled in"),
    ("/edit", "write the message in your editor"),
    ("/name [character]", "your character's name for this profile (shown instead of \"you\")"),
    ("/nick [name]", "a nickname for your current partner"),
    ("/profile [name]", "switch preference profile"),
    ("/theme [name]", "switch theme"),
    ("/export <path>", "export all profiles and settings (.toml/.json)"),
    ("/export-profile <path>", "export the active profile"),
    ("/import <path>", "import profiles (also yiffspot localStorage JSON)"),
    ("/import-all <path>", "import profiles and settings"),
    ("/drawer-export <path>", "export links and snippets"),
    ("/drawer-import <path>", "import links and snippets"),
    ("/trust <domain>", "allow image previews from a domain"),
    ("/untrust <domain>", "stop image previews from a domain"),
    ("/log <path>", "save this chat (.txt, .md or .html)"),
    ("/raw <json>", "send a raw websocket frame"),
    ("/links", "pick a link from the chat"),
    ("/search [text]", "search this chat"),
    ("/select", "select a message to quote, copy or save"),
    ("/stats", "partners met, time chatting and more"),
    ("/kinks", "your partner's kinks next to yours, with what each means"),
    ("/logs", "browse earlier chats"),
    ("/drawer", "toggle the drawer"),
    ("/new", "open another chat alongside this one"),
    ("/close", "close this chat"),
    ("/chat <n>", "switch to chat n"),
    ("/clear", "clear the chat view"),
    ("/reconnect", "reconnect to the server"),
    ("/quit", "exit"),
    ("//text", "send a message starting with /"),
];

/// Commands whose name starts with what's been typed so far (`/lo` finds `/log` and
/// `/logs`), in help order. Only while typing the name itself.
pub fn completions(typed: &str) -> Vec<(&'static str, &'static str)> {
    if !typed.starts_with('/') || typed.starts_with("//") || typed.contains(char::is_whitespace) {
        return Vec::new();
    }
    let typed = typed.to_ascii_lowercase();
    HELP.iter()
        .filter(|(syntax, _)| !syntax.starts_with("//") && name_of(syntax).starts_with(&typed))
        .copied()
        .collect()
}

/// The command from its help syntax: `/log <path>` is `/log`.
pub fn name_of(syntax: &str) -> &str {
    syntax.split_once(' ').map_or(syntax, |(name, _)| name)
}

/// What Tab turns the typed text into: the first completion, with a space after it
/// if the command takes something.
pub fn complete(typed: &str) -> Option<String> {
    let (syntax, _) = *completions(typed).first()?;
    let name = name_of(syntax);
    Some(if name.len() < syntax.len() { format!("{name} ") } else { name.to_owned() })
}

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
        "find" => Parsed::Command(Command::Find),
        "next" | "skip" => Parsed::Command(Command::Next),
        "snip" | "snippet" => Parsed::Command(Command::Snip(opt())),
        "snip-add" | "snippet-add" => match arg.split_once(char::is_whitespace) {
            Some((name, text)) if !text.trim().is_empty() => {
                Parsed::Command(Command::SnipAdd { name: name.into(), text: text.trim().into() })
            }
            _ => Parsed::Error("/snip-add needs a name and some text".into()),
        },
        "edit" | "editor" => Parsed::Command(Command::Edit),
        "name" | "iam" => Parsed::Command(Command::Name(opt())),
        "nick" => Parsed::Command(Command::Nick(opt())),
        "search" | "grep" => Parsed::Command(Command::Search(opt())),
        "select" | "quote" => Parsed::Command(Command::Select),
        "stats" => Parsed::Command(Command::Stats),
        "kinks" | "define" => Parsed::Command(Command::Kinks),
        "drawer-export" => need("a file path", Command::DrawerExport),
        "drawer-import" => need("a file path", Command::DrawerImport),
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
        "new" | "newchat" => Parsed::Command(Command::NewChat),
        "close" => Parsed::Command(Command::CloseChat),
        "chat" | "tab" => match arg.parse::<usize>() {
            Ok(n) if n >= 1 => Parsed::Command(Command::SwitchChat(n)),
            _ => Parsed::Error("/chat needs a chat number, like /chat 2".into()),
        },
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
        assert_eq!(cmd("/NEXT"), Command::Next);
        assert_eq!(cmd("/new"), Command::NewChat);
        assert_eq!(cmd("/chat 2"), Command::SwitchChat(2));
        assert!(matches!(parse("/chat zero"), Parsed::Error(_)));
        assert_eq!(cmd("/snip"), Command::Snip(None));
        assert_eq!(cmd("/snip intro"), Command::Snip(Some("intro".into())));
        assert_eq!(
            cmd("/snip-add intro Hi there, {partner_species}!"),
            Command::SnipAdd { name: "intro".into(), text: "Hi there, {partner_species}!".into() }
        );
        assert!(matches!(parse("/snip-add intro"), Parsed::Error(_)));
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

    #[test]
    fn tab_completes_command_names() {
        assert_eq!(complete("/lo").as_deref(), Some("/log "), "takes a path, so a space follows");
        assert_eq!(complete("/logs").as_deref(), Some("/logs"));
        assert_eq!(complete("/ST").as_deref(), Some("/stats"));
        assert_eq!(completions("/lo").iter().map(|(s, _)| name_of(s)).collect::<Vec<_>>(), ["/log", "/logs"]);
        assert_eq!(complete("/nope"), None);
        assert_eq!(complete("/log file"), None, "only while typing the name");
        assert_eq!(complete("//"), None);
        assert_eq!(complete("hi"), None);
    }
}
