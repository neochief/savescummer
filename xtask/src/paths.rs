//! The four disk roots (PLAN-BUILD.md DISK) and the paths inside them.
//!
//! - `target/`: Cargo's cache, never written directly
//! - `build/`: everything regenerable
//! - `dist/`: release files and nothing else
//! - `.runtime/`: SDKs, tools and dev data; never cleaned

use std::path::{Path, PathBuf};

/// The workspace root: xtask lives one level below it.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask is inside the workspace").to_path_buf()
}

/// Cargo's output, honoring `CARGO_TARGET_DIR` like Cargo does.
pub fn target() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => root().join(dir),
        None => root().join("target"),
    }
}

pub fn build() -> PathBuf {
    root().join("build")
}

pub fn dist() -> PathBuf {
    root().join("dist")
}

pub fn runtime() -> PathBuf {
    root().join(".runtime")
}

/// Pinned tools installed by `cargo xtask setup …`.
pub fn tools() -> PathBuf {
    runtime().join("tools")
}

/// The dev host's `--data-dir`, so development never touches the real app's data.
pub fn dev_data() -> PathBuf {
    runtime().join("dev")
}

pub fn packaging() -> PathBuf {
    root().join("packaging")
}

/// Scratch space; removed with the rest of `build/`.
pub fn scratch() -> PathBuf {
    build().join("tmp")
}

/// Dev builds keep debug info and never create a sign-in entry; release
/// builds are optimized, stripped, and what ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dev,
    Release,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Dev => "dev",
            Mode::Release => "release",
        }
    }

    /// `build/<mode>/`.
    pub fn dir(self) -> PathBuf {
        build().join(self.name())
    }

    /// Where the APP PACKAGE is assembled and swapped into place.
    pub fn package_parent(self) -> PathBuf {
        self.dir().join("package")
    }

    /// Cargo's output folder for this mode.
    pub fn cargo_out(self) -> PathBuf {
        target().join(match self {
            Mode::Dev => "debug",
            Mode::Release => "release",
        })
    }
}

/// `path` relative to the repository root, for messages.
pub fn show(path: &Path) -> String {
    path.strip_prefix(root()).unwrap_or(path).display().to_string()
}
