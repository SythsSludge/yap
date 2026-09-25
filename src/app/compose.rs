//! Writing long messages in an external editor.

use super::*;

/// Turn editor text into one line for the site's single-line input: lines within a
/// paragraph are joined with spaces, paragraphs with `paragraph_break`.
pub fn join_paragraphs(text: &str, paragraph_break: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs.join(paragraph_break)
}

/// The inverse, for putting a message back into the editor: paragraph breaks become
/// blank lines again.
pub fn split_paragraphs(text: &str, paragraph_break: &str) -> String {
    if paragraph_break.trim().is_empty() {
        return text.to_owned();
    }
    let mut out = text.split(paragraph_break).collect::<Vec<_>>().join("\n\n");
    out.push('\n');
    out
}

impl App {
    /// `$VISUAL`, then `$EDITOR`, then `vi`, unless a command is set in Settings.
    pub fn editor_command(&self) -> String {
        let configured = self.config.settings.editor.trim();
        if !configured.is_empty() {
            return configured.to_owned();
        }
        ["VISUAL", "EDITOR"]
            .iter()
            .filter_map(|var| std::env::var(var).ok())
            .find(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "vi".into())
    }

    /// Open the message box's text in the external editor.
    pub fn compose_in_editor(&mut self) {
        let text = split_paragraphs(self.input.text(), &self.config.settings.paragraph_break);
        let command = self.editor_command();
        self.effect(Effect::OpenEditor { command, text, purpose: EditorPurpose::Message });
    }

    pub fn on_editor(&mut self, purpose: EditorPurpose, result: Result<String, String>) {
        let text = match result {
            Ok(text) => text,
            Err(e) => return self.toast(Level::Error, format!("Editor: {e}")),
        };
        match purpose {
            EditorPurpose::Message => {
                let line = join_paragraphs(&text, &self.config.settings.paragraph_break);
                self.tab = Tab::Chat;
                self.chat_focus = ChatFocus::Input;
                self.input.set(&line);
                self.input_changed();
                let len = line.encode_utf16().count();
                if len >= MAX_MESSAGE_LEN {
                    self.toast(
                        Level::Warning,
                        format!("That's {len} characters; the limit is {}.", MAX_MESSAGE_LEN - 1),
                    );
                }
            }
            EditorPurpose::Snippet(index) => {
                if let Some(snippet) = self.drawer.snippets.get_mut(index) {
                    snippet.text = text.trim_end().to_owned();
                    self.drawer_changed();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_lines_and_paragraphs() {
        let text = "  First line\nsame paragraph.\n\n\nSecond   paragraph.\n\nThird\n";
        assert_eq!(join_paragraphs(text, " / "), "First line same paragraph. / Second   paragraph. / Third");
        assert_eq!(join_paragraphs("\n\n", " / "), "");
        assert_eq!(join_paragraphs("one", " / "), "one");
    }

    #[test]
    fn splits_back_for_editing() {
        assert_eq!(split_paragraphs("a / b", " / "), "a\n\nb\n");
        assert_eq!(join_paragraphs(&split_paragraphs("a / b", " / "), " / "), "a / b");
        assert_eq!(split_paragraphs("keep / as is", " "), "keep / as is");
    }
}
