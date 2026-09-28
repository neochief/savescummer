//! The UI frontend layer: Tauri on Windows, and the earlier Qt build on
//! platforms where it still exists.
//! Nothing outside this module (and the platform steps that deploy its
//! runtime) assumes either.
//!
//! The frontend plugs in through:
//!
//! - a setup command for its pinned SDK (`setup qt`)
//! - a build step producing the `SaveScummer.UI` executable and its runtime files
//! - a test step that runs headless against the freshly built host
//! - its license texts, shipped in every package
//!
//! Until `apps/ui` exists the frontend is skipped and packages carry only
//! the host and CLI; once it exists it's required.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::paths::{self, Mode};
use crate::{cmd, pins, platform};

/// A built UI, ready to be packaged.
pub struct Ui {
    /// The `cmake --install` output. Read by the platform packaging, which
    /// Linux doesn't have yet.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub install: PathBuf,
}

pub fn source() -> PathBuf {
    paths::root().join("apps").join("ui")
}

/// Whether there is a UI to build yet.
pub fn present() -> bool {
    #[cfg(windows)]
    {
        source().join("package.json").is_file() && source().join("src-tauri").join("Cargo.toml").is_file()
    }
    #[cfg(not(windows))]
    {
        source().join("CMakeLists.txt").is_file()
    }
}

/// License texts for a frontend with separately shipped runtime libraries.
pub fn licenses() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        None
    }
    #[cfg(not(windows))]
    {
        Some(paths::packaging().join("licenses"))
    }
}

pub fn version() -> &'static str {
    #[cfg(windows)]
    {
        "Tauri 2"
    }
    #[cfg(not(windows))]
    {
        pins::QT_VERSION
    }
}

/// `.runtime/Qt/<version>/<kit>/`, installed by `setup qt`.
pub fn kit() -> PathBuf {
    paths::runtime().join("Qt").join(pins::QT_VERSION).join(platform::QT_KIT_DIR)
}

pub fn configuration(mode: Mode) -> &'static str {
    #[cfg(windows)]
    {
        return match mode {
            Mode::Dev => "debug",
            Mode::Release => "release",
        };
    }
    #[cfg(not(windows))]
    match mode {
        Mode::Dev => "RelWithDebInfo",
        Mode::Release => "Release",
    }
}

/// Configures, builds, optionally tests, and installs the UI.
#[cfg(not(windows))]
pub fn build(mode: Mode, version: &str, test: bool, host: &Path) -> anyhow::Result<Ui> {
    let kit = kit();
    if !kit.join("lib").is_dir() {
        anyhow::bail!("Qt kit not found at {} — run `cargo xtask setup qt`", paths::show(&kit));
    }
    let cmake = cmd::on_path(
        "cmake",
        "install CMake 3.21+ (Visual Studio, Xcode command-line tools or your distro provide it)",
    )?;
    let dir = mode.dir().join("ui");
    let install = mode.dir().join("ui-install");
    let config = configuration(mode);

    let mut configure = Command::new(&cmake);
    configure
        .arg("-S")
        .arg(source())
        .arg("-B")
        .arg(&dir)
        .arg(format!("-DCMAKE_PREFIX_PATH={}", kit.display()))
        .arg(format!("-DCMAKE_BUILD_TYPE={config}"))
        .arg(format!("-DSAVESCUMMER_VERSION={version}"))
        .args(platform::cmake_args());
    cmd::run(&mut configure)?;

    let mut targets = vec!["savescummer-ui"];
    if test {
        targets.push("ui-tests");
    }
    let mut compile = Command::new(&cmake);
    compile.arg("--build").arg(&dir).args(["--config", config, "--parallel", "--target"]).args(&targets);
    cmd::run(&mut compile)?;

    if test {
        let ctest = cmake.with_file_name(crate::naming::exe("ctest"));
        let mut run = Command::new(if ctest.is_file() { ctest } else { PathBuf::from("ctest") });
        run.arg("--test-dir")
            .arg(&dir)
            .args(["-C", config, "--output-on-failure"])
            .env("SAVESCUMMER_TEST_HOST", host)
            .env("QT_QPA_PLATFORM", "offscreen");
        platform::qt_runtime_env(&mut run, &kit);
        cmd::run(&mut run)?;
    }

    if install.exists() {
        fs::remove_dir_all(&install).with_context(|| format!("removing {}", install.display()))?;
    }
    let mut deploy = Command::new(&cmake);
    deploy.arg("--install").arg(&dir).args(["--config", config, "--prefix"]).arg(&install);
    cmd::run(&mut deploy)?;
    Ok(Ui { install })
}

/// Build the embedded WebView2 UI and stage it under the fixed package name.
#[cfg(windows)]
pub fn build(mode: Mode, version: &str, test: bool, _host: &Path) -> anyhow::Result<Ui> {
    let source = source();
    let config: serde_json::Value = serde_json::from_slice(&fs::read(source.join("src-tauri/tauri.conf.json"))?)?;
    anyhow::ensure!(
        config["version"].as_str() == Some(version),
        "Tauri UI version must match the workspace version {version}"
    );
    let pnpm = cmd::on_path("pnpm", "install pnpm for the Tauri UI build")?;
    cmd::run(Command::new(&pnpm).current_dir(&source).args(["install", "--frozen-lockfile"]))?;
    if test {
        cmd::run(Command::new(&pnpm).current_dir(&source).arg("test"))?;
    }
    let mut build = Command::new(&pnpm);
    build.current_dir(&source).args(["tauri", "build", "--no-bundle"]);
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
    Ok(Ui { install })
}
