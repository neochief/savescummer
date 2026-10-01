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

/// Linux AppImage tools (PLAN-BUILD.md Linux), for the build machine's
/// architecture. The first three are what Tauri's AppImage bundler runs
/// (its `linuxdeploy` build pinned by commit, and its `AppRun`); `setup
/// linux-tools` puts them in its cache so the build downloads nothing.
/// `appimagetool` and its runtime make the release file.
pub struct Pinned {
    pub url: &'static str,
    pub sha256: &'static str,
    /// The file name the user of the tool expects.
    pub file: &'static str,
    /// In Tauri's tool cache, rather than beside appimagetool.
    pub tauri: bool,
}

pub const APPIMAGETOOL_VERSION: &str = "1.9.1";
pub const APPIMAGE_RUNTIME_VERSION: &str = "20251108";

#[cfg(target_arch = "x86_64")]
pub const LINUX_TOOLS: [Pinned; 5] = [
    Pinned {
        url: "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage",
        sha256: "36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62",
        file: "linuxdeploy-07333c6-x86_64.AppImage",
        tauri: true,
    },
    Pinned {
        url: "https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64",
        sha256: "f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f",
        file: "AppRun-x86_64",
        tauri: true,
    },
    Pinned {
        url: "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage",
        sha256: "992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3",
        file: "linuxdeploy-plugin-appimage.AppImage",
        tauri: true,
    },
    Pinned {
        url: "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage",
        sha256: "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0",
        file: "appimagetool",
        tauri: false,
    },
    Pinned {
        url: "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64",
        sha256: "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
        file: "runtime",
        tauri: false,
    },
];

#[cfg(target_arch = "aarch64")]
pub const LINUX_TOOLS: [Pinned; 5] = [
    Pinned {
        url: "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-aarch64.AppImage",
        sha256: "556ab80baa98e600aa80f0dcedfb70bca0e1ce7e9f147fb345be3fcc3e91b2b1",
        file: "linuxdeploy-07333c6-aarch64.AppImage",
        tauri: true,
    },
    Pinned {
        url: "https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-aarch64",
        sha256: "072f17c0895a85c490282fe5395c5007e5fc75da727e553b3b8fb680feb11578",
        file: "AppRun-aarch64",
        tauri: true,
    },
    Pinned {
        url: "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-aarch64.AppImage",
        sha256: "83c292149274965a865dcd44c135cfca8ba28c6b7de3eb628d4b8b5f248af17c",
        file: "linuxdeploy-plugin-appimage.AppImage",
        tauri: true,
    },
    Pinned {
        url: "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-aarch64.AppImage",
        sha256: "f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158",
        file: "appimagetool",
        tauri: false,
    },
    Pinned {
        url: "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-aarch64",
        sha256: "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444",
        file: "runtime",
        tauri: false,
    },
];

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
