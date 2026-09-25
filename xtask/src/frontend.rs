//! The desktop frontend layer (PLAN-BUILD.md TOOLCHAINS): today Qt and C++.
//! Nothing outside this module (and the platform steps that deploy its
//! runtime) assumes either.
//!
//! The frontend plugs in through:
//!
//! - a setup command for its pinned SDK (`setup qt`)
//! - a build step producing the `SaveScummer` executable and its runtime files
//! - a test step that runs headless against the freshly built host
//! - its license texts, shipped in every package
//!
//! Until `apps/desktop` exists the frontend is skipped and packages carry only
//! the host and CLI; once it exists it's required.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::paths::{self, Mode};
use crate::{cmd, pins, platform};

/// A built desktop, ready to be packaged.
pub struct Desktop {
    /// The `cmake --install` output.
    pub install: PathBuf,
}

pub fn source() -> PathBuf {
    paths::root().join("apps").join("desktop")
}

/// Whether there is a desktop to build yet.
pub fn present() -> bool {
    source().join("CMakeLists.txt").is_file()
}

/// Qt's license texts, shipped with every package that contains the desktop.
pub fn licenses() -> PathBuf {
    paths::packaging().join("licenses")
}

pub fn version() -> &'static str {
    pins::QT_VERSION
}

/// `.runtime/Qt/<version>/<kit>/`, installed by `setup qt`.
pub fn kit() -> PathBuf {
    paths::runtime().join("Qt").join(pins::QT_VERSION).join(platform::QT_KIT_DIR)
}

pub fn configuration(mode: Mode) -> &'static str {
    match mode {
        Mode::Dev => "RelWithDebInfo",
        Mode::Release => "Release",
    }
}

/// Configures, builds, optionally tests, and installs the desktop.
pub fn build(mode: Mode, version: &str, test: bool, host: &Path) -> anyhow::Result<Desktop> {
    let kit = kit();
    if !kit.join("lib").is_dir() {
        anyhow::bail!("Qt kit not found at {} — run `cargo xtask setup qt`", paths::show(&kit));
    }
    let cmake = cmd::on_path(
        "cmake",
        "install CMake 3.21+ (Visual Studio, Xcode command-line tools or your distro provide it)",
    )?;
    let dir = mode.dir().join("desktop");
    let install = mode.dir().join("desktop-install");
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

    let mut targets = vec!["savescummer-desktop"];
    if test {
        targets.push("desktop-tests");
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
    Ok(Desktop { install })
}
