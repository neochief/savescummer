//! Parse the pinned Ludusavi manifest into the subset of fields the builder
//! uses. Unknown fields are ignored so upstream additions never break a build.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Entry {
    #[serde(default)]
    pub steam: Option<SteamField>,
    #[serde(default)]
    pub gog: Option<GogField>,
    #[serde(default)]
    pub id: Option<ExtraIds>,
    #[serde(default)]
    pub launch: Option<BTreeMap<String, Vec<LaunchItem>>>,
    #[serde(default)]
    pub files: Option<BTreeMap<String, FileEntry>>,
    /// Parsed for the deferred loose-install scanner work; the bundle has no
    /// field for it yet, so it is intentionally unused here.
    #[serde(default, rename = "installDir")]
    #[allow(dead_code)]
    pub install_dir: Option<BTreeMap<String, serde_yaml::Value>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SteamField {
    pub id: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GogField {
    pub id: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExtraIds {
    #[serde(default)]
    pub steam_extra: Vec<u64>,
    #[serde(default)]
    pub gog_extra: Vec<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LaunchItem {
    #[serde(default)]
    pub when: Option<Whens>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileEntry {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub when: Option<Whens>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Whens {
    One(Cond),
    Many(Vec<Cond>),
}

impl Whens {
    pub fn slice_or_default(whens: &Option<Whens>) -> &[Cond] {
        match whens {
            None => &[],
            Some(Whens::One(cond)) => std::slice::from_ref(cond),
            Some(Whens::Many(conds)) => conds,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Cond {
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
    /// Parsed but intentionally ignored (Section 3.1: `bit` is dropped).
    #[serde(default)]
    #[allow(dead_code)]
    pub bit: Option<u64>,
}

/// Parse a manifest document.
pub fn parse(text: &str) -> Result<BTreeMap<String, Entry>, String> {
    serde_yaml::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_used_subset_and_ignores_unknown_fields() {
        let text = r#"
HighFleet:
  cloud:
    steam: true
  files:
    "<base>/Config.ini":
      tags: [config]
      when:
        - os: windows
    "<base>/Saves":
      tags: [save]
      when:
        - os: windows
  gog:
    id: 1589167087
  installDir:
    HighFleet: {}
  launch:
    "<base>/Highfleet.exe":
      - when:
          - os: windows
            store: steam
  steam:
    id: 1434950
  notes: ignored
"#;
        let entries = parse(text).unwrap();
        let entry = &entries["HighFleet"];
        assert_eq!(entry.steam.as_ref().unwrap().id, 1434950);
        assert_eq!(entry.gog.as_ref().unwrap().id, 1589167087);
        assert_eq!(entry.install_dir.as_ref().unwrap().len(), 1);
        assert!(entry.files.as_ref().unwrap().contains_key("<base>/Saves"));
        let launch = &entry.launch.as_ref().unwrap()["<base>/Highfleet.exe"];
        assert_eq!(Whens::slice_or_default(&launch[0].when).len(), 1);
    }
}
