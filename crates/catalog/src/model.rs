//! Bundle model shared by the builder (`catalog-build`) and the resolver.
//!
//! `catalog.json` is the contract between the two units, so the model and
//! its validator live here: builder output and resolver input cannot drift.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeMap, fmt};

/// Major schema version. A host refuses bundles with a different major version.
pub const SCHEMA: u32 = 1;

/// A store product identifier list. Serialized as a bare number for a single
/// id and as an array when extras exist, matching Section 3.4.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdList(pub Vec<u64>);

impl IdList {
    pub fn one(id: u64) -> Self {
        Self(vec![id])
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn first(&self) -> Option<u64> {
        self.0.first().copied()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IdRepr {
    One(u64),
    Many(Vec<u64>),
}

impl Serialize for IdList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0.as_slice() {
            [one] => serializer.serialize_u64(*one),
            many => many.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for IdList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match IdRepr::deserialize(deserializer)? {
            IdRepr::One(id) => Self::one(id),
            IdRepr::Many(ids) => Self(ids),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        })
    }
}

impl std::str::FromStr for Platform {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "windows" => Ok(Self::Windows),
            "macos" | "mac" => Ok(Self::Macos),
            "linux" => Ok(Self::Linux),
            other => Err(ParseError(format!("unknown platform {other:?}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Store {
    Steam,
    Gog,
    Epic,
    Standalone,
}

impl fmt::Display for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Steam => "steam",
            Self::Gog => "gog",
            Self::Epic => "epic",
            Self::Standalone => "standalone",
        })
    }
}

impl std::str::FromStr for Store {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "steam" => Ok(Self::Steam),
            "gog" => Ok(Self::Gog),
            "epic" => Ok(Self::Epic),
            "standalone" => Ok(Self::Standalone),
            other => Err(ParseError(format!("unknown store {other:?}"))),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ParseError(String);

/// Applicability of one save candidate. Absent fields mean "any".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// True when this condition-set is compatible with `os`/`store`.
    pub fn applies(&self, os: Platform, store: Store) -> bool {
        self.os.is_none_or(|candidate| candidate == os)
            && self.store.is_none_or(|candidate| candidate == store)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveCandidate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    pub dir: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detect {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam: Option<IdList>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gog: Option<IdList>,
}

impl Detect {
    pub fn is_empty(&self) -> bool {
        self.steam.as_ref().is_none_or(IdList::is_empty)
            && self.gog.as_ref().is_none_or(IdList::is_empty)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Game {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub info: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect: Option<Detect>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub executables: BTreeMap<Platform, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save: Vec<SaveCandidate>,
}

impl Game {
    pub fn steam_ids(&self) -> &[u64] {
        self.detect
            .as_ref()
            .and_then(|detect| detect.steam.as_ref())
            .map(|ids| ids.0.as_slice())
            .unwrap_or_default()
    }
    pub fn gog_ids(&self) -> &[u64] {
        self.detect
            .as_ref()
            .and_then(|detect| detect.gog.as_ref())
            .map(|ids| ids.0.as_slice())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub repo: String,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub schema: u32,
    pub source: Source,
    pub games: Vec<Game>,
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("invalid bundle JSON: {0}")]
    Json(String),
    #[error("unsupported bundle schema {found}; this build understands schema {supported}")]
    Schema { found: u32, supported: u32 },
    #[error("invalid bundle: {0}")]
    Invalid(String),
}

/// Placeholders the resolver understands. Builder output may only use these.
pub const PLACEHOLDERS: &[&str] = &[
    "INSTALL_DIR",
    "APPDATA",
    "LOCALAPPDATA",
    "LOCALLOW",
    "DOCUMENTS",
    "PUBLIC",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "WINDIR",
    "HOME",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "STORE_USER_ID",
    "STEAM_USERDATA",
];

impl Bundle {
    pub fn parse(json: &str) -> Result<Self, BundleError> {
        let bundle: Bundle =
            serde_json::from_str(json).map_err(|e| BundleError::Json(e.to_string()))?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn to_json_pretty(&self) -> Result<String, BundleError> {
        let mut text =
            serde_json::to_string_pretty(self).map_err(|e| BundleError::Json(e.to_string()))?;
        text.push('\n');
        Ok(text)
    }

    pub fn validate(&self) -> Result<(), BundleError> {
        if self.schema != SCHEMA {
            return Err(BundleError::Schema {
                found: self.schema,
                supported: SCHEMA,
            });
        }
        if self.source.repo.trim().is_empty() {
            return Err(invalid("source repo must not be empty"));
        }
        if self.source.revision.trim().is_empty() {
            return Err(invalid("source revision must not be empty"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for game in &self.games {
            if game.id.trim().is_empty() {
                return Err(invalid("game id must not be empty"));
            }
            if game.name.trim().is_empty() {
                return Err(invalid(format!("game {} has an empty name", game.id)));
            }
            if !seen.insert(game.id.as_str()) {
                return Err(invalid(format!("duplicate game id {}", game.id)));
            }
            for (platform, executables) in &game.executables {
                for executable in executables {
                    validate_relative(executable).map_err(|e| {
                        invalid(format!("game {} {platform} executable: {e}", game.id))
                    })?;
                }
            }
            for candidate in &game.save {
                validate_dir(&candidate.dir)
                    .map_err(|e| invalid(format!("game {} save dir: {e}", game.id)))?;
            }
        }
        Ok(())
    }

    pub fn game(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|game| game.id == id)
    }
}

fn invalid(message: impl Into<String>) -> BundleError {
    BundleError::Invalid(message.into())
}

fn validate_relative(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("path must not be empty".into());
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err(format!("path must be relative: {path:?}"));
    }
    if normalized.split('/').any(|segment| segment == "..") {
        return Err(format!("path must not traverse: {path:?}"));
    }
    if normalized.contains(['<', '>', '{', '}']) {
        return Err(format!("path must be resolved, not a template: {path:?}"));
    }
    Ok(())
}

/// A bundle dir is an absolute template: a known `{PLACEHOLDER}` root or a
/// literal absolute path, followed by relative segments.
fn validate_dir(dir: &str) -> Result<(), String> {
    if dir.contains(['<', '>']) {
        return Err(format!("dir must use {{{{...}}}} placeholders: {dir:?}"));
    }
    let normalized = dir.replace('\\', "/");
    if normalized.is_empty() {
        return Err("dir must not be empty".into());
    }
    for segment in normalized.split('/') {
        if segment == ".." {
            return Err(format!("dir must not traverse: {dir:?}"));
        }
        let mut rest = segment;
        while let Some(open) = rest.find('{') {
            let close = rest[open..]
                .find('}')
                .ok_or_else(|| format!("unclosed placeholder in {dir:?}"))?
                + open;
            let token = &rest[open + 1..close];
            if !PLACEHOLDERS.contains(&token) {
                return Err(format!("unknown placeholder {{{token}}} in {dir:?}"));
            }
            rest = &rest[close + 1..];
        }
        if rest.contains(['{', '}']) {
            return Err(format!("unbalanced placeholder in {dir:?}"));
        }
    }
    let rooted = normalized.starts_with('{');
    if !rooted && !is_absolute_path(&normalized) {
        return Err(format!(
            "dir must start with a placeholder root or be absolute: {dir:?}"
        ));
    }
    Ok(())
}

/// Host-independent absolute-path test (mirrors the resolver's check).
fn is_absolute_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.starts_with('/')
        || text.starts_with("\\\\")
        || (bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{
      "schema": 1,
      "source": { "repo": "mtkennerly/ludusavi-manifest", "revision": "abc" },
      "games": [
        {
          "id": "steam-1434950",
          "name": "HighFleet",
          "info": "Exit to main menu.",
          "detect": { "steam": 1434950, "gog": 1589167087 },
          "executables": { "windows": ["Highfleet.exe"] },
          "save": [
            { "when": { "os": "windows" }, "dir": "{INSTALL_DIR}/Saves" },
            { "when": { "os": "windows" }, "dir": "{INSTALL_DIR}/SavesSkirmish" },
            { "when": { "os": "windows" }, "dir": "{INSTALL_DIR}/Ships" }
          ]
        }
      ]
    }"#;

    #[test]
    fn example_round_trips_and_extras_serialize_as_arrays() {
        let bundle = Bundle::parse(EXAMPLE).unwrap();
        let game = bundle.game("steam-1434950").unwrap();
        assert_eq!(game.steam_ids(), &[1434950]);
        assert_eq!(game.gog_ids(), &[1589167087]);
        assert_eq!(game.save.len(), 3);

        let text = bundle.to_json_pretty().unwrap();
        let reparsed = Bundle::parse(&text).unwrap();
        assert_eq!(bundle, reparsed);
        assert!(text.contains("\"steam\": 1434950"));
    }

    #[test]
    fn extra_ids_round_trip_as_arrays() {
        let bundle = Bundle::parse(
            r#"{"schema":1,"source":{"repo":"r","revision":"v"},"games":[
                {"id":"steam-1","name":"G","detect":{"steam":[1,2]}}]}"#,
        )
        .unwrap();
        assert_eq!(bundle.games[0].steam_ids(), &[1, 2]);
        let text = bundle.to_json_pretty().unwrap();
        let reparsed = Bundle::parse(&text).unwrap();
        assert_eq!(reparsed.games[0].steam_ids(), &[1, 2]);
        assert!(text.contains('['));
    }

    #[test]
    fn rejects_unknown_schema_placeholders_and_duplicates() {
        let errors = [
            r#"{"schema":2,"source":{"repo":"r","revision":"v"},"games":[]}"#,
            r#"{"schema":1,"source":{"repo":"r","revision":"v"},"games":[
                {"id":"a","name":"G","save":[{"dir":"{UNKNOWN}/x"}]}]}"#,
            r#"{"schema":1,"source":{"repo":"r","revision":"v"},"games":[
                {"id":"a","name":"G","save":[{"dir":"relative/path"}]}]}"#,
            r#"{"schema":1,"source":{"repo":"r","revision":"v"},"games":[
                {"id":"a","name":"G"},{"id":"a","name":"H"}]}"#,
            r#"{"schema":1,"source":{"repo":"r","revision":"v"},"games":[
                {"id":"a","name":"G","executables":{"windows":["../escape.exe"]}}]}"#,
        ];
        for json in errors {
            assert!(Bundle::parse(json).is_err(), "accepted {json}");
        }
    }

    #[test]
    fn accepts_literal_absolute_save_dirs() {
        let bundle = Bundle::parse(
            r#"{"schema":1,"source":{"repo":"r","revision":"v"},"games":[
                {"id":"a","name":"G","save":[{"dir":"/var/games/nethack/save"}]}]}"#,
        )
        .unwrap();
        assert_eq!(bundle.games[0].save[0].dir, "/var/games/nethack/save");
    }
}
