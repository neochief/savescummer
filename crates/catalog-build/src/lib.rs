//! Catalog builder: deterministic translation of `games.csv` + `addendum.yaml`
//! against a pinned Ludusavi manifest into `catalog.json` plus a report.
//!
//! Offline and host-free; depends on `savescummer-catalog` for the bundle model
//! and validator so builder output and resolver input cannot drift.

mod addendum;
mod games;
mod identity;
mod manifest;
mod report;
mod translate;

pub use games::{GameRow, ProductFit};
pub use report::BuildReport;

use savescummer_catalog::model::{
    Bundle, Detect, Game, Platform, SCHEMA, SaveCandidate, Source, When,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// `catalog/manifest.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    pub repo: String,
    pub revision: String,
    pub sha256: String,
}

impl Lock {
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|e| format!("manifest.lock: {e}"))?;
        let map = value
            .as_mapping()
            .ok_or_else(|| "manifest.lock must be a mapping".to_string())?;
        let get = |key: &str| {
            map.get(serde_yaml::Value::String(key.into()))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        Ok(Self {
            repo: get("repo").ok_or("manifest.lock is missing repo")?,
            revision: get("revision").ok_or("manifest.lock is missing revision")?,
            sha256: get("sha256")
                .ok_or("manifest.lock is missing sha256")?
                .to_lowercase(),
        })
    }
}

pub struct Inputs<'a> {
    pub games_csv: &'a str,
    pub addendum_yaml: &'a str,
    pub manifest_yaml: &'a str,
    pub lock: &'a Lock,
}

#[derive(Debug)]
pub struct BuildOutput {
    pub bundle: Bundle,
    pub report: BuildReport,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("{0}")]
    Input(String),
    #[error("manifest sha256 mismatch: lock expects {expected}, input is {actual}")]
    Hash { expected: String, actual: String },
    #[error("catalog build failed with {} error(s)", .report.errors.len())]
    Failed { report: BuildReport },
}

pub fn build(inputs: &Inputs) -> Result<BuildOutput, BuildError> {
    build_inner(inputs, false)
}

/// Bootstrap/inspection mode: games that cannot be built are reported as
/// warnings and omitted from the bundle instead of stopping the build. The
/// strict [`build`] remains the contract; this exists because some current
/// `Keep` rows still lack a verified save directory.
pub fn build_lenient(inputs: &Inputs) -> Result<BuildOutput, BuildError> {
    build_inner(inputs, true)
}

fn build_inner(inputs: &Inputs, lenient: bool) -> Result<BuildOutput, BuildError> {
    let actual = sha256_hex(inputs.manifest_yaml.as_bytes());
    if actual != inputs.lock.sha256 {
        return Err(BuildError::Hash {
            expected: inputs.lock.sha256.clone(),
            actual,
        });
    }
    let rows = games::parse(inputs.games_csv).map_err(BuildError::Input)?;
    let addenda = addendum::parse(inputs.addendum_yaml).map_err(BuildError::Input)?;
    let manifest = manifest::parse(inputs.manifest_yaml).map_err(BuildError::Input)?;

    let mut report = BuildReport::default();
    let keep: BTreeSet<String> = rows
        .iter()
        .filter(|row| row.fit() == Some(ProductFit::Keep))
        .map(|row| row.name.to_lowercase())
        .collect();
    for name in addenda.keys() {
        if !keep.contains(&name.to_lowercase()) {
            report.warn(format!(
                "addendum entry {name:?} has no matching Keep row; ignored"
            ));
        }
    }

    let mut games = Vec::new();
    let mut known_ids = BTreeSet::new();
    for row in rows
        .iter()
        .filter(|row| row.fit() == Some(ProductFit::Keep))
    {
        match build_game(row, &manifest, &addenda, &mut report) {
            Ok(game) => {
                if !known_ids.insert(game.id.clone()) {
                    report.error(format!("{}: duplicate catalog id {}", row.name, game.id));
                } else {
                    games.push(game);
                }
            }
            Err(message) => {
                if lenient {
                    report.warn(format!("{}: unresolved ({message})", row.name));
                } else {
                    report.error(format!("{}: {message}", row.name));
                }
            }
        }
    }
    games.sort_by(|a, b| a.id.cmp(&b.id));
    let bundle = Bundle {
        schema: SCHEMA,
        source: Source {
            repo: inputs.lock.repo.clone(),
            revision: inputs.lock.revision.clone(),
        },
        games,
    };
    if !report.is_clean() {
        return Err(BuildError::Failed { report });
    }
    bundle
        .validate()
        .map_err(|e| BuildError::Input(e.to_string()))?;
    report.games = bundle.games.len();
    Ok(BuildOutput { bundle, report })
}

fn build_game(
    row: &GameRow,
    manifest: &BTreeMap<String, manifest::Entry>,
    addenda: &BTreeMap<String, addendum::AddendumEntry>,
    report: &mut BuildReport,
) -> Result<Game, String> {
    let addendum = addenda.get(&row.name);
    let (mut detect, mut executables, save, drops) = if let Some(entry) = manifest.get(&row.name) {
        if addendum.is_some_and(addendum_has_data) {
            report.warn(format!(
                "addendum entry {:?} is shadowed by the manifest entry; delete it",
                row.name
            ));
        }
        let detect = translate::detect(entry);
        let (executables, mut warnings) = translate::executables(entry);
        report.warnings.append(&mut warnings);
        let (save, drops) = translate::save(entry);
        (detect, executables, save, drops)
    } else {
        let Some(addendum) = addendum else {
            return Err("name unresolved in the manifest and no addendum entry".into());
        };
        let detect = addendum.detect.clone().unwrap_or_default();
        (
            detect,
            addendum.executables.clone(),
            addendum.save.clone(),
            Vec::new(),
        )
    };

    if let Some(addendum) = addendum {
        overlay(&mut detect, &mut executables, addendum);
    }

    let save = overlay_save(save, addendum);

    if save.is_empty() {
        let reasons: Vec<&str> = drops.iter().map(|reason| reason.describe()).collect();
        let detail = if reasons.is_empty() {
            "no save candidates".to_string()
        } else {
            reasons.join(", ")
        };
        return Err(format!("no usable save directory ({detail})"));
    }
    warn_if_ambiguous(row, &save, report);

    let id = identity::derive(&row.name, &detect, addendum.and_then(|a| a.id.as_deref()));
    Ok(Game {
        id,
        name: row.name.clone(),
        info: row.info.clone(),
        detect: (!detect.is_empty()).then_some(detect),
        executables: normalize_executables(executables, &row.name, report)?,
        save,
    })
}

fn addendum_has_data(entry: &addendum::AddendumEntry) -> bool {
    entry.id.is_some()
        || entry
            .detect
            .as_ref()
            .is_some_and(|detect| !detect.is_empty())
        || !entry.executables.is_empty()
        || !entry.save.is_empty()
}

/// Addendum overlay (Section 3.2). `detect` and `executables` are filled here;
/// `save` is handled by [`overlay_save`].
fn overlay(
    detect: &mut Detect,
    executables: &mut BTreeMap<Platform, Vec<String>>,
    addendum: &addendum::AddendumEntry,
) {
    if let Some(extra) = &addendum.detect {
        if detect.steam.is_none() {
            detect.steam = extra.steam.clone();
        }
        if detect.gog.is_none() {
            detect.gog = extra.gog.clone();
        }
    }
    for (platform, values) in &addendum.executables {
        let bucket = executables.entry(*platform).or_default();
        if bucket.is_empty() {
            bucket.extend(values.clone());
        }
    }
}

/// Manifest save candidates replace the addendum for every OS the manifest
/// covers; the addendum keeps only OSes the manifest produced nothing for.
fn overlay_save(
    manifest_save: Vec<SaveCandidate>,
    addendum: Option<&addendum::AddendumEntry>,
) -> Vec<SaveCandidate> {
    if manifest_save.is_empty() {
        return addendum.map(|a| a.save.clone()).unwrap_or_default();
    }
    let Some(addendum) = addendum else {
        return manifest_save;
    };
    let mut result = manifest_save;
    for candidate in &addendum.save {
        match candidate.when.and_then(|when| when.os) {
            Some(os) => {
                if !covers_os(&result, os) {
                    result.push(candidate.clone());
                }
            }
            None => {
                for os in [Platform::Windows, Platform::Macos, Platform::Linux] {
                    if !covers_os(&result, os) {
                        let mut candidate = candidate.clone();
                        candidate.when = Some(When {
                            os: Some(os),
                            store: candidate.when.and_then(|when| when.store),
                        });
                        result.push(candidate);
                    }
                }
            }
        }
    }
    result
}

fn covers_os(save: &[SaveCandidate], os: Platform) -> bool {
    save.iter().any(|candidate| {
        candidate
            .when
            .map(|when| when.os.is_none() || when.os == Some(os))
            .unwrap_or(true)
    })
}

fn normalize_executables(
    executables: BTreeMap<Platform, Vec<String>>,
    name: &str,
    report: &mut BuildReport,
) -> Result<BTreeMap<Platform, Vec<String>>, String> {
    let mut normalized: BTreeMap<Platform, Vec<String>> = BTreeMap::new();
    for (platform, values) in executables {
        let mut set = BTreeSet::new();
        for value in values {
            if value.trim().is_empty()
                || value.starts_with('/')
                || value.contains(':')
                || value.split('/').any(|segment| segment == "..")
            {
                report.warn(format!(
                    "{name}: ignoring invalid {platform} executable {value:?}"
                ));
                continue;
            }
            set.insert(value);
        }
        if !set.is_empty() {
            normalized.insert(platform, set.into_iter().collect());
        }
    }
    Ok(normalized)
}

fn warn_if_ambiguous(row: &GameRow, save: &[SaveCandidate], report: &mut BuildReport) {
    const EXACT: &[&str] = &[
        "save",
        "saves",
        "savegame",
        "savegames",
        "saved games",
        "save data",
    ];
    let has_exact = save.iter().any(|candidate| {
        candidate
            .dir
            .rsplit('/')
            .next()
            .is_some_and(|name| EXACT.iter().any(|exact| name.eq_ignore_ascii_case(exact)))
    });
    if save.len() > 1 && !has_exact {
        report.warn(format!(
            "{}: {} save candidates with no exact-name signal; runtime activity decides",
            row.name,
            save.len()
        ));
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Convenience for tests and the CLI: an empty addendum.
pub const EMPTY_ADDENDUM: &str = "{}\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(revision: &str, manifest: &str) -> Lock {
        Lock {
            repo: "mtkennerly/ludusavi-manifest".into(),
            revision: revision.into(),
            sha256: sha256_hex(manifest.as_bytes()),
        }
    }

    const HEADER: &str = "\"Name\",\"Extra1\",\"Extra2\",\"Info\",\"Product fit\",\"Extra3\"\n";

    #[test]
    fn highfleet_worked_example_matches_the_bundle_entry() {
        let manifest = r#"
HighFleet:
  files:
    "<base>/Config.ini":
      tags: [config]
      when:
        - os: windows
    "<base>/Saves":
      tags: [save]
      when:
        - os: windows
    "<base>/SavesSkirmish":
      tags: [save]
      when:
        - os: windows
    "<base>/Ships":
      tags: [save]
      when:
        - os: windows
    "<root>/steamapps/common/HighFleet/Config.ini":
      tags: [config]
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Saves":
      tags: [save]
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/SavesSkirmish":
      tags: [save]
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Ships":
      tags: [save]
      when:
        - store: steam
  gog:
    id: 1589167087
  launch:
    "<base>/Highfleet.exe":
      - when:
          - os: windows
            store: steam
  steam:
    id: 1434950
"#;
        let games_csv = format!(
            "{HEADER}\"HighFleet\",\"yes\",\"cat\",\"Exit to main menu before saving.\",\"Keep\",\"\"\n"
        );
        let lock = lock("abc", manifest);
        let output = build(&Inputs {
            games_csv: &games_csv,
            addendum_yaml: EMPTY_ADDENDUM,
            manifest_yaml: manifest,
            lock: &lock,
        })
        .unwrap();
        assert_eq!(output.bundle.games.len(), 1);
        let game = &output.bundle.games[0];
        assert_eq!(game.id, "steam-1434950");
        assert_eq!(game.name, "HighFleet");
        assert_eq!(game.info, "Exit to main menu before saving.");
        assert_eq!(game.executables[&Platform::Windows], vec!["Highfleet.exe"]);
        assert_eq!(
            game.save.iter().map(|c| c.dir.as_str()).collect::<Vec<_>>(),
            vec![
                "{INSTALL_DIR}/Saves",
                "{INSTALL_DIR}/SavesSkirmish",
                "{INSTALL_DIR}/Ships"
            ]
        );
        assert_eq!(output.bundle.source.revision, "abc");
    }

    #[test]
    fn addendum_fills_a_manifest_gap_and_info_comes_from_games_csv() {
        let manifest = "---\n{}";
        let addendum = r#"
Void War:
  detect:
    steam: 2853590
  executables:
    windows: ["Void War.exe"]
  save:
    - when: { os: windows }
      dir: "{APPDATA}/Void_War"
"#;
        let games_csv =
            format!("{HEADER}\"Void War\",\"yes\",\"cat\",\"Games info.\",\"Keep\",\"\"\n");
        let lock = lock("abc", manifest);
        let output = build(&Inputs {
            games_csv: &games_csv,
            addendum_yaml: addendum,
            manifest_yaml: manifest,
            lock: &lock,
        })
        .unwrap();
        let game = &output.bundle.games[0];
        assert_eq!(game.id, "steam-2853590");
        assert_eq!(game.info, "Games info.");
        assert_eq!(game.save[0].dir, "{APPDATA}/Void_War");
    }

    #[test]
    fn manifest_precedence_shadows_the_addendum_with_a_warning() {
        let manifest = r#"
Void War:
  steam:
    id: 2853590
  files:
    "<base>/saves":
      tags: [save]
      when:
        - os: windows
"#;
        let addendum = r#"
Void War:
  save:
    - when: { os: windows }
      dir: "{APPDATA}/Void_War"
"#;
        let games_csv = format!("{HEADER}\"Void War\",\"yes\",\"cat\",\"Info\",\"Keep\",\"\"\n");
        let lock = lock("abc", manifest);
        let output = build(&Inputs {
            games_csv: &games_csv,
            addendum_yaml: addendum,
            manifest_yaml: manifest,
            lock: &lock,
        })
        .unwrap();
        assert_eq!(output.bundle.games[0].save[0].dir, "{INSTALL_DIR}/saves");
        assert!(
            output
                .report
                .warnings
                .iter()
                .any(|warning| warning.contains("shadowed")),
            "{:?}",
            output.report.warnings
        );
    }

    #[test]
    fn hard_failures_stop_the_build() {
        let manifest = "---\n{}";
        let games_csv =
            format!("{HEADER}\"Missing Game\",\"yes\",\"cat\",\"Info\",\"Keep\",\"\"\n");
        let lock = lock("abc", manifest);
        let error = build(&Inputs {
            games_csv: &games_csv,
            addendum_yaml: EMPTY_ADDENDUM,
            manifest_yaml: manifest,
            lock: &lock,
        })
        .unwrap_err();
        match error {
            BuildError::Failed { report } => {
                assert!(
                    report
                        .errors
                        .iter()
                        .any(|error| error.contains("no addendum entry")),
                    "{:?}",
                    report.errors
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hash_mismatch_is_rejected() {
        let manifest = "---\n{}";
        let mut lock = lock("abc", manifest);
        lock.sha256 = "00".repeat(32);
        let error = build(&Inputs {
            games_csv: &format!("{HEADER}\"A\",\"\",\"\",\"\",\"Keep\",\"\"\n"),
            addendum_yaml: EMPTY_ADDENDUM,
            manifest_yaml: manifest,
            lock: &lock,
        })
        .unwrap_err();
        assert!(matches!(error, BuildError::Hash { .. }));
    }

    #[test]
    fn byte_identical_regeneration() {
        let manifest = "---\n{}";
        let addendum = r#"
Void War:
  detect: { steam: 2853590 }
  executables: { windows: ["Void War.exe"] }
  save:
    - when: { os: windows }
      dir: "{APPDATA}/Void_War"
"#;
        let games_csv = format!("{HEADER}\"Void War\",\"yes\",\"cat\",\"Info\",\"Keep\",\"\"\n");
        let lock = lock("abc", manifest);
        let build_once = || {
            let output = build(&Inputs {
                games_csv: &games_csv,
                addendum_yaml: addendum,
                manifest_yaml: manifest,
                lock: &lock,
            })
            .unwrap();
            output.bundle.to_json_pretty().unwrap()
        };
        assert_eq!(build_once(), build_once());
    }

    #[test]
    fn warnings_cover_orphan_addendum_entries() {
        let manifest = r#"
One Step from Eden:
  steam:
    id: 960690
  files:
    "<base>/save":
      tags: [save]
      when:
        - os: windows
"#;
        let addendum = r#"
Ghost:
  save:
    - dir: "{APPDATA}/Ghost"
"#;
        let games_csv =
            format!("{HEADER}\"One Step from Eden\",\"yes\",\"cat\",\"Info\",\"Keep\",\"\"\n");
        let lock = lock("abc", manifest);
        let output = build(&Inputs {
            games_csv: &games_csv,
            addendum_yaml: addendum,
            manifest_yaml: manifest,
            lock: &lock,
        })
        .unwrap();
        assert!(
            output
                .report
                .warnings
                .iter()
                .any(|warning| warning.contains("no matching Keep row")),
            "{:?}",
            output.report.warnings
        );
    }
}
