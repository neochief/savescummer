//! The human inputs (PLAN-CATALOG.md Section 2): `games.csv`, `addendum.yaml`
//! and `manifest.lock`, parsed into plain values. Parsing only; the rules that
//! combine them live in [`crate::build`].

use std::collections::BTreeMap;

use savescummer_catalog::{Detect, Executables, PathRule, Platform, Store, When};
use serde::Deserialize;

/// One row of `games.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRow {
    pub name: String,
    pub fit: Fit,
    /// Markdown instructions; `None` when the cell is empty.
    pub info: Option<String>,
    /// 1-based line of the row's record in the file, for messages.
    pub line: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Keep,
    Remove,
}

/// A `games.csv` problem that isn't about one game's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    pub line: u64,
    pub name: Option<String>,
    pub message: String,
}

/// Reads `games.csv`. Structural problems (missing columns, broken CSV) are a
/// single `Err`; per-row problems (unknown `Product fit`, empty name) are
/// returned next to the rows that did parse, so every one is reported.
pub fn parse_games_csv(text: &str) -> Result<(Vec<GameRow>, Vec<RowError>), String> {
    // Spreadsheet exports start with a BOM; it would otherwise stick to the
    // first header name.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new().has_headers(true).flexible(true).from_reader(text.as_bytes());
    let headers = reader.headers().map_err(|e| format!("games.csv: {e}"))?.clone();
    let column = |name: &str| headers.iter().position(|h| h.trim() == name);
    let name_col = column("Name").ok_or("games.csv: no `Name` column")?;
    let fit_col = column("Product fit").ok_or("games.csv: no `Product fit` column")?;
    let info_col = column("Info");

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| format!("games.csv: {e}"))?;
        let line = record.position().map(|p| p.line()).unwrap_or(0);
        let name = record.get(name_col).unwrap_or("").trim().to_string();
        let fit_text = record.get(fit_col).unwrap_or("").trim();
        if name.is_empty() {
            // A fully empty trailing line is harmless; anything else is a mistake.
            if record.iter().all(|cell| cell.trim().is_empty()) {
                continue;
            }
            errors.push(RowError { line, name: None, message: "empty `Name`".into() });
            continue;
        }
        let fit = match fit_text {
            "Keep" => Fit::Keep,
            "Remove" => Fit::Remove,
            other => {
                errors.push(RowError {
                    line,
                    name: Some(name),
                    message: format!("invalid `Product fit` {other:?} (expected `Keep` or `Remove`)"),
                });
                continue;
            }
        };
        let info =
            info_col.and_then(|c| record.get(c)).filter(|s| !s.trim().is_empty()).map(|s| s.replace("\r\n", "\n"));
        rows.push(GameRow { name, fit, info, line });
    }
    Ok((rows, errors))
}

/// `catalog/manifest.lock`: the pinned upstream manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    pub repo: String,
    pub revision: String,
    pub sha256: String,
}

impl Lock {
    pub fn parse(text: &str) -> Result<Lock, String> {
        serde_yaml::from_str(text).map_err(|e| format!("manifest.lock: {e}"))
    }

    /// Where the pinned manifest is downloaded from.
    pub fn manifest_url(&self) -> String {
        format!("https://raw.githubusercontent.com/{}/{}/data/manifest.yaml", self.repo, self.revision)
    }
}

/// One `addendum.yaml` entry: the bundle entry's shape (Section 3.4) minus
/// `name`/`info`, plus `override`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddendumEntry {
    /// Used only when the game has no store id (Section 3.3).
    pub id: Option<String>,
    pub fields: Fields,
    /// Fields that replace the manifest's outright (Section 3.2).
    pub overrides: Fields,
}

/// The entry fields an addendum may give. `None` means "not given", which is
/// different from an explicitly empty list only for `override` (an empty
/// `exclude:` under `override` clears the manifest's excludes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub detect: Option<Detect>,
    pub install_dirs: Option<Vec<String>>,
    pub executables: Option<Executables>,
    pub save: Option<Vec<PathRule>>,
    pub exclude: Option<Vec<PathRule>>,
}

impl Fields {
    /// The names of the fields present, in a fixed order, for the report.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.detect.is_some() {
            names.push("detect");
        }
        if self.install_dirs.is_some() {
            names.push("installDirs");
        }
        if self.executables.is_some() {
            names.push("executables");
        }
        if self.save.is_some() {
            names.push("save");
        }
        if self.exclude.is_some() {
            names.push("exclude");
        }
        names
    }

    pub fn is_empty(&self) -> bool {
        self.names().is_empty()
    }
}

/// Parses `addendum.yaml`, keyed by game name. Unknown keys anywhere are
/// schema errors, so a typo (or the retired `dir:` spelling) can't silently
/// drop a fix.
pub fn parse_addendum(text: &str) -> Result<BTreeMap<String, AddendumEntry>, String> {
    let raw: Option<BTreeMap<String, RawEntry>> =
        serde_yaml::from_str(text).map_err(|e| format!("addendum.yaml: {e}"))?;
    Ok(raw
        .unwrap_or_default()
        .into_iter()
        .map(|(name, raw)| {
            let entry = AddendumEntry {
                id: raw.id,
                fields: RawFields {
                    detect: raw.detect,
                    install_dirs: raw.install_dirs,
                    executables: raw.executables,
                    save: raw.save,
                    exclude: raw.exclude,
                }
                .into_fields(),
                overrides: raw.overrides.map(RawFields::into_fields).unwrap_or_default(),
            };
            (name, entry)
        })
        .collect())
}

// Mirrors of the bundle types with `deny_unknown_fields`: the bundle model
// itself is lenient on read, the addendum must not be.

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawEntry {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    detect: Option<RawDetect>,
    #[serde(default)]
    install_dirs: Option<Vec<String>>,
    #[serde(default)]
    executables: Option<RawExecutables>,
    #[serde(default)]
    save: Option<Vec<RawRule>>,
    #[serde(default)]
    exclude: Option<Vec<RawRule>>,
    #[serde(default, rename = "override")]
    overrides: Option<RawFields>,
}

/// The fields `override` may replace. `id` is not among them: identity comes
/// from the store ids (Section 3.3).
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawFields {
    #[serde(default)]
    detect: Option<RawDetect>,
    #[serde(default)]
    install_dirs: Option<Vec<String>>,
    #[serde(default)]
    executables: Option<RawExecutables>,
    #[serde(default)]
    save: Option<Vec<RawRule>>,
    #[serde(default)]
    exclude: Option<Vec<RawRule>>,
}

impl RawFields {
    fn into_fields(self) -> Fields {
        Fields {
            detect: self.detect.map(|d| Detect {
                steam: d.steam.into_vec(),
                gog: d.gog.into_vec(),
                uninstall: d.uninstall,
            }),
            install_dirs: self.install_dirs,
            executables: self.executables.map(|e| Executables { windows: e.windows, macos: e.macos, linux: e.linux }),
            save: self.save.map(|rules| rules.into_iter().map(RawRule::into_rule).collect()),
            exclude: self.exclude.map(|rules| rules.into_iter().map(RawRule::into_rule).collect()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDetect {
    #[serde(default)]
    steam: OneOrMany,
    #[serde(default)]
    gog: OneOrMany,
    #[serde(default)]
    uninstall: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(untagged)]
enum OneOrMany {
    #[default]
    None,
    One(u64),
    Many(Vec<u64>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<u64> {
        match self {
            OneOrMany::None => Vec::new(),
            OneOrMany::One(id) => vec![id],
            OneOrMany::Many(ids) => ids,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExecutables {
    #[serde(default)]
    windows: Vec<String>,
    #[serde(default)]
    macos: Vec<String>,
    #[serde(default)]
    linux: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    #[serde(default)]
    when: RawWhen,
    path: String,
}

impl RawRule {
    fn into_rule(self) -> PathRule {
        PathRule { when: When { os: self.when.os, store: self.when.store }, path: self.path }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawWhen {
    #[serde(default)]
    os: Option<Platform>,
    #[serde(default)]
    store: Option<Store>,
}
