//! The build report (PLAN-CATALOG.md 3.5): hard errors, warnings, and the
//! items listed for review. Written as JSON next to the build and printed as
//! a readable summary.

use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub stats: Stats,
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
    pub review: Review,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub keep_rows: usize,
    pub remove_rows: usize,
    pub ignored_rows: usize,
    pub games_built: usize,
    pub from_manifest: usize,
    pub from_addendum_only: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub kind: IssueKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    pub message: String,
}

impl Issue {
    pub fn new(kind: IssueKind, game: Option<&str>, message: impl Into<String>) -> Issue {
        Issue { kind, game: game.map(str::to_string), message: message.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueKind {
    // Hard errors.
    /// The manifest's bytes don't hash to `manifest.lock`'s `sha256`.
    ManifestHash,
    /// An input file can't be read or parsed at all.
    InvalidInput,
    /// A `Keep` name isn't in the manifest and has no addendum entry.
    UnresolvedName,
    DuplicateName,
    InvalidProductFit,
    /// `addendum.yaml` doesn't match the entry shape.
    AddendumSchema,
    /// A `Keep` game ended up with no save target.
    NoUsableSave,
    /// The finished entry fails the bundle validator.
    InvalidEntry,
    // Warnings.
    AddendumShadowed,
    AddendumWithoutKeepRow,
    DroppedTarget,
    /// An `Ignored` row: left out until someone addresses it.
    IgnoredGame,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    /// Every addendum `override`, so fixes upstream has since made can go.
    pub overrides: Vec<OverrideItem>,
    /// Every game whose save set has more than one target.
    pub multi_target: Vec<MultiTargetItem>,
    /// Every save target that is a whole folder tagged both config and save.
    pub config_save_folders: Vec<FolderItem>,
    /// `launch` entries that didn't become executables (documents, paths
    /// outside the install folder). Informational.
    pub dropped_launch: Vec<FolderItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverrideItem {
    pub game: String,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MultiTargetItem {
    pub game: String,
    pub targets: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderItem {
    pub game: String,
    pub path: String,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Pretty JSON with a trailing newline.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("report serializes");
        text.push('\n');
        text
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.game {
            Some(game) => write!(f, "[{}] {game}: {}", kind_name(self.kind), self.message),
            None => write!(f, "[{}] {}", kind_name(self.kind), self.message),
        }
    }
}

fn kind_name(kind: IssueKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// The readable summary `catalog-gen` and `cargo xtask catalog` print.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = &self.stats;
        writeln!(
            f,
            "catalog: {} games built ({} from the manifest, {} addendum-only) of {} Keep rows; \
             {} Remove rows skipped; {} Ignored rows need attention",
            s.games_built, s.from_manifest, s.from_addendum_only, s.keep_rows, s.remove_rows, s.ignored_rows
        )?;
        let r = &self.review;
        writeln!(
            f,
            "review: {} overrides, {} multi-target games, {} config+save folders, {} dropped launch entries",
            r.overrides.len(),
            r.multi_target.len(),
            r.config_save_folders.len(),
            r.dropped_launch.len()
        )?;
        for item in &r.overrides {
            writeln!(f, "  override  {}: {}", item.game, item.fields.join(", "))?;
        }
        for item in &r.config_save_folders {
            writeln!(f, "  folder    {}: {}", item.game, item.path)?;
        }
        if !self.warnings.is_empty() {
            writeln!(f, "{} warnings:", self.warnings.len())?;
            for issue in &self.warnings {
                writeln!(f, "  {issue}")?;
            }
        }
        if !self.errors.is_empty() {
            writeln!(f, "{} errors:", self.errors.len())?;
            for issue in &self.errors {
                writeln!(f, "  {issue}")?;
            }
        }
        Ok(())
    }
}
