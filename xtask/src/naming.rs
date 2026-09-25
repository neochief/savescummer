//! Release file names and the platforms that ship them (PLAN-BUILD.md WHAT
//! USERS GET). Names only: how each file is made lives in the platform module.

/// One release file per supported platform.
pub struct Platform {
    pub os: &'static str,
    /// Each OS's own habit (`x64` on Windows, `arm64`/`x86_64` elsewhere); never unified.
    pub arch: &'static str,
    /// Appended after the version, e.g. `setup`.
    pub suffix: Option<&'static str>,
    pub ext: &'static str,
    /// Whether releases include it yet. `publish` requires exactly one file
    /// per shipping platform.
    pub ships: bool,
}

pub const WINDOWS: Platform = Platform { os: "windows", arch: "x64", suffix: Some("setup"), ext: "exe", ships: true };
pub const MACOS: Platform = Platform { os: "macos", arch: "arm64", suffix: None, ext: "dmg", ships: false };
pub const LINUX: Platform = Platform { os: "linux", arch: "x86_64", suffix: None, ext: "AppImage", ships: false };

pub const ALL: [Platform; 3] = [WINDOWS, MACOS, LINUX];

/// The fixed executable names, whatever the platform packages them in.
pub const DESKTOP: &str = "SaveScummer";
pub const HOST: &str = "SaveScummer.Host";
pub const CLI: &str = "SaveScummer.CLI";

/// Cargo can't put dots in binary names, so packaging renames these.
pub const CARGO_HOST: &str = "savescummer-host";
pub const CARGO_CLI: &str = "savescummer-cli";

impl Platform {
    /// `SaveScummer-<os>-<arch>-<version>[-<suffix>].<ext>`.
    pub fn release_file(&self, version: &str) -> String {
        let suffix = self.suffix.map(|s| format!("-{s}")).unwrap_or_default();
        format!("SaveScummer-{}-{}-{version}{suffix}.{}", self.os, self.arch, self.ext)
    }

    /// `SaveScummer-<os>-<arch>`, the stem of package folders.
    pub fn stem(&self) -> String {
        format!("SaveScummer-{}-{}", self.os, self.arch)
    }
}

/// `name` plus this OS's executable suffix.
pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_file_names() {
        assert_eq!(WINDOWS.release_file("1.2.3"), "SaveScummer-windows-x64-1.2.3-setup.exe");
        assert_eq!(MACOS.release_file("1.2.3"), "SaveScummer-macos-arm64-1.2.3.dmg");
        assert_eq!(LINUX.release_file("1.2.3"), "SaveScummer-linux-x86_64-1.2.3.AppImage");
    }
}
