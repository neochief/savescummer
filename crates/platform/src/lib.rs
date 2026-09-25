//! OS adapters the host owns: data folders, opening folders, launch on
//! startup, sounds, hotkeys and the tray, and watching store folders.
//!
//! Each adapter keeps one file per OS next to its shared code, with an
//! `unsupported.rs` where an OS isn't done yet, so adding an OS means adding
//! files. Windows is the first real target.

use std::path::{Path, PathBuf};

pub mod autostart;
pub mod integration;
pub mod process;
pub mod sounds;
pub mod watch;

#[cfg(windows)]
mod win;

/// The folder name used under every per-user base folder.
const APP_DIR: &str = "SaveScummer";

// Where data lives and how folders are opened, per OS.
#[cfg_attr(windows, path = "folders/windows.rs")]
#[cfg_attr(target_os = "macos", path = "folders/macos.rs")]
#[cfg_attr(all(unix, not(target_os = "macos")), path = "folders/linux.rs")]
mod folders;

/// The per-user data folder (database, settings, downloaded catalog).
///
/// Resolved through the OS, never from a hardcoded user name or drive:
/// - Windows: `%LOCALAPPDATA%\SaveScummer` (known folder, env as fallback)
/// - macOS: `~/Library/Application Support/SaveScummer`
/// - Linux: `$XDG_DATA_HOME/SaveScummer`, else `~/.local/share/SaveScummer`
pub fn data_dir() -> PathBuf {
    folders::data_base().join(APP_DIR)
}

/// The disposable cache folder.
///
/// - Windows: `%LOCALAPPDATA%\SaveScummer\cache`
/// - macOS: `~/Library/Caches/SaveScummer`
/// - Linux: `$XDG_CACHE_HOME/SaveScummer`, else `~/.cache/SaveScummer`
pub fn cache_dir() -> PathBuf {
    folders::cache_dir(&data_dir(), APP_DIR)
}

/// Opens a folder in the OS file manager. Never blocks long: the file manager
/// is started, not waited for.
pub fn open_folder(path: &Path) -> std::io::Result<()> {
    folders::open_folder(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_absolute_and_named_for_the_app() {
        let data = data_dir();
        assert!(data.is_absolute(), "{data:?}");
        assert_eq!(data.file_name().unwrap(), APP_DIR);
    }

    #[test]
    fn cache_dir_is_absolute() {
        let cache = cache_dir();
        assert!(cache.is_absolute(), "{cache:?}");
    }
}
