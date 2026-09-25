//! Tab completion for what comes after a command: `/theme n` → `/theme nord`.

use super::*;

/// Commands whose argument is a file path.
const PATH_COMMANDS: &[&str] =
    &["log", "export", "export-profile", "import", "import-all", "drawer-export", "drawer-import", "backup"];

impl App {
    /// What the argument being typed could be: where it starts, then the candidates
    /// (ones starting with it first).
    pub fn arg_suggestions(&self) -> Option<(usize, Vec<String>)> {
        let text = self.input.text();
        if !text.starts_with('/') || text.starts_with("//") || self.input.cursor() != text.len() {
            return None;
        }
        let (name, arg) = text[1..].split_once(' ')?;
        let start = name.len() + 2;
        let name = name.to_ascii_lowercase();
        if PATH_COMMANDS.contains(&name.as_str()) {
            let found = config::complete_path(arg, 8);
            return (!found.is_empty()).then_some((start, found));
        }
        let candidates: Vec<String> = match name.as_str() {
            "profile" => self.config.profiles.iter().map(|p| p.name.clone()).collect(),
            "theme" => self.themes.iter().map(|t| t.name.clone()).collect(),
            "snip" => self.drawer.snippets.iter().map(|s| s.name.clone()).collect(),
            "chat" => (1..=self.session_count()).map(|n| n.to_string()).collect(),
            "untrust" => self.config.settings.images.trusted_domains.clone(),
            "trust" => self.untrusted_hosts(),
            "buddy" => self.buddy_names(),
            "dict" => {
                let installed = crate::spell::installed(&self.dictionary_dir());
                let mut all = vec!["list".to_owned()];
                all.extend(crate::spell::DOWNLOADABLE.iter().map(|l| format!("get {l}")));
                all.extend(installed.iter().map(|l| format!("use {l}")));
                all
            }
            _ => return None,
        };
        let typed = arg.to_lowercase();
        let starts = candidates.iter().filter(|c| c.to_lowercase().starts_with(&typed));
        let contains =
            candidates.iter().filter(|c| !c.to_lowercase().starts_with(&typed) && c.to_lowercase().contains(&typed));
        let found: Vec<String> = starts.chain(contains).take(8).cloned().collect();
        let only_itself = found.len() == 1 && found[0] == arg;
        (!found.is_empty() && !only_itself).then_some((start, found))
    }

    /// Tab after `/theme n`: fill in the first suggestion.
    pub(super) fn complete_arg(&mut self) -> bool {
        let Some((start, found)) = self.arg_suggestions() else { return false };
        self.input.replace_to_cursor(start, &found[0]);
        true
    }

    /// Hosts of links in this chat that aren't trusted yet, for `/trust`.
    fn untrusted_hosts(&self) -> Vec<String> {
        let trusted = &self.config.settings.images.trusted_domains;
        let mut hosts: Vec<String> = self
            .chat
            .links()
            .into_iter()
            .filter_map(|l| url::Url::parse(&l.url).ok()?.host_str().map(str::to_owned))
            .filter(|h| !trusted.iter().any(|d| crate::links::host_matches(h, d)))
            .collect();
        hosts.dedup();
        hosts
    }
}
