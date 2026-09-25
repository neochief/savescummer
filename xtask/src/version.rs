//! The one version, in `Cargo.toml` → `[workspace.package] version`
//! (PLAN-BUILD.md VERSION). Only `cargo xtask release` changes it.

use std::fs;

use anyhow::{Context, bail};

use crate::paths;

/// The current version, parsed with the `toml` crate (never pattern-matched).
pub fn current() -> anyhow::Result<String> {
    let path = paths::root().join("Cargo.toml");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let table: toml::Table = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    table
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("Cargo.toml has no [workspace.package] version")
}

/// Writes `version` into `Cargo.toml`, keeping its formatting.
pub fn set(version: &str) -> anyhow::Result<()> {
    let path = paths::root().join("Cargo.toml");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text.parse().with_context(|| format!("parsing {}", path.display()))?;
    doc["workspace"]["package"]["version"] = toml_edit::value(version);
    fs::write(&path, doc.to_string()).with_context(|| format!("writing {}", path.display()))
}

/// Accepts `1.2.3` or `v1.2.3` and returns `1.2.3`.
pub fn parse(input: &str) -> anyhow::Result<String> {
    let version = input.strip_prefix('v').unwrap_or(input);
    let parts: Vec<&str> = version.split('.').collect();
    let numeric =
        |p: &&str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (p.len() == 1 || !p.starts_with('0'));
    if parts.len() != 3 || !parts.iter().all(numeric) {
        bail!("`{input}` isn't a version: expected three numbers like 1.2.3");
    }
    Ok(version.to_string())
}

pub fn tag(version: &str) -> String {
    format!("v{version}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse("1.2.3").unwrap(), "1.2.3");
        assert_eq!(parse("v0.10.0").unwrap(), "0.10.0");
        for bad in ["1.2", "1.2.3.4", "1.x.3", "01.2.3", "", "v", "1.2.3-beta"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reads_the_workspace_version() {
        assert_eq!(current().unwrap(), env!("CARGO_PKG_VERSION"));
    }
}
