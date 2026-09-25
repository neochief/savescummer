//! Every pinned tool version and every supported-platform minimum, in one
//! place (PLAN-BUILD.md TOOLCHAINS, WHAT USERS GET). Bumping one is a
//! deliberate, one-line commit; CI caches are keyed on this file.
//!
//! Each platform module uses its own minimums; the others are unused there.
#![allow(dead_code)]

/// Qt follows the latest minor release; patches are taken promptly.
pub const QT_VERSION: &str = "6.12.0";
/// Installs Qt into `.runtime/Qt/`.
pub const AQTINSTALL_VERSION: &str = "3.3.0";

pub const INNO_VERSION: &str = "6.7.3";
pub const INNO_URL: &str = "https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe";
pub const INNO_SHA256: &str = "9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732";

pub const CARGO_ABOUT_VERSION: &str = "0.9.2";

/// Windows 10/11 x64.
pub const MIN_WINDOWS: &str = "10.0";
/// Apple Silicon only. Never below what the pinned Qt supports.
pub const MIN_MACOS: &str = "13.0";
/// Linux builds run on the oldest supported Ubuntu, so this is its glibc.
pub const MIN_GLIBC: &str = "2.35";
pub const OLDEST_UBUNTU: &str = "22.04";

#[cfg(test)]
mod tests {
    /// `.cargo/config.toml` sets the deployment target for every Rust build;
    /// it must be this minimum.
    #[test]
    fn cargo_builds_target_the_minimum_macos() {
        let config = std::fs::read_to_string(crate::paths::root().join(".cargo").join("config.toml")).unwrap();
        assert!(config.contains(&format!("MACOSX_DEPLOYMENT_TARGET = \"{}\"", super::MIN_MACOS)), "{config}");
    }
}
