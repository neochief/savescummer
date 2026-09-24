//! The slice of the Ludusavi manifest the builder reads (PLAN-CATALOG.md 3.1,
//! "Used fields"). Everything else in an entry (`cloud`, `registry`, `notes`,
//! `alias`, launch `arguments`/`workingDir`, other `id.*` keys) is ignored by
//! simply not being declared here.

use std::collections::BTreeMap;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The whole manifest, keyed by game name. A `BTreeMap` keeps every walk over
/// it (and over each entry's `files`/`launch`) in one fixed order, so the
/// output never depends on hash order.
pub type Manifest = BTreeMap<String, ManifestGame>;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestGame {
    #[serde(default)]
    pub files: Option<BTreeMap<String, FileEntry>>,
    #[serde(default)]
    pub install_dir: BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    pub launch: BTreeMap<String, Vec<LaunchEntry>>,
    #[serde(default)]
    pub steam: Option<StoreId>,
    #[serde(default)]
    pub gog: Option<StoreId>,
    #[serde(default)]
    pub id: Option<ExtraIds>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FileEntry {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub when: Vec<Condition>,
}

/// One `when` condition. A list of them means "any of".
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Condition {
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LaunchEntry {
    #[serde(default)]
    pub when: Vec<Condition>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StoreId {
    #[serde(default)]
    pub id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraIds {
    #[serde(default)]
    pub steam_extra: Vec<u64>,
    #[serde(default)]
    pub gog_extra: Vec<u64>,
}

/// Lowercase hex SHA-256 of the manifest bytes, compared with `manifest.lock`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Parses manifest text.
pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
    serde_yaml::from_slice(bytes).map_err(|e| format!("manifest: {e}"))
}
