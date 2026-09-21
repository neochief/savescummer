use savescummer_core::*;
use savescummer_monitor::{Observation, ObservationSource};
use savescummer_scanner::Discovery;
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub mod desktop;
pub mod sounds;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_desktop;
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

pub struct Paths {
    protected: std::sync::RwLock<Vec<PathBuf>>,
}
impl Paths {
    pub fn new(protected: Vec<PathBuf>) -> Self {
        Self {
            protected: std::sync::RwLock::new(protected),
        }
    }
    pub fn protect(&self, paths: impl IntoIterator<Item = PathBuf>) -> Result<()> {
        let mut protected = self
            .protected
            .write()
            .map_err(|_| Error::new(ErrorCode::InvalidPath, "protected paths lock poisoned"))?;
        protected.extend(paths);
        protected.sort();
        protected.dedup();
        Ok(())
    }
    fn reference_key(&self, path: &Path) -> Result<PathBuf> {
        check_absolute_path(path)?;
        // References reserve locations even while their volume is unavailable.
        // Only the actual operation target must resolve successfully. Keep the
        // recorded absolute path as a comparison key if this reference cannot.
        Ok(comparable(
            &self
                .resolve(path)
                .unwrap_or_else(|_| path.components().collect()),
        ))
    }
    pub fn system(extra: Vec<PathBuf>) -> Self {
        let mut protected = known_folders().into_values().collect::<Vec<_>>();
        #[cfg(windows)]
        for key in [
            "SystemRoot",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramData",
            "PUBLIC",
        ] {
            if let Some(path) = std::env::var_os(key) {
                protected.push(path.into());
            }
        }
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            protected.push(PathBuf::from(root).join("System32"));
        }
        protected.extend(extra);
        Self::new(protected)
    }
}
fn comparable(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}
fn check_absolute_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::new(
            ErrorCode::InvalidPath,
            "an absolute path is required",
        ));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::new(
            ErrorCode::InvalidPath,
            "parent traversal is not allowed",
        ));
    }
    #[cfg(windows)]
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name = name.to_string_lossy();
            if name.contains(':') || name.ends_with(['.', ' ']) {
                return Err(Error::new(
                    ErrorCode::InvalidPath,
                    "ambiguous Windows path component",
                ));
            }
        }
    }
    Ok(())
}
impl PathPolicy for Paths {
    fn same_location(&self, recorded: &Path, current: &Path) -> bool {
        if recorded == current {
            return true;
        }
        // Resolve aliases and actual spelling instead of folding case globally.
        // Unavailable unrelated locations cannot establish a match.
        matches!((self.resolve(recorded), self.resolve(current)), (Ok(a), Ok(b)) if a == b)
    }
    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        check_absolute_path(path)?;
        let mut ancestor = path.to_path_buf();
        let mut tail = vec![];
        loop {
            match dunce::canonicalize(&ancestor) {
                Ok(mut resolved) => {
                    for component in tail.iter().rev() {
                        resolved.push(component);
                    }
                    return Ok(resolved);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tail.push(
                        ancestor
                            .file_name()
                            .ok_or_else(|| {
                                Error::new(
                                    ErrorCode::InvalidPath,
                                    "no accessible existing ancestor",
                                )
                            })?
                            .to_os_string(),
                    );
                    if !ancestor.pop() {
                        return Err(Error::new(ErrorCode::InvalidPath, "no existing ancestor"));
                    }
                }
                Err(e) => {
                    return Err(Error::new(
                        ErrorCode::InvalidPath,
                        format!("{}: {e}", ancestor.display()),
                    ));
                }
            }
        }
    }
    fn validate(&self, path: &Path, others: &[(Id, PathBuf)]) -> Result<PathBuf> {
        let resolved = self.resolve(path)?;
        if resolved.exists() && !resolved.is_dir() {
            return Err(Error::new(ErrorCode::InvalidPath, "select a directory"));
        }
        if resolved.parent().is_none() || resolved.file_name().is_none() {
            return Err(Error::new(
                ErrorCode::InvalidPath,
                "select the game's own data directory, not a volume or share root",
            ));
        }
        let key = comparable(&resolved);
        for protected in self
            .protected
            .read()
            .map_err(|_| Error::new(ErrorCode::InvalidPath, "protected paths lock poisoned"))?
            .iter()
        {
            let protected = self.reference_key(protected)?;
            if protected.starts_with(&key) {
                return Err(Error::new(
                    ErrorCode::InvalidPath,
                    "select the game's own data directory, not a protected root or its ancestor",
                ));
            }
        }
        for (id, other) in others {
            let other = self.reference_key(other)?;
            if key.starts_with(&other) || other.starts_with(&key) {
                return Err(Error::new(
                    ErrorCode::InvalidPath,
                    format!(
                        "data location conflicts with game {id}: {}",
                        other.display()
                    ),
                ));
            }
        }
        Ok(resolved)
    }
}
pub struct NativeDiscovery {
    pub additional_steam_roots: Vec<PathBuf>,
}
impl Discovery for NativeDiscovery {
    fn steam_roots(&self) -> Vec<PathBuf> {
        let mut roots = self.additional_steam_roots.clone();
        #[cfg(windows)]
        roots.extend(windows::steam_roots());
        roots.sort();
        roots.dedup();
        roots
    }
    fn known_folders(&self) -> BTreeMap<String, PathBuf> {
        known_folders()
    }
    fn platform(&self) -> &'static str {
        std::env::consts::OS
    }
    fn applications(&self) -> savescummer_scanner::ApplicationScan {
        #[cfg(windows)]
        {
            windows::applications()
        }
        #[cfg(not(windows))]
        {
            savescummer_scanner::ApplicationScan::default()
        }
    }
}
pub fn known_folders() -> BTreeMap<String, PathBuf> {
    #[cfg(windows)]
    {
        windows::known_folders()
    }
    #[cfg(not(windows))]
    {
        let mut folders = BTreeMap::new();
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            folders.insert("HOME".into(), home.clone());
            folders.insert("DOCUMENTS".into(), home.join("Documents"));
            folders.insert(
                "XDG_DATA_HOME".into(),
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or(home.join(".local/share")),
            );
            folders.insert(
                "XDG_CONFIG_HOME".into(),
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or(home.join(".config")),
            );
        }
        folders
    }
}
/// Durable per-user application data shared by the desktop, host and CLI.
pub fn app_data_dir() -> std::io::Result<PathBuf> {
    let folders = known_folders();
    let root = folders
        .get("LOCALAPPDATA")
        .or_else(|| folders.get("XDG_DATA_HOME"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot resolve per-user application data directory",
            )
        })?;
    Ok(root.join("SaveScummer"))
}
/// Disposable application cache, independent of Steam and game installation paths.
pub fn artwork_cache_dir() -> std::io::Result<PathBuf> {
    let folders = known_folders();
    let platform = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    cache_dir_for(
        platform,
        &folders,
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .as_deref(),
    )
}

fn cache_dir_for(
    platform: &str,
    folders: &BTreeMap<String, PathBuf>,
    xdg: Option<&Path>,
) -> std::io::Result<PathBuf> {
    let missing = || {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot resolve per-user cache directory",
        )
    };
    let root = match platform {
        "windows" => folders
            .get("LOCALAPPDATA")
            .ok_or_else(missing)?
            .join("SaveScummer/cache"),
        "macos" => folders
            .get("HOME")
            .ok_or_else(missing)?
            .join("Library/Caches/SaveScummer"),
        _ => xdg
            .filter(|p| p.is_absolute())
            .map(Path::to_path_buf)
            .or_else(|| folders.get("HOME").map(|p| p.join(".cache")))
            .ok_or_else(missing)?
            .join("SaveScummer"),
    };
    if !root.is_absolute() {
        return Err(missing());
    }
    Ok(root)
}

pub struct NativeObserver;
#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn cache_roots_follow_platform_conventions_and_ignore_relative_xdg() {
        let root = std::env::temp_dir();
        let folders = BTreeMap::from([
            ("HOME".into(), root.join("home")),
            ("LOCALAPPDATA".into(), root.join("redirected-local")),
        ]);
        assert_eq!(
            cache_dir_for("windows", &folders, None).unwrap(),
            root.join("redirected-local/SaveScummer/cache")
        );
        assert_eq!(
            cache_dir_for("macos", &folders, None).unwrap(),
            root.join("home/Library/Caches/SaveScummer")
        );
        for xdg in [None, Some(Path::new("")), Some(Path::new("relative/cache"))] {
            assert_eq!(
                cache_dir_for("linux", &folders, xdg).unwrap(),
                root.join("home/.cache/SaveScummer")
            );
        }
        assert_eq!(
            cache_dir_for("linux", &folders, Some(&root.join("xdg"))).unwrap(),
            root.join("xdg/SaveScummer")
        );
        assert!(cache_dir_for("macos", &BTreeMap::new(), None).is_err());
    }
}
/// True only when absence can be established from an accessible ancestor.
/// Missing/offline volumes and access errors do not confirm uninstallation.
pub fn confirmed_missing(path: &Path) -> bool {
    let mut ancestor = path;
    loop {
        match std::fs::metadata(ancestor) {
            Ok(metadata) => {
                let accessible = metadata.is_dir() && std::fs::read_dir(ancestor).is_ok();
                return ancestor != path && accessible;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = ancestor.parent() else {
                    return false;
                };
                ancestor = parent;
            }
            Err(_) => return false,
        }
    }
}
impl ObservationSource for NativeObserver {
    fn observe(&self) -> Result<Observation> {
        #[cfg(windows)]
        {
            windows::observe()
        }
        #[cfg(not(windows))]
        {
            Err(Error::new(
                ErrorCode::Unavailable,
                "native process/focus monitoring is currently implemented on Windows only",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn unresolved_references_keep_their_recorded_overlap_protection() {
        let tmp = tempfile::tempdir().unwrap();
        let unavailable = tmp.path().join("unavailable");
        // A loop makes canonicalization fail even though the parent is online.
        std::os::unix::fs::symlink(&unavailable, &unavailable).unwrap();
        let paths = Paths::new(vec![unavailable.clone()]);
        assert!(paths.resolve(&unavailable).is_err());
        assert_eq!(paths.reference_key(&unavailable).unwrap(), unavailable);
        assert!(paths.validate(tmp.path(), &[]).is_err());
        assert!(paths.validate(&tmp.path().join("unrelated"), &[]).is_ok());
        let paths = Paths::new(vec![]);
        assert!(
            paths
                .validate(tmp.path(), &[("other".into(), unavailable)])
                .is_err()
        );
    }
    #[test]
    fn validates_missing_paths_without_creating_them_and_compares_components() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(vec![tmp.path().join("AppData")]);
        assert!(paths.validate(tmp.path(), &[]).is_err());
        assert!(paths.validate(&tmp.path().join("AppData"), &[]).is_err());
        let game = paths
            .validate(&tmp.path().join("AppData/Game"), &[])
            .unwrap();
        assert!(!game.exists());
        assert!(
            paths
                .validate(
                    &tmp.path().join("AppData/Game2"),
                    &[("a".into(), game.clone())]
                )
                .is_ok()
        );
        assert!(
            paths
                .validate(&game.join("child"), &[("a".into(), game)])
                .is_err()
        );
    }
}
