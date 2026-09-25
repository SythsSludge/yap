//! Colour themes: the website's three, some popular palettes, and user TOML files.

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

macro_rules! theme_slots {
    ($($slot:ident: $doc:literal),* $(,)?) => {
        /// Every colour a theme defines. `Color::Reset` means "terminal default".
        #[derive(Debug, Clone, PartialEq)]
        pub struct Theme {
            pub name: String,
            $(#[doc = $doc] pub $slot: Color,)*
        }

        impl Theme {
            pub const SLOTS: &'static [&'static str] = &[$(stringify!($slot)),*];

            fn slot_mut(&mut self, slot: &str) -> Option<&mut Color> {
                match slot {
                    $(stringify!($slot) => Some(&mut self.$slot),)*
                    _ => None,
                }
            }
        }
    };
}

theme_slots! {
    bg: "Page background.",
    fg: "Body text.",
    muted: "Secondary text, hints, borders of unfocused panes.",
    surface: "Background of input fields and popups.",
    accent: "Primary accent: focused borders, active tab, buttons.",
    you: "Your name in chat.",
    partner: "Your partner's name in chat.",
    system: "System messages.",
    link: "Links.",
    highlight: "Kinks you have in common with your partner.",
    selection_bg: "Selected list row background.",
    selection_fg: "Selected list row text.",
    success: "Connected / OK indicators.",
    warning: "Pending / attention indicators.",
    error: "Errors and destructive actions.",
    traffic_in: "Incoming frames in the traffic viewer.",
    traffic_out: "Outgoing frames in the traffic viewer.",
}

impl Theme {
    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }

    pub fn muted(&self) -> Style {
        Style::new().fg(self.muted)
    }

    pub fn border(&self, focused: bool) -> Style {
        Style::new().fg(if focused { self.accent } else { self.muted })
    }

    pub fn selected(&self) -> Style {
        Style::new().fg(self.selection_fg).bg(self.selection_bg).add_modifier(Modifier::BOLD)
    }

    pub fn link(&self) -> Style {
        Style::new().fg(self.link).add_modifier(Modifier::UNDERLINED)
    }

    pub fn system(&self) -> Style {
        Style::new().fg(self.system).add_modifier(Modifier::BOLD)
    }
}

/// Parse a colour: `#rrggbb`, `#rgb`, a 0-255 palette index, a name (`red`,
/// `lightblue`...), or `default`/`reset` for the terminal's own colour.
pub fn parse_color(s: &str) -> Result<Color, String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "default" | "reset" | "none" | "terminal") {
        return Ok(Color::Reset);
    }
    if let Some(hex) = s.strip_prefix('#')
        && hex.len() == 3
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        let d = |i: usize| u8::from_str_radix(&hex[i..=i], 16).expect("checked") * 17;
        return Ok(Color::Rgb(d(0), d(1), d(2)));
    }
    Color::from_str(s).map_err(|_| format!("`{s}` is not a colour"))
}

fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Build a theme from a compact palette. Traffic colours reuse link/success.
#[allow(clippy::too_many_arguments)]
fn palette(
    name: &str,
    bg: Color,
    fg: Color,
    muted: Color,
    surface: Color,
    accent: Color,
    you: Color,
    partner: Color,
    highlight: Color,
    success: Color,
    warning: Color,
    error: Color,
) -> Theme {
    Theme {
        name: name.into(),
        bg,
        fg,
        muted,
        surface,
        accent,
        you,
        partner,
        system: muted,
        link: accent,
        highlight,
        selection_bg: accent,
        selection_fg: bg,
        success,
        warning,
        error,
        traffic_in: partner,
        traffic_out: you,
    }
}

pub fn builtins() -> Vec<Theme> {
    let mut themes = vec![
        // Follows the terminal's own 16-colour palette, so it matches your kitty theme.
        Theme {
            name: "terminal".into(),
            bg: Color::Reset,
            fg: Color::Reset,
            muted: Color::DarkGray,
            surface: Color::Reset,
            accent: Color::Blue,
            you: Color::Cyan,
            partner: Color::Magenta,
            system: Color::DarkGray,
            link: Color::Blue,
            highlight: Color::Yellow,
            selection_bg: Color::Blue,
            selection_fg: Color::Black,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            traffic_in: Color::Magenta,
            traffic_out: Color::Cyan,
        },
        // The website's default, from `scss/dark.scss`.
        palette(
            "dark",
            rgb(0x36393e),
            rgb(0xafb0b1),
            rgb(0x949494),
            rgb(0x26272b),
            rgb(0x375a7f),
            rgb(0xffffff),
            rgb(0xffffff),
            rgb(0x5b8fc7),
            rgb(0x3fb950),
            rgb(0xe3b341),
            rgb(0xe74c3c),
        ),
        palette(
            "oled-dark",
            rgb(0x000000),
            rgb(0xafb0b1),
            rgb(0x949494),
            rgb(0x26272b),
            rgb(0x375a7f),
            rgb(0xffffff),
            rgb(0xffffff),
            rgb(0x5b8fc7),
            rgb(0x3fb950),
            rgb(0xe3b341),
            rgb(0xe74c3c),
        ),
        palette(
            "light",
            rgb(0xffffff),
            rgb(0x333333),
            rgb(0x949494),
            rgb(0xf5f5f5),
            rgb(0x375a7f),
            rgb(0x000000),
            rgb(0x000000),
            rgb(0x375a7f),
            rgb(0x1a7f37),
            rgb(0x9a6700),
            rgb(0xe74c3c),
        ),
        palette(
            "gruvbox",
            rgb(0x282828),
            rgb(0xebdbb2),
            rgb(0x928374),
            rgb(0x3c3836),
            rgb(0xfe8019),
            rgb(0x83a598),
            rgb(0xd3869b),
            rgb(0xfabd2f),
            rgb(0xb8bb26),
            rgb(0xfabd2f),
            rgb(0xfb4934),
        ),
        palette(
            "catppuccin-mocha",
            rgb(0x1e1e2e),
            rgb(0xcdd6f4),
            rgb(0x7f849c),
            rgb(0x313244),
            rgb(0xcba6f7),
            rgb(0x89b4fa),
            rgb(0xf5c2e7),
            rgb(0xf9e2af),
            rgb(0xa6e3a1),
            rgb(0xf9e2af),
            rgb(0xf38ba8),
        ),
        palette(
            "nord",
            rgb(0x2e3440),
            rgb(0xd8dee9),
            rgb(0x616e88),
            rgb(0x3b4252),
            rgb(0x88c0d0),
            rgb(0x81a1c1),
            rgb(0xb48ead),
            rgb(0xebcb8b),
            rgb(0xa3be8c),
            rgb(0xebcb8b),
            rgb(0xbf616a),
        ),
        palette(
            "dracula",
            rgb(0x282a36),
            rgb(0xf8f8f2),
            rgb(0x6272a4),
            rgb(0x44475a),
            rgb(0xbd93f9),
            rgb(0x8be9fd),
            rgb(0xff79c6),
            rgb(0xf1fa8c),
            rgb(0x50fa7b),
            rgb(0xffb86c),
            rgb(0xff5555),
        ),
        palette(
            "rose-pine",
            rgb(0x191724),
            rgb(0xe0def4),
            rgb(0x6e6a86),
            rgb(0x26233a),
            rgb(0xc4a7e7),
            rgb(0x9ccfd8),
            rgb(0xebbcba),
            rgb(0xf6c177),
            rgb(0x31748f),
            rgb(0xf6c177),
            rgb(0xeb6f92),
        ),
    ];
    // Light backgrounds need dark selection text; everything else inverts via bg.
    if let Some(light) = themes.iter_mut().find(|t| t.name == "light") {
        light.selection_fg = rgb(0xffffff);
    }
    themes
}

/// A user theme file, e.g. `~/.config/yap/themes/mine.toml`:
///
/// ```toml
/// name = "mine"          # optional, defaults to the file stem
/// extends = "dark"       # optional, defaults to "dark"
/// [colors]
/// accent = "#ff79c6"
/// bg = "default"
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: Option<String>,
    extends: Option<String>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

/// Parse a theme file against the themes loaded so far (for `extends`).
pub fn parse_theme(src: &str, fallback_name: &str, known: &[Theme]) -> Result<Theme, String> {
    let file: ThemeFile = toml::from_str(src).map_err(|e| e.message().to_owned())?;
    let parent = file.extends.as_deref().unwrap_or("dark");
    let mut theme =
        known.iter().find(|t| t.name == parent).cloned().ok_or_else(|| format!("extends unknown theme `{parent}`"))?;
    theme.name = file.name.unwrap_or_else(|| fallback_name.to_owned());
    for (slot, value) in &file.colors {
        let color = parse_color(value).map_err(|e| format!("colors.{slot}: {e}"))?;
        *theme
            .slot_mut(slot)
            .ok_or_else(|| format!("unknown colour slot `{slot}` (valid: {})", Theme::SLOTS.join(", ")))? = color;
    }
    Ok(theme)
}

/// Built-ins plus every `*.toml` in `dir`, sorted by file name so `extends` can refer
/// to earlier files. A user theme with a built-in's name replaces it. Returns the themes
/// and any per-file errors.
pub fn load_all(dir: Option<&Path>) -> (Vec<Theme>, Vec<String>) {
    let mut themes = builtins();
    let mut errors = Vec::new();
    let Some(entries) = dir.and_then(|d| std::fs::read_dir(d).ok()) else {
        return (themes, errors);
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    for path in paths {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("custom").to_owned();
        let parsed =
            std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|src| parse_theme(&src, &stem, &themes));
        match parsed {
            Ok(theme) => match themes.iter_mut().find(|t| t.name == theme.name) {
                Some(existing) => *existing = theme,
                None => themes.push(theme),
            },
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    (themes, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colour_formats() {
        assert_eq!(parse_color("#375a7f"), Ok(Color::Rgb(0x37, 0x5a, 0x7f)));
        assert_eq!(parse_color("#fff"), Ok(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_color("red"), Ok(Color::Red));
        assert_eq!(parse_color("LightBlue"), Ok(Color::LightBlue));
        assert_eq!(parse_color("208"), Ok(Color::Indexed(208)));
        assert_eq!(parse_color("default"), Ok(Color::Reset));
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("blurple").is_err());
    }

    #[test]
    fn builtins_have_unique_names_and_include_site_themes() {
        let themes = builtins();
        let mut names: Vec<_> = themes.iter().map(|t| t.name.as_str()).collect();
        for site in ["dark", "oled-dark", "light"] {
            assert!(names.contains(&site));
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), themes.len());
        let dark = themes.iter().find(|t| t.name == "dark").unwrap();
        assert_eq!(dark.bg, Color::Rgb(0x36, 0x39, 0x3e));
        assert_eq!(dark.accent, Color::Rgb(0x37, 0x5a, 0x7f));
    }

    #[test]
    fn user_theme_extends_and_overrides() {
        let src = r##"
            extends = "nord"
            [colors]
            accent = "#ff00ff"
            bg = "default"
        "##;
        let theme = parse_theme(src, "mine", &builtins()).unwrap();
        let nord = builtins().into_iter().find(|t| t.name == "nord").unwrap();
        assert_eq!(theme.name, "mine");
        assert_eq!(theme.accent, Color::Rgb(255, 0, 255));
        assert_eq!(theme.bg, Color::Reset);
        assert_eq!(theme.fg, nord.fg);
    }

    #[test]
    fn user_theme_errors_are_descriptive() {
        let known = builtins();
        let err = parse_theme("[colors]\nacent = \"red\"", "x", &known).unwrap_err();
        assert!(err.contains("unknown colour slot `acent`"), "{err}");
        let err = parse_theme("[colors]\nfg = \"nah\"", "x", &known).unwrap_err();
        assert!(err.contains("colors.fg"), "{err}");
        let err = parse_theme("extends = \"ghost\"", "x", &known).unwrap_err();
        assert!(err.contains("ghost"), "{err}");
        assert!(parse_theme("bogus = 1", "x", &known).is_err());
    }

    #[test]
    fn loads_theme_directory_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a-base.toml"), "extends = \"dracula\"\n[colors]\nfg = \"red\"").unwrap();
        std::fs::write(dir.path().join("b-child.toml"), "extends = \"a-base\"").unwrap();
        std::fs::write(dir.path().join("c-broken.toml"), "[colors]\nfg = 5").unwrap();
        std::fs::write(dir.path().join("dark.toml"), "extends = \"light\"").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

        let (themes, errors) = load_all(Some(dir.path()));
        let child = themes.iter().find(|t| t.name == "b-child").unwrap();
        assert_eq!(child.fg, Color::Red);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("c-broken"));
        // Overriding a built-in replaces it in place.
        let dark = themes.iter().find(|t| t.name == "dark").unwrap();
        assert_eq!(dark.bg, Color::Rgb(255, 255, 255));
        assert_eq!(themes.iter().filter(|t| t.name == "dark").count(), 1);
    }
}
