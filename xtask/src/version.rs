//! The one version, in `Cargo.toml` → `[workspace.package] version`
//! (PLAN-BUILD.md VERSION). Only `cargo xtask release` changes it.

use std::fs;
use std::path::Path;

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

/// Writes the workspace and UI versions for a release.
pub fn set(version: &str) -> anyhow::Result<()> {
    let root = paths::root();
    let cargo = root.join("Cargo.toml");
    let tauri = root.join("apps/ui/src-tauri/tauri.conf.json");
    let package = root.join("apps/ui/package.json");

    // Prepare every file before writing any of them. The release command
    // restores all four version files if a later write or check fails.
    let text = fs::read_to_string(&cargo).with_context(|| format!("reading {}", cargo.display()))?;
    let mut doc: toml_edit::DocumentMut = text.parse().with_context(|| format!("parsing {}", cargo.display()))?;
    doc["workspace"]["package"]["version"] = toml_edit::value(version);
    let tauri_text = updated_json_version(&tauri, version)?;
    let package_text = updated_json_version(&package, version)?;

    for (path, text) in [(&cargo, doc.to_string()), (&tauri, tauri_text), (&package, package_text)] {
        fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn updated_json_version(path: &Path, version: &str) -> anyhow::Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    update_json_version(&text, version).with_context(|| format!("updating {}", path.display()))
}

fn update_json_version(text: &str, version: &str) -> anyhow::Result<String> {
    let mut json: serde_json::Value = serde_json::from_str(text)?;
    if !json.get("version").is_some_and(serde_json::Value::is_string) {
        bail!("JSON has no string version");
    }
    json["version"] = serde_json::Value::String(version.to_owned());
    Ok(serde_json::to_string_pretty(&json)? + "\n")
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

    #[test]
    fn ui_versions_match_the_workspace() {
        let version = current().unwrap();
        for path in ["apps/ui/src-tauri/tauri.conf.json", "apps/ui/package.json"] {
            let text = fs::read_to_string(paths::root().join(path)).unwrap();
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(json["version"], version, "{path}");
        }
    }

    #[test]
    fn updates_a_json_version_without_changing_other_fields() {
        let input = r#"{"name":"savescummer-ui","version":"0.2.0","private":true}"#;
        let updated: serde_json::Value = serde_json::from_str(&update_json_version(input, "0.3.0").unwrap()).unwrap();
        assert_eq!(updated["version"], "0.3.0");
        assert_eq!(updated["name"], "savescummer-ui");
        assert_eq!(updated["private"], true);
    }
}
