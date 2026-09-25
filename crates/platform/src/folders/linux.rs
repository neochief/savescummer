use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME`, else `~/.local/share`.
pub fn data_base() -> PathBuf {
    xdg_or_home("XDG_DATA_HOME", ".local/share")
}

/// `$XDG_CACHE_HOME/<app>`, else `~/.cache/<app>`.
pub fn cache_dir(_data_dir: &Path, app: &str) -> PathBuf {
    xdg_or_home("XDG_CACHE_HOME", ".cache").join(app)
}

pub fn open_folder(path: &Path) -> std::io::Result<()> {
    spawn_opener("xdg-open", path)
}

/// `$<var>` when it is set to an absolute path (the XDG rule), else `~/<fallback>`.
fn xdg_or_home(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var).map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        _ => home().join(fallback),
    }
}

/// Starts `opener` on the folder and reaps it in the background, so it never
/// lingers as a zombie and the caller never waits for the file manager.
fn spawn_opener(opener: &str, path: &Path) -> std::io::Result<()> {
    let mut child = std::process::Command::new(opener).arg(path).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The real home directory (`$HOME`, else the password database).
fn home() -> PathBuf {
    #[allow(deprecated)] // Correct on Unix; the deprecation was about Windows.
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}
