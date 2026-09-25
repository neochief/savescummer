//! Linux x86_64, glibc 2.35+ (PLAN-BUILD.md Linux): one AppImage that acts
//! as all three programs. Not implemented yet; the shared parts already call
//! into this module, so adding it doesn't touch them.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::bail;

use crate::naming;
use crate::package::{Inputs, Layout};

pub use crate::unix::{ask_to_close, hide_window, spawn_detached};

pub const PLATFORM: naming::Platform = naming::LINUX;
pub const RUST_TARGET: &str = "x86_64-unknown-linux-gnu";
pub const QT_AQT_HOST: &str = "linux";
pub const QT_AQT_ARCH: &str = "linux_gcc_64";
pub const QT_KIT_DIR: &str = "gcc_64";

const NOT_YET: &str = "Linux packaging isn't implemented yet (PLAN-BUILD.md Linux)";

pub fn package_name() -> String {
    "SaveScummer.AppDir".into()
}

pub fn check_build_machine() -> anyhow::Result<()> {
    Ok(())
}

pub fn cmake_args() -> Vec<String> {
    Vec::new()
}

pub fn qt_runtime_env(_command: &mut Command, _kit: &Path) {}

/// Where the program with the fixed executable name `name` is in a package
/// (in the AppDir; the AppImage itself dispatches through `AppRun`).
pub fn program(package: &Path, name: &str) -> PathBuf {
    package.join("usr").join("bin").join(name)
}

pub fn fill_package(_root: &Path, _inputs: &Inputs) -> anyhow::Result<Layout> {
    bail!(NOT_YET)
}

pub fn finish_package(_root: &Path) -> anyhow::Result<()> {
    Ok(())
}

pub fn release_file(_package: &Path, _version: &str) -> anyhow::Result<PathBuf> {
    bail!(NOT_YET)
}

pub fn setup_inno() -> anyhow::Result<()> {
    bail!("`setup inno` is only for Windows builds")
}

pub fn setup_linux_tools() -> anyhow::Result<()> {
    bail!(NOT_YET)
}
