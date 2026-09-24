//! The bundle model: what `catalog/catalog.json` holds, and its validator.
//!
//! The builder writes this shape and the host reads it, through the same
//! types and the same [`Bundle::parse`], so the two can't drift apart.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::broad;

/// The bundle schema this code reads and writes. A host refuses bundles with
/// another schema and keeps its current catalog.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub schema: u32,
    pub source: Source,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub repo: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    #[serde(default, skip_serializing_if = "Detect::is_empty")]
    pub detect: Detect,
    /// Folder names the game installs under (the manifest's `installDir`
    /// keys). Loose installs and Epic manifests are matched by them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "Executables::is_empty")]
    pub executables: Executables,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save: Vec<PathRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<PathRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detect {
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "id_list")]
    pub steam: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "id_list")]
    pub gog: Vec<u64>,
    /// Windows uninstall-registry key names (the subkey under
    /// `...\CurrentVersion\Uninstall`) of standalone installers. Only the
    /// addendum supplies them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uninstall: Vec<String>,
}

impl Detect {
    pub fn is_empty(&self) -> bool {
        self.steam.is_empty() && self.gog.is_empty() && self.uninstall.is_empty()
    }
}

/// A store id list is written as a bare number when there is one id, and as
/// an array when the manifest lists extras.
mod id_list {
    use super::*;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(u64),
        Many(Vec<u64>),
    }

    pub fn serialize<S: Serializer>(ids: &[u64], serializer: S) -> Result<S::Ok, S::Error> {
        if ids.len() == 1 { serializer.serialize_u64(ids[0]) } else { ids.serialize(serializer) }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u64>, D::Error> {
        Ok(match OneOrMany::deserialize(deserializer)? {
            OneOrMany::One(id) => vec![id],
            OneOrMany::Many(ids) => ids,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Executables {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macos: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linux: Vec<String>,
}

impl Executables {
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty() && self.macos.is_empty() && self.linux.is_empty()
    }

    pub fn for_platform(&self, platform: Platform) -> &[String] {
        match platform {
            Platform::Windows => &self.windows,
            Platform::Macos => &self.macos,
            Platform::Linux => &self.linux,
        }
    }

    pub fn for_platform_mut(&mut self, platform: Platform) -> &mut Vec<String> {
        match platform {
            Platform::Windows => &mut self.windows,
            Platform::Macos => &mut self.macos,
            Platform::Linux => &mut self.linux,
        }
    }
}

/// One `save` or `exclude` entry: a path template with an optional condition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathRule {
    #[serde(default, skip_serializing_if = "When::is_any")]
    pub when: When,
    pub path: String,
}

impl PathRule {
    pub fn new(path: impl Into<String>) -> Self {
        Self { when: When::default(), path: path.into() }
    }

    pub fn when(mut self, os: Option<Platform>, store: Option<Store>) -> Self {
        self.when = When { os, store };
        self
    }

    /// Whether this rule applies to a build and a store.
    pub fn applies(&self, build: Platform, store: Store) -> bool {
        self.when.os.is_none_or(|os| os == build) && self.when.store.is_none_or(|s| s == store)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct When {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Platform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<Store>,
}

impl When {
    pub fn is_any(&self) -> bool {
        self.os.is_none() && self.store.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Windows, Platform::Macos, Platform::Linux];

    pub fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Linux
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Macos => "macos",
            Platform::Linux => "linux",
        }
    }

    /// Whether this platform's usual file system ignores case. Paths are
    /// compared this way wherever the real directory can't be asked.
    pub fn case_insensitive(self) -> bool {
        !matches!(self, Platform::Linux)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an install was found. Only these stores are discovered, so the
/// builder drops conditions naming any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Store {
    Steam,
    Gog,
    Epic,
    Standalone,
}

impl Store {
    pub fn as_str(self) -> &'static str {
        match self {
            Store::Steam => "steam",
            Store::Gog => "gog",
            Store::Epic => "epic",
            Store::Standalone => "standalone",
        }
    }

    /// The name an install tag uses.
    pub fn display_name(self) -> &'static str {
        match self {
            Store::Steam => "Steam",
            Store::Gog => "GOG",
            Store::Epic => "Epic",
            Store::Standalone => "Standalone",
        }
    }

    pub fn parse(value: &str) -> Option<Store> {
        match value {
            "steam" => Some(Store::Steam),
            "gog" => Some(Store::Gog),
            "epic" => Some(Store::Epic),
            "standalone" => Some(Store::Standalone),
            _ => None,
        }
    }
}

impl fmt::Display for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Placeholders a bundle path may use. Anything else in braces is invalid.
pub const PLACEHOLDERS: &[&str] = &[
    "INSTALL_DIR",
    "HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "LOCALLOW",
    "DOCUMENTS",
    "PUBLIC",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "WINDIR",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "STEAM_ACCOUNT_ID",
    "STEAM_ID64",
    "STEAM_USERDATA",
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BundleError {
    #[error("the bundle isn't valid JSON: {0}")]
    Json(String),
    #[error("unsupported bundle schema {found} (this build reads {SCHEMA})")]
    Schema { found: u32 },
    #[error("{game}: {problem}")]
    Game { game: String, problem: String },
    #[error("duplicate game id {0}")]
    DuplicateId(String),
}

impl Bundle {
    /// Parses and validates a bundle. The builder validates its output with
    /// this too, so a bundle it writes always loads.
    pub fn parse(text: &str) -> Result<Bundle, BundleError> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| BundleError::Json(e.to_string()))?;
        let schema = value.get("schema").and_then(|s| s.as_u64()).unwrap_or(0) as u32;
        if schema != SCHEMA {
            return Err(BundleError::Schema { found: schema });
        }
        let bundle: Bundle = serde_json::from_value(value).map_err(|e| BundleError::Json(e.to_string()))?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), BundleError> {
        if self.schema != SCHEMA {
            return Err(BundleError::Schema { found: self.schema });
        }
        let mut ids = BTreeSet::new();
        for game in &self.games {
            if !ids.insert(game.id.clone()) {
                return Err(BundleError::DuplicateId(game.id.clone()));
            }
            game.validate().map_err(|problem| BundleError::Game { game: game.name.clone(), problem })?;
        }
        Ok(())
    }

    /// The canonical text of a bundle: pretty JSON with a trailing newline,
    /// byte-identical for identical content.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("bundle serializes");
        text.push('\n');
        text
    }

    pub fn game(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.id == id)
    }
}

impl Game {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("empty id".into());
        }
        if self.id.contains('#') {
            return Err(format!("id {:?} contains '#', which is reserved for install-level ids", self.id));
        }
        if self.name.trim().is_empty() {
            return Err("empty name".into());
        }
        if self.save.is_empty() {
            return Err("no save target".into());
        }
        for rule in self.save.iter().chain(&self.exclude) {
            validate_template(&rule.path).map_err(|e| format!("{}: {e}", rule.path))?;
        }
        for rule in &self.save {
            if broad::is_broad_template(&rule.path) {
                return Err(format!("{}: takes a broad folder", rule.path));
            }
        }
        for key in &self.detect.uninstall {
            if key.trim().is_empty() || key.contains(['\\', '/']) {
                return Err(format!("uninstall key {key:?} must be one subkey name"));
            }
        }
        for platform in Platform::ALL {
            for exe in self.executables.for_platform(platform) {
                if exe.is_empty() || exe.starts_with('/') || exe.contains('{') {
                    return Err(format!("executable {exe:?} must be relative to the install folder"));
                }
            }
        }
        Ok(())
    }
}

/// Checks that a path template starts at a placeholder or an absolute root
/// and only uses known placeholders.
pub fn validate_template(path: &str) -> Result<(), String> {
    if path.contains('\\') {
        return Err("use '/' as the separator".into());
    }
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let end = rest[start..].find('}').ok_or("unclosed placeholder")? + start;
        let name = &rest[start + 1..end];
        if !PLACEHOLDERS.contains(&name) {
            return Err(format!("unknown placeholder {{{name}}}"));
        }
        rest = &rest[end + 1..];
    }
    if !(path.starts_with('{') || path.starts_with('/')) {
        return Err("must start with a placeholder or '/'".into());
    }
    if path.split('/').any(|s| s == ".." || s == ".") {
        return Err("'.' and '..' segments aren't allowed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_text(games: &str) -> String {
        format!(r#"{{"schema":1,"source":{{"repo":"r","revision":"x"}},"games":{games}}}"#)
    }

    #[test]
    fn parses_the_documented_example() {
        let text = bundle_text(
            r#"[{"id":"steam-588650","name":"Dead Cells",
                "detect":{"steam":588650,"gog":[1,2]},
                "save":[{"path":"{INSTALL_DIR}/save/user_*.dat"},
                        {"when":{"store":"steam"},"path":"{STEAM_USERDATA}/588650/remote/user_*.dat"}],
                "exclude":[{"path":"{INSTALL_DIR}/save/dc_options.json"}]}]"#,
        );
        let bundle = Bundle::parse(&text).unwrap();
        let game = &bundle.games[0];
        assert_eq!(game.detect.steam, vec![588650]);
        assert_eq!(game.detect.gog, vec![1, 2]);
        assert_eq!(game.save[1].when.store, Some(Store::Steam));
        // Round trip keeps the single id as a bare number.
        let again = Bundle::parse(&bundle.to_json()).unwrap();
        assert_eq!(again, bundle);
        assert!(bundle.to_json().contains("\"steam\": 588650"));
    }

    #[test]
    fn refuses_another_schema() {
        let text = r#"{"schema":2,"source":{"repo":"r","revision":"x"},"games":[]}"#;
        assert_eq!(Bundle::parse(text), Err(BundleError::Schema { found: 2 }));
    }

    #[test]
    fn refuses_unknown_placeholders_and_broad_targets() {
        let bad = bundle_text(r#"[{"id":"a","name":"A","save":[{"path":"{NOPE}/x"}]}]"#);
        assert!(matches!(Bundle::parse(&bad), Err(BundleError::Game { .. })));
        let broad = bundle_text(r#"[{"id":"a","name":"A","save":[{"path":"{APPDATA}"}]}]"#);
        assert!(matches!(Bundle::parse(&broad), Err(BundleError::Game { .. })));
        let wildcard = bundle_text(r#"[{"id":"a","name":"A","save":[{"path":"{INSTALL_DIR}/save*"}]}]"#);
        assert!(matches!(Bundle::parse(&wildcard), Err(BundleError::Game { .. })));
    }

    #[test]
    fn refuses_duplicate_ids() {
        let text = bundle_text(
            r#"[{"id":"a","name":"A","save":[{"path":"{APPDATA}/A"}]},
                {"id":"a","name":"B","save":[{"path":"{APPDATA}/B"}]}]"#,
        );
        assert_eq!(Bundle::parse(&text), Err(BundleError::DuplicateId("a".into())));
    }
}
