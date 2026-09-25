//! Persistent settings, preference profiles, and import/export.

use crate::links::DEFAULT_TRUSTED_DOMAINS;
use crate::prefs::Preferences;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_SERVER: &str = "wss://www.yiffspot.com/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatStyle {
    /// Name on its own line above the message, like the website.
    #[default]
    Cozy,
    /// `12:01 You: message` on one line.
    Compact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifySettings {
    /// Ring the terminal bell (kitty can turn this into a sound or urgency hint).
    pub bell: bool,
    /// Flash the window title while unfocused, like the website does with the tab.
    pub title: bool,
    /// Send a desktop notification via OSC 99 (kitty) / OSC 777.
    pub desktop: bool,
    /// Also notify for each message, not only partner connect/leave.
    pub on_message: bool,
}

impl Default for NotifySettings {
    fn default() -> Self {
        NotifySettings { bell: true, title: true, desktop: false, on_message: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageSettings {
    pub enabled: bool,
    /// Load previews automatically for trusted links; otherwise only on request.
    pub auto_load: bool,
    pub https_only: bool,
    pub trusted_domains: Vec<String>,
    pub max_rows: u16,
    pub max_cols: u16,
    pub max_bytes: u64,
}

impl Default for ImageSettings {
    fn default() -> Self {
        ImageSettings {
            enabled: true,
            auto_load: true,
            https_only: true,
            trusted_domains: DEFAULT_TRUSTED_DOMAINS.iter().map(|s| (*s).into()).collect(),
            max_rows: 12,
            max_cols: 48,
            max_bytes: 10 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrafficSettings {
    /// How many frames the traffic viewer keeps.
    pub capacity: usize,
    pub hide_heartbeat: bool,
}

impl Default for TrafficSettings {
    fn default() -> Self {
        TrafficSettings { capacity: 5000, hide_heartbeat: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub server_url: String,
    /// The live site predates the language field; turn this off if it ever starts
    /// rejecting preferences with it.
    pub send_language: bool,
    pub timestamps: bool,
    pub chat_style: ChatStyle,
    pub show_sidebar: bool,
    pub auto_reconnect: bool,
    /// Ask before leaving, blocking or re-rolling a partner.
    pub confirm_actions: bool,
    pub notify: NotifySettings,
    pub images: ImageSettings,
    pub traffic: TrafficSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "dark".into(),
            server_url: DEFAULT_SERVER.into(),
            send_language: true,
            timestamps: true,
            chat_style: ChatStyle::default(),
            show_sidebar: true,
            auto_reconnect: true,
            confirm_actions: true,
            notify: NotifySettings::default(),
            images: ImageSettings::default(),
            traffic: TrafficSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub preferences: Preferences,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub active_profile: String,
    pub settings: Settings,
    pub profiles: Vec<Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            active_profile: "default".into(),
            settings: Settings::default(),
            profiles: vec![Profile { name: "default".into(), preferences: Preferences::default() }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("profile name can't be empty")]
    EmptyName,
    #[error("a profile called `{0}` already exists")]
    Exists(String),
    #[error("no profile called `{0}`")]
    NotFound(String),
    #[error("can't delete the last profile")]
    LastProfile,
}

impl Config {
    /// Repair anything a hand edit could have broken: no profiles, a dangling active
    /// profile, duplicate names, or preference values the server won't accept.
    pub fn normalize(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.profiles.is_empty() {
            self.profiles.push(Profile { name: "default".into(), preferences: Preferences::default() });
        }
        let mut seen = std::collections::HashSet::new();
        for i in 0..self.profiles.len() {
            let name = self.profiles[i].name.trim().to_owned();
            let name = if name.is_empty() { "profile".to_owned() } else { name };
            let unique = unique_name(&name, |n| seen.contains(n));
            seen.insert(unique.clone());
            self.profiles[i].name = unique;
            let dropped = self.profiles[i].preferences.sanitize();
            if !dropped.is_empty() {
                warnings.push(format!(
                    "profile `{}`: dropped unknown values {}",
                    self.profiles[i].name,
                    dropped.join(", ")
                ));
            }
        }
        if self.find(&self.active_profile).is_none() {
            self.active_profile = self.profiles[0].name.clone();
        }
        let domains = std::mem::take(&mut self.settings.images.trusted_domains);
        for d in domains {
            match crate::links::normalize_domain(&d) {
                Some(n) if !self.settings.images.trusted_domains.contains(&n) => {
                    self.settings.images.trusted_domains.push(n);
                }
                Some(_) => {}
                None => warnings.push(format!("ignored invalid trusted domain `{d}`")),
            }
        }
        warnings
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.profiles.iter().position(|p| p.name == name)
    }

    pub fn active(&self) -> &Profile {
        &self.profiles[self.find(&self.active_profile).unwrap_or(0)]
    }

    pub fn active_mut(&mut self) -> &mut Profile {
        let i = self.find(&self.active_profile).unwrap_or(0);
        &mut self.profiles[i]
    }

    pub fn set_active(&mut self, name: &str) -> Result<(), ProfileError> {
        self.find(name).ok_or_else(|| ProfileError::NotFound(name.into()))?;
        self.active_profile = name.into();
        Ok(())
    }

    fn check_new_name(&self, name: &str) -> Result<String, ProfileError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        if self.find(name).is_some() {
            return Err(ProfileError::Exists(name.into()));
        }
        Ok(name.into())
    }

    pub fn create_profile(&mut self, name: &str, preferences: Preferences) -> Result<String, ProfileError> {
        let name = self.check_new_name(name)?;
        self.profiles.push(Profile { name: name.clone(), preferences });
        Ok(name)
    }

    pub fn rename_profile(&mut self, from: &str, to: &str) -> Result<(), ProfileError> {
        let i = self.find(from).ok_or_else(|| ProfileError::NotFound(from.into()))?;
        if from == to.trim() {
            return Ok(());
        }
        let to = self.check_new_name(to)?;
        if self.active_profile == from {
            self.active_profile = to.clone();
        }
        self.profiles[i].name = to;
        Ok(())
    }

    pub fn delete_profile(&mut self, name: &str) -> Result<(), ProfileError> {
        let i = self.find(name).ok_or_else(|| ProfileError::NotFound(name.into()))?;
        if self.profiles.len() == 1 {
            return Err(ProfileError::LastProfile);
        }
        self.profiles.remove(i);
        if self.active_profile == name {
            self.active_profile = self.profiles[i.min(self.profiles.len() - 1)].name.clone();
        }
        Ok(())
    }

    /// A name based on `base` that no existing profile uses: `base`, `base (2)`, ...
    pub fn unique_profile_name(&self, base: &str) -> String {
        unique_name(base, |n| self.find(n).is_some())
    }

    pub fn load(path: &Path) -> Result<(Self, Vec<String>)> {
        let mut config = match std::fs::read_to_string(path) {
            Ok(src) => toml::from_str::<Config>(&src)
                .with_context(|| format!("{} is not a valid yap config", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let warnings = config.normalize();
        Ok((config, warnings))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &toml::to_string_pretty(self)?)
    }
}

fn unique_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|candidate| !taken(candidate)).expect("infinite iterator")
}

/// Write via a temp file and rename so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// The file format for `export` / `import`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportDoc {
    /// Format marker and version, so imports can tell our files apart.
    pub yap_export: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Toml,
    Json,
}

impl Format {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("json") => Format::Json,
            _ => Format::Toml,
        }
    }
}

impl ExportDoc {
    pub fn new(profiles: Vec<Profile>, settings: Option<Settings>) -> Self {
        ExportDoc { yap_export: 1, settings, profiles }
    }

    pub fn render(&self, format: Format) -> Result<String> {
        Ok(match format {
            Format::Toml => toml::to_string_pretty(self)?,
            Format::Json => serde_json::to_string_pretty(self)?,
        })
    }

    /// Parse any supported import: our own TOML/JSON exports, or a web-client
    /// `localStorage` dump (which becomes a single profile named `web_name`).
    pub fn parse(src: &str, web_name: &str) -> Result<Self> {
        let trimmed = src.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            let value: serde_json::Value = serde_json::from_str(src).context("invalid JSON")?;
            if value.get("yap_export").is_some() {
                return serde_json::from_value(value).context("invalid yap export");
            }
            if let Some(prefs) = Preferences::from_web_local_storage(&value) {
                return Ok(ExportDoc::new(vec![Profile { name: web_name.into(), preferences: prefs }], None));
            }
            bail!("JSON is neither a yap export nor a yiffspot.com localStorage dump");
        }
        let doc: ExportDoc = toml::from_str(src).context("invalid yap export")?;
        Ok(doc)
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ImportReport {
    pub profiles: Vec<String>,
    pub settings_applied: bool,
    pub warnings: Vec<String>,
}

impl Config {
    /// Export one profile, or all profiles plus settings when `name` is `None`.
    pub fn export(&self, name: Option<&str>) -> Result<ExportDoc, ProfileError> {
        Ok(match name {
            Some(name) => {
                let i = self.find(name).ok_or_else(|| ProfileError::NotFound(name.into()))?;
                ExportDoc::new(vec![self.profiles[i].clone()], None)
            }
            None => ExportDoc::new(self.profiles.clone(), Some(self.settings.clone())),
        })
    }

    /// Merge an import. Profiles are always added (renamed on clashes, never
    /// overwritten); settings only replace ours when `apply_settings` is set.
    pub fn import(&mut self, doc: ExportDoc, apply_settings: bool) -> ImportReport {
        let mut report = ImportReport::default();
        for mut profile in doc.profiles {
            let dropped = profile.preferences.sanitize();
            let base = if profile.name.trim().is_empty() { "imported" } else { profile.name.trim() };
            let name = self.unique_profile_name(base);
            if !dropped.is_empty() {
                report.warnings.push(format!("{name}: dropped unknown values {}", dropped.join(", ")));
            }
            profile.name = name.clone();
            self.profiles.push(profile);
            report.profiles.push(name);
        }
        if apply_settings && let Some(settings) = doc.settings {
            self.settings = settings;
            report.settings_applied = true;
        }
        report.warnings.extend(self.normalize());
        report
    }
}

pub fn export_to_file(doc: &ExportDoc, path: &Path) -> Result<()> {
    write_atomic(path, &doc.render(Format::from_path(path))?)
}

pub fn read_import(path: &Path) -> Result<ExportDoc> {
    let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("web");
    ExportDoc::parse(&src, stem)
}

/// Expand a leading `~/` so prompts accept the paths people actually type.
pub fn expand_tilde(input: &str) -> PathBuf {
    let input = input.trim();
    match (input.strip_prefix("~/"), directories::BaseDirs::new()) {
        (Some(rest), Some(dirs)) => dirs.home_dir().join(rest),
        _ if input == "~" => directories::BaseDirs::new().map_or_else(|| input.into(), |d| d.home_dir().into()),
        _ => PathBuf::from(input),
    }
}

/// Where everything lives on disk.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_file: PathBuf,
    pub themes_dir: PathBuf,
    pub drawer_file: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    /// XDG locations (`~/.config/yap`, `~/.local/share/yap`), with the config file
    /// optionally overridden.
    pub fn discover(config_override: Option<PathBuf>) -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "yap").context("no home directory")?;
        let config_file = config_override.unwrap_or_else(|| dirs.config_dir().join("config.toml"));
        Ok(Paths {
            themes_dir: dirs.config_dir().join("themes"),
            drawer_file: dirs.data_dir().join("drawer.toml"),
            data_dir: dirs.data_dir().to_owned(),
            config_file,
        })
    }

    /// Everything under one directory; used by tests.
    pub fn in_dir(dir: &Path) -> Self {
        Paths {
            config_file: dir.join("config.toml"),
            themes_dir: dir.join("themes"),
            drawer_file: dir.join("drawer.toml"),
            data_dir: dir.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Field;
    use pretty_assertions::assert_eq;

    fn named(name: &str, species: &str) -> Profile {
        let mut preferences = Preferences::default();
        preferences.toggle(Field::Species, species);
        Profile { name: name.into(), preferences }
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (config, warnings) = Config::load(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(config, Config::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn broken_file_is_an_error_not_silently_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "settings = 5").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(format!("{err:#}").contains("not a valid yap config"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "settings = 5");
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        let mut config = Config::default();
        config.profiles.push(named("fox", "Fox"));
        config.settings.theme = "nord".into();
        config.save(&path).unwrap();
        let (loaded, warnings) = Config::load(&path).unwrap();
        assert_eq!(loaded, config);
        assert!(warnings.is_empty());
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn partial_config_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[settings]\ntheme = \"nord\"\n[settings.images]\nmax_rows = 4\n").unwrap();
        let (config, _) = Config::load(&path).unwrap();
        assert_eq!(config.settings.theme, "nord");
        assert_eq!(config.settings.images.max_rows, 4);
        assert!(config.settings.images.https_only);
        assert_eq!(config.profiles.len(), 1);
    }

    #[test]
    fn normalize_repairs_hand_edits() {
        let mut config = Config {
            active_profile: "ghost".into(),
            profiles: vec![named("a", "Fox"), named("a", "Wolf"), named("  ", "Cat")],
            ..Config::default()
        };
        config.settings.images.trusted_domains = vec!["E621.NET".into(), "e621.net".into(), "bad domain".into()];
        let warnings = config.normalize();
        let names: Vec<_> = config.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["a", "a (2)", "profile"]);
        assert_eq!(config.active_profile, "a");
        assert_eq!(config.settings.images.trusted_domains, vec!["e621.net"]);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn profile_crud() {
        let mut config = Config::default();
        assert_eq!(config.create_profile("  ", Preferences::default()), Err(ProfileError::EmptyName));
        config.create_profile("fox", Preferences::default()).unwrap();
        assert_eq!(config.create_profile("fox", Preferences::default()), Err(ProfileError::Exists("fox".into())));
        config.set_active("fox").unwrap();
        config.rename_profile("fox", "vulpine").unwrap();
        assert_eq!(config.active().name, "vulpine");
        assert_eq!(config.rename_profile("vulpine", "default"), Err(ProfileError::Exists("default".into())));
        config.delete_profile("vulpine").unwrap();
        assert_eq!(config.active().name, "default");
        assert_eq!(config.delete_profile("default"), Err(ProfileError::LastProfile));
        assert_eq!(config.set_active("nope"), Err(ProfileError::NotFound("nope".into())));
    }

    #[test]
    fn export_import_single_profile_renames_on_clash() {
        let mut config = Config::default();
        config.profiles.push(named("fox", "Fox"));
        let doc = config.export(Some("fox")).unwrap();
        assert!(doc.settings.is_none());

        for format in [Format::Toml, Format::Json] {
            let text = doc.render(format).unwrap();
            let parsed = ExportDoc::parse(&text, "x").unwrap();
            assert_eq!(parsed, doc);
        }
        let report = config.import(doc, true);
        assert_eq!(report.profiles, vec!["fox (2)"]);
        assert!(!report.settings_applied);
        assert_eq!(config.profiles.last().unwrap().preferences.species.as_deref(), Some("Fox"));
    }

    #[test]
    fn full_export_carries_settings_only_applied_on_request() {
        let mut source = Config::default();
        source.settings.theme = "dracula".into();
        let doc = source.export(None).unwrap();

        let mut target = Config::default();
        target.import(doc.clone(), false);
        assert_eq!(target.settings.theme, "dark");
        target.import(doc, true);
        assert_eq!(target.settings.theme, "dracula");
    }

    #[test]
    fn imports_web_local_storage_and_sanitizes() {
        let src =
            r#"{"gender":"Male","species":"Fox","role":"Switch","partnerRole":"Dominant","kinks":"Biting,NotAKink"}"#;
        let doc = ExportDoc::parse(src, "yiffspot").unwrap();
        let mut config = Config::default();
        let report = config.import(doc, false);
        assert_eq!(report.profiles, vec!["yiffspot"]);
        let p = &config.profiles.last().unwrap().preferences;
        assert_eq!(p.kinks, vec!["Biting"]);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn rejects_unrelated_json() {
        let err = ExportDoc::parse(r#"{"hello":"world"}"#, "x").unwrap_err();
        assert!(err.to_string().contains("neither"));
    }

    #[test]
    fn file_roundtrip_picks_format_from_extension() {
        let dir = tempfile::tempdir().unwrap();
        let doc = Config::default().export(None).unwrap();
        for name in ["out.toml", "out.json", "out.JSON"] {
            let path = dir.path().join(name);
            export_to_file(&doc, &path).unwrap();
            let is_json = std::fs::read_to_string(&path).unwrap().starts_with('{');
            assert_eq!(is_json, name.to_lowercase().ends_with("json"), "{name}");
            assert_eq!(read_import(&path).unwrap(), doc);
        }
    }

    #[test]
    fn unique_names() {
        let mut config = Config::default();
        assert_eq!(config.unique_profile_name("new"), "new");
        assert_eq!(config.unique_profile_name("default"), "default (2)");
        config.create_profile("default (2)", Preferences::default()).unwrap();
        assert_eq!(config.unique_profile_name("default"), "default (3)");
    }
}
