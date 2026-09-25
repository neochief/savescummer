use std::path::{Path, PathBuf};

use crate::win;

/// `%LOCALAPPDATA%` (known folder, env as fallback).
pub fn data_base() -> PathBuf {
    win::local_app_data()
}

/// Windows keeps the cache inside the data folder.
pub fn cache_dir(data_dir: &Path, _app: &str) -> PathBuf {
    data_dir.join("cache")
}

pub fn open_folder(path: &Path) -> std::io::Result<()> {
    win::shell_open(path)
}

#[cfg(test)]
mod tests {
    use crate::{cache_dir, data_dir};
    use std::path::PathBuf;

    #[test]
    fn the_cache_is_inside_the_data_folder() {
        assert_eq!(cache_dir(), data_dir().join("cache"));
    }

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
