//! The catalog builder (PLAN-CATALOG.md Section 3): turns `games.csv`,
//! `addendum.yaml` and the pinned Ludusavi manifest into `catalog.json` plus
//! a build report. Offline and deterministic: the same inputs always give
//! byte-identical output.
//!
//! The bundle model and its validator come from `savescummer-catalog`, so
//! what this writes is exactly what the host reads.

pub mod build;
pub mod inputs;
pub mod manifest;
pub mod report;
pub mod translate;

use std::fs;
use std::path::{Path, PathBuf};

pub use build::{Inputs, Options, Outcome, build, build_with};
pub use inputs::Lock;
pub use report::{Issue, IssueKind, Report};

/// Where the builder's files are.
#[derive(Debug, Clone)]
pub struct Paths {
    pub games: PathBuf,
    pub addendum: PathBuf,
    pub manifest: PathBuf,
    pub lock: PathBuf,
}

/// Reads the input files and runs [`build_with`].
pub fn build_from_files(paths: &Paths, options: Options) -> anyhow::Result<Outcome> {
    let read = |path: &Path| fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()));
    let games_csv = read(&paths.games)?;
    let addendum = read(&paths.addendum)?;
    let lock = read(&paths.lock)?;
    let manifest =
        fs::read(&paths.manifest).map_err(|e| anyhow::anyhow!("reading {}: {e}", paths.manifest.display()))?;
    Ok(build_with(&Inputs { games_csv: &games_csv, addendum: &addendum, manifest: &manifest, lock: &lock }, options))
}

/// Whether the file at `path` already holds `expected`. Line endings are
/// compared loosely: a Windows checkout with `core.autocrlf` turns the
/// committed LF bundle into CRLF, which is the same bundle.
pub fn file_matches(path: &Path, expected: &str) -> bool {
    match fs::read_to_string(path) {
        Ok(actual) => actual.replace("\r\n", "\n") == expected,
        Err(_) => false,
    }
}

/// Writes `text` to `path`, creating parent folders.
pub fn write_file(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text).map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))
}

/// What a finished run did, for the two front ends to turn into an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    Ok,
    /// Hard errors: no bundle.
    Failed,
    /// `--check`: the committed bundle differs from the regenerated one.
    Stale,
    /// `--strict`: built, but with warnings.
    Warnings,
}

/// The shared tail of `catalog-gen` and `cargo xtask catalog`: print the
/// report, write it, then write or check the bundle.
pub fn finish(
    outcome: &Outcome,
    out: &Path,
    report_path: Option<&Path>,
    check: bool,
    strict: bool,
) -> anyhow::Result<RunResult> {
    print!("{}", outcome.report);
    if let Some(report_path) = report_path {
        write_file(report_path, &outcome.report.to_json())?;
        println!("report: {}", report_path.display());
    }
    let Some(bundle) = &outcome.bundle else {
        eprintln!("catalog build failed with {} errors", outcome.report.errors.len());
        return Ok(RunResult::Failed);
    };
    let text = bundle.to_json();
    // Belt and braces: what we write must load.
    savescummer_catalog::Bundle::parse(&text).map_err(|e| anyhow::anyhow!("generated bundle doesn't validate: {e}"))?;
    if check {
        if !file_matches(out, &text) {
            eprintln!("{} is out of date; run `cargo xtask catalog` and commit the result", out.display());
            return Ok(RunResult::Stale);
        }
        println!("{} is up to date", out.display());
    } else {
        write_file(out, &text)?;
        println!("wrote {}", out.display());
    }
    if strict && !outcome.report.warnings.is_empty() {
        eprintln!("--strict: {} warnings", outcome.report.warnings.len());
        return Ok(RunResult::Warnings);
    }
    Ok(RunResult::Ok)
}
