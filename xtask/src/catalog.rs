//! `catalog`: regenerate `catalog/catalog.json` from the repo's fixed inputs
//! (PLAN-CATALOG.md 3.6), fetching the pinned manifest first when it's
//! missing or doesn't match `catalog/manifest.lock`.

use std::fs;
use std::io::Read;

use anyhow::{Context, bail};
use savescummer_catalog_build::{Lock, Options, Paths, RunResult, build_from_files, finish, manifest};

use crate::paths;

pub fn run(check: bool, strict: bool, options: Options) -> anyhow::Result<RunResult> {
    let root = paths::root();
    let paths = Paths {
        games: root.join("catalog").join("games.csv"),
        addendum: root.join("catalog").join("addendum.yaml"),
        manifest: paths::runtime().join("catalog").join("manifest.yaml"),
        lock: root.join("catalog").join("manifest.lock"),
    };
    ensure_manifest(&paths)?;
    let outcome = build_from_files(&paths, options)?;
    finish(
        &outcome,
        &root.join("catalog").join("catalog.json"),
        Some(&paths::build().join("catalog").join("build-report.json")),
        check,
        strict,
    )
}

/// Downloads the pinned manifest unless a copy with the locked hash is
/// already on disk. The builder verifies the hash again when it reads it.
fn ensure_manifest(paths: &Paths) -> anyhow::Result<()> {
    let lock_text = fs::read_to_string(&paths.lock).with_context(|| format!("reading {}", paths.lock.display()))?;
    let lock = Lock::parse(&lock_text).map_err(anyhow::Error::msg)?;
    if let Ok(bytes) = fs::read(&paths.manifest)
        && manifest::sha256_hex(&bytes).eq_ignore_ascii_case(lock.sha256.trim())
    {
        return Ok(());
    }
    let url = lock.manifest_url();
    println!("fetching {url}");
    let mut bytes = Vec::new();
    ureq::get(&url)
        .call()
        .with_context(|| format!("downloading {url}"))?
        .into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("downloading {url}"))?;
    let actual = manifest::sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(lock.sha256.trim()) {
        bail!("downloaded manifest sha256 is {actual}, but manifest.lock pins {}", lock.sha256);
    }
    if let Some(parent) = paths.manifest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.manifest, &bytes).with_context(|| format!("writing {}", paths.manifest.display()))?;
    println!("saved {} ({} bytes)", paths.manifest.display(), bytes.len());
    Ok(())
}
