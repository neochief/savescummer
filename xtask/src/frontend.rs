//! The UI frontend layer: Tauri on Windows and macOS. The earlier Qt build
//! remains available on platforms with a Qt project.
//! Nothing outside this module (and the platform steps that deploy its
//! runtime) assumes either.
//!
//! The frontend plugs in through:
//!
//! - a setup command for any SDK it needs (`setup qt` on Qt platforms)
//! - a build step producing the `SaveScummer.UI` executable and its runtime files
//! - a test step that runs headless against the freshly built host
//! - its license texts, shipped in every package
//!
//! Windows and macOS require the Tauri UI. Linux still allows the frontend to
//! be absent while its UI packaging is implemented.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::paths::{self, Mode};
use crate::{cache, cmd, pins, platform};

/// A built UI, ready to be packaged.
pub struct Ui {
    /// Files staged for packaging: `bin/SaveScummer.UI`, and on Linux the
    /// `AppDir` of Tauri's AppImage bundling.
    pub install: PathBuf,
}

pub fn source() -> PathBuf {
    paths::root().join("apps").join("ui")
}

/// Whether the platform's UI source is present.
pub fn present() -> bool {
    source().join("package.json").is_file() && source().join("src-tauri").join("Cargo.toml").is_file()
}

/// License texts for a frontend with separately shipped runtime libraries.
pub fn licenses() -> Option<PathBuf> {
    None
}

pub fn version() -> &'static str {
    "Tauri 2"
}

/// `.runtime/Qt/<version>/<kit>/`, installed by `setup qt`.
pub fn kit() -> PathBuf {
    paths::runtime().join("Qt").join(pins::QT_VERSION).join(platform::QT_KIT_DIR)
}

pub fn configuration(mode: Mode) -> &'static str {
    match mode {
        Mode::Dev => "debug",
        Mode::Release => "release",
    }
}

/// Build the Tauri UI and stage it under the fixed package name.
pub fn build(mode: Mode, version: &str, test: bool, package: bool, _host: &Path) -> anyhow::Result<Ui> {
    let source = source();
    let config: serde_json::Value = serde_json::from_slice(&fs::read(source.join("src-tauri/tauri.conf.json"))?)?;
    anyhow::ensure!(
        config["version"].as_str() == Some(version),
        "Tauri UI version must match the workspace version {version}"
    );
    #[cfg(target_os = "macos")]
    anyhow::ensure!(
        config["bundle"]["macOS"]["minimumSystemVersion"].as_str() == Some(pins::MIN_MACOS),
        "Tauri UI minimum macOS version must be {}",
        pins::MIN_MACOS
    );
    let binary = mode.cargo_out().join(crate::naming::exe("savescummer-ui"));
    let dist = source.join("dist");
    // Everything under apps/ui (the frontend, its lock file, the Tauri crate
    // and config), and every Rust file the UI binary was built from. pnpm's
    // version is pinned in package.json, and checked whenever this runs.
    let inputs = || {
        let mut key = cache::Key::new("ui")?;
        key.text(&format!("package={package}"));
        key.sources(&["apps/ui"])?;
        key.cargo_setup()?;
        key.dep_info(&mode.cargo_out().join("savescummer-ui.d"), std::slice::from_ref(&dist))?;
        Ok(key)
    };
    let work = || -> anyhow::Result<()> {
        let pnpm = cmd::on_path("pnpm", "install pnpm for the Tauri UI build")?;
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(source.join("package.json"))?)?;
        let required_pnpm = manifest["packageManager"]
            .as_str()
            .and_then(|value| value.strip_prefix("pnpm@"))
            .context("apps/ui/package.json must specify a pnpm version")?;
        let installed_pnpm = cmd::output(Command::new(&pnpm).arg("--version"))?;
        anyhow::ensure!(
            installed_pnpm == required_pnpm,
            "the Tauri UI needs pnpm {required_pnpm}, but {} is {installed_pnpm}",
            paths::show(&pnpm)
        );
        cmd::run(Command::new(&pnpm).current_dir(&source).args(["install", "--frozen-lockfile"]))?;
        if test {
            cmd::run(Command::new(&pnpm).current_dir(&source).arg("test"))?;
        }
        // The frontend is built here rather than by Tauri's beforeBuildCommand, so
        // its output can be fixed up before Cargo looks at it.
        cmd::run(Command::new(&pnpm).current_dir(&source).arg("build"))?;
        keep_unchanged_times(&dist, &mode.dir().join("ui-dist.json"))?;
        let no_frontend = paths::scratch().join("tauri-no-frontend.json");
        fs::create_dir_all(paths::scratch())?;
        fs::write(&no_frontend, r#"{ "build": { "beforeBuildCommand": null } }"#)?;
        let mut build = Command::new(&pnpm);
        build.current_dir(&source).args(["tauri", "build", "--config"]).arg(&no_frontend);
        platform::tauri_bundle(&mut build, package)?;
        if mode == Mode::Dev {
            build.arg("--debug");
        }
        cmd::run(&mut build)
    };
    cache::Step::new("ui", mode, !test, vec![binary.clone()]).run(inputs, work)?;

    anyhow::ensure!(binary.is_file(), "the Tauri build did not create {}", paths::show(&binary));
    let install = mode.dir().join("ui-install");
    if install.exists() {
        fs::remove_dir_all(&install).with_context(|| format!("removing {}", install.display()))?;
    }
    let staged = install.join("bin").join(crate::naming::exe(crate::naming::UI));
    fs::create_dir_all(staged.parent().expect("bin has a parent"))?;
    fs::copy(&binary, &staged).with_context(|| format!("staging {}", paths::show(&binary)))?;
    if package {
        platform::stage_tauri_bundle(mode, &install)?;
    }
    Ok(Ui { install })
}

/// Gives each file in `dist` whose content is the same as at the last build
/// its old modification time back. Vite rewrites every file on every build,
/// and the Tauri crate embeds them, so otherwise Cargo (which goes by
/// modification times) recompiles the UI each time for identical bytes.
/// `record` holds each file's hash and time from the last build in this mode.
fn keep_unchanged_times(dist: &Path, record: &Path) -> anyhow::Result<()> {
    let previous: serde_json::Map<String, serde_json::Value> =
        fs::read(record).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let mut current = serde_json::Map::new();
    for file in crate::package::files(dist)? {
        let name = file.strip_prefix(dist).expect("listed under dist").to_string_lossy().replace('\\', "/");
        let hash = crate::package::sha256_file(&file)?;
        let mut modified = fs::metadata(&file)?.modified()?;
        let old = previous.get(&name);
        if old.and_then(|old| old["sha256"].as_str()) == Some(hash.as_str())
            && let Some(nanos) = old.and_then(|old| old["modified_ns"].as_u64())
        {
            modified = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(nanos);
            fs::File::options()
                .write(true)
                .open(&file)
                .and_then(|f| f.set_modified(modified))
                .with_context(|| format!("restoring the time of {}", file.display()))?;
        }
        let nanos = modified.duration_since(std::time::UNIX_EPOCH)?.as_nanos() as u64;
        current.insert(name, serde_json::json!({ "sha256": hash, "modified_ns": nanos }));
    }
    fs::create_dir_all(record.parent().expect("record is in a folder"))?;
    fs::write(record, serde_json::to_vec_pretty(&current)?).with_context(|| format!("writing {}", record.display()))
}
