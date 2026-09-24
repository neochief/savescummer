//! Shared helpers: build from in-memory inputs with a lock that matches.

#![allow(dead_code)]

use savescummer_catalog::{Game, PathRule, Platform, Store, When};
use savescummer_catalog_build::manifest::{self, sha256_hex};
use savescummer_catalog_build::translate::{Translation, translate};
use savescummer_catalog_build::{Inputs, Issue, IssueKind, Options, Outcome, build_with};

pub const CSV_HEADER: &str = "\"Name\",\"Product fit\",\"Info\"\n";

/// A lock pinning exactly these manifest bytes.
pub fn lock_for(manifest: &str) -> String {
    format!("repo: test/manifest\nrevision: abc123\nsha256: {}\n", sha256_hex(manifest.as_bytes()))
}

/// `games.csv` text with one `Keep` row per name.
pub fn keep(names: &[&str]) -> String {
    let mut csv = CSV_HEADER.to_string();
    for name in names {
        csv.push_str(&format!("\"{name}\",\"Keep\",\"\"\n"));
    }
    csv
}

pub fn run(csv: &str, addendum: &str, manifest: &str) -> Outcome {
    run_with(csv, addendum, manifest, Options::default())
}

pub fn run_with(csv: &str, addendum: &str, manifest: &str, options: Options) -> Outcome {
    let lock = lock_for(manifest);
    build_with(&Inputs { games_csv: csv, addendum, manifest: manifest.as_bytes(), lock: &lock }, options)
}

/// Builds and returns the one game named `name`, failing on hard errors.
pub fn game(outcome: &Outcome, name: &str) -> Game {
    let bundle = outcome.bundle.as_ref().unwrap_or_else(|| panic!("build failed: {:#?}", outcome.report.errors));
    bundle.games.iter().find(|g| g.name == name).cloned().unwrap_or_else(|| panic!("{name} not built"))
}

/// Translates the one entry of a manifest snippet.
pub fn translate_one(manifest_text: &str) -> Translation {
    let manifest = manifest::parse(manifest_text.as_bytes()).expect("manifest parses");
    let (name, game) = manifest.iter().next().expect("one entry");
    translate(name, game)
}

pub fn rule(path: &str, os: Option<Platform>, store: Option<Store>) -> PathRule {
    PathRule { when: When { os, store }, path: path.to_string() }
}

pub fn any(path: &str) -> PathRule {
    rule(path, None, None)
}

pub fn win(path: &str) -> PathRule {
    rule(path, Some(Platform::Windows), None)
}

pub fn steam(path: &str) -> PathRule {
    rule(path, None, Some(Store::Steam))
}

pub fn issues_of(issues: &[Issue], kind: IssueKind) -> Vec<&Issue> {
    issues.iter().filter(|i| i.kind == kind).collect()
}
