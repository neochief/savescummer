//! macOS 13+, Apple Silicon (PLAN-BUILD.md macOS): `SaveScummer.app` in a
//! drag-to-Applications DMG. Not implemented yet; the shared parts already
//! call into this module, so adding it doesn't touch them.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::bail;

use crate::naming;
use crate::package::{Inputs, Layout};
use crate::pins;

pub use crate::unix::{ask_to_close, hide_window, spawn_detached};

pub const PLATFORM: naming::Platform = naming::MACOS;
pub const RUST_TARGET: &str = "aarch64-apple-darwin";
pub const QT_AQT_HOST: &str = "mac";
pub const QT_AQT_ARCH: &str = "clang_64";
pub const QT_KIT_DIR: &str = "macos";

const NOT_YET: &str = "macOS packaging isn't implemented yet (PLAN-BUILD.md macOS)";

pub fn package_name() -> String {
    "SaveScummer.app".into()
}

/// Intel Macs aren't supported.
pub fn check_build_machine() -> anyhow::Result<()> {
    if std::env::consts::ARCH != "aarch64" {
        bail!("SaveScummer builds only on Apple Silicon Macs; Intel Macs aren't supported");
    }
    Ok(())
}

pub fn cmake_args() -> Vec<String> {
    vec!["-DCMAKE_OSX_ARCHITECTURES=arm64".into(), format!("-DCMAKE_OSX_DEPLOYMENT_TARGET={}", pins::MIN_MACOS)]
}

pub fn qt_runtime_env(_command: &mut Command, _kit: &Path) {}

pub fn fill_package(_root: &Path, _inputs: &Inputs) -> anyhow::Result<Layout> {
    bail!(NOT_YET)
}

pub fn release_file(_package: &Path, _version: &str) -> anyhow::Result<PathBuf> {
    bail!(NOT_YET)
}

pub fn setup_inno() -> anyhow::Result<()> {
    bail!("`setup inno` is only for Windows builds")
}

pub fn setup_linux_tools() -> anyhow::Result<()> {
    bail!("`setup linux-tools` is only for Linux builds")
}
