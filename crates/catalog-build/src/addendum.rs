//! Addendum model. Same entry shape as the generated bundle, keyed by the
//! `Name` column in `games.csv`.

use savescummer_catalog::model::{Detect, Platform, SaveCandidate};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddendumEntry {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub detect: Option<Detect>,
    #[serde(default)]
    pub executables: BTreeMap<Platform, Vec<String>>,
    #[serde(default)]
    pub save: Vec<SaveCandidate>,
}

pub fn parse(text: &str) -> Result<BTreeMap<String, AddendumEntry>, String> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_yaml::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_example() {
        let entries = parse(
            r#"
Void War:
  detect:
    steam: 2853590
  executables:
    windows: ["Void War.exe"]
  save:
    - when: { os: windows }
      dir: "{APPDATA}/Void_War"
"#,
        )
        .unwrap();
        let entry = &entries["Void War"];
        assert_eq!(
            entry.detect.as_ref().unwrap().steam.as_ref().unwrap().0,
            vec![2853590]
        );
        assert_eq!(entry.executables[&Platform::Windows], vec!["Void War.exe"]);
        assert_eq!(entry.save[0].dir, "{APPDATA}/Void_War");
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(parse("Void War:\n  nonsense: true\n").is_err());
    }
}
