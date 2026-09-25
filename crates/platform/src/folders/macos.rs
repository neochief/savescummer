use std::path::{Path, PathBuf};

/// `~/Library/Application Support`.
pub fn data_base() -> PathBuf {
    home().join("Library/Application Support")
}

/// `~/Library/Caches/<app>`.
pub fn cache_dir(_data_dir: &Path, app: &str) -> PathBuf {
    home().join("Library/Caches").join(app)
}

pub fn open_folder(path: &Path) -> std::io::Result<()> {
    spawn_opener("open", path)
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
