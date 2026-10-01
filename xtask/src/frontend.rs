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
use crate::{cmd, pins, platform};

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
    let mut build = Command::new(&pnpm);
    build.current_dir(&source).args(["tauri", "build"]);
    platform::tauri_bundle(&mut build, package)?;
    if mode == Mode::Dev {
        build.arg("--debug");
    }
    cmd::run(&mut build)?;

    let binary = mode.cargo_out().join(crate::naming::exe("savescummer-ui"));
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
