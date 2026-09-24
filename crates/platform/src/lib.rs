//! OS adapters the host owns: data folders, opening folders, launch on
//! startup, sounds, hotkeys and the tray, and watching store folders.
//!
//! Windows is the first real target. macOS and Linux compile everywhere, with
//! simple implementations where they are cheap (folders, opening, watching) and
//! explicit "not supported yet" answers elsewhere.

use std::path::{Path, PathBuf};

pub mod autostart;
pub mod integration;
pub mod sounds;
pub mod watch;

#[cfg(windows)]
mod win;

/// The folder name used under every per-user base folder.
const APP_DIR: &str = "SaveScummer";

/// The per-user data folder (database, settings, downloaded catalog).
///
/// Resolved through the OS, never from a hardcoded user name or drive:
/// - Windows: `%LOCALAPPDATA%\SaveScummer` (known folder, env as fallback)
/// - macOS: `~/Library/Application Support/SaveScummer`
/// - Linux: `$XDG_DATA_HOME/SaveScummer`, else `~/.local/share/SaveScummer`
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        win::local_app_data().join(APP_DIR)
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Application Support").join(APP_DIR)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        xdg_or_home("XDG_DATA_HOME", ".local/share").join(APP_DIR)
    }
}

/// The disposable cache folder.
///
/// - Windows: `%LOCALAPPDATA%\SaveScummer\cache`
/// - macOS: `~/Library/Caches/SaveScummer`
/// - Linux: `$XDG_CACHE_HOME/SaveScummer`, else `~/.cache/SaveScummer`
pub fn cache_dir() -> PathBuf {
    #[cfg(windows)]
    {
        data_dir().join("cache")
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Caches").join(APP_DIR)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        xdg_or_home("XDG_CACHE_HOME", ".cache").join(APP_DIR)
    }
}

/// Opens a folder in the OS file manager. Never blocks long: the file manager
/// is started, not waited for.
pub fn open_folder(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        win::shell_open(path)
    }
    #[cfg(unix)]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let mut child = std::process::Command::new(opener).arg(path).spawn()?;
        // Reap it in the background so it never lingers as a zombie, without
        // making the caller wait for the file manager.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

/// The real home directory (`$HOME`, else the password database).
#[cfg(unix)]
fn home() -> PathBuf {
    #[allow(deprecated)] // Correct on Unix; the deprecation was about Windows.
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// `$<var>` when it is set to an absolute path (the XDG rule), else `~/<fallback>`.
#[cfg(all(unix, not(target_os = "macos")))]
fn xdg_or_home(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var).map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        _ => home().join(fallback),
    }
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
        #[cfg(windows)]
        assert_eq!(cache, data_dir().join("cache"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_data_dir_is_under_local_app_data() {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        if let Some(local) = local {
            let data = data_dir().to_string_lossy().to_lowercase();
            let local = local.to_string_lossy().to_lowercase();
            assert!(data.starts_with(&local), "{data} vs {local}");
        }
    }
}
