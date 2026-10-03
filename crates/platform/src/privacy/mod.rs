//! Locations macOS guards behind a permission prompt (TCC), decided from the
//! path alone (PLAN.md, PLATFORMS). There's no public way to
//! ask whether access is granted: reading is the test, and reading is what
//! prompts. So nothing here reads inside a guarded location, except
//! [`probe`], which exists to prompt.
//!
//! Other OSes have no such locations: their table is empty.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(target_os = "macos"), path = "unsupported.rs")]
mod imp;

/// The kinds of protection macOS asks about. Containers and volumes can
/// have independent grants within the same category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Documents,
    Desktop,
    Downloads,
    IcloudDrive,
    /// Removable and network volumes under `/Volumes`.
    Volumes,
    /// Other apps' containers and group containers (macOS 14+).
    AppData,
    /// Anything inside an `.app` bundle; only writes are guarded.
    AppBundles,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Documents => "documents",
            Category::Desktop => "desktop",
            Category::Downloads => "downloads",
            Category::IcloudDrive => "icloud_drive",
            Category::Volumes => "volumes",
            Category::AppData => "app_data",
            Category::AppBundles => "app_bundles",
        }
    }

    /// How macOS names it, for UI guidance and logs.
    pub fn display_name(self) -> &'static str {
        match self {
            Category::Documents => "Documents",
            Category::Desktop => "Desktop",
            Category::Downloads => "Downloads",
            Category::IcloudDrive => "iCloud Drive",
            Category::Volumes => "removable and network volumes",
            Category::AppData => "other apps' data",
            Category::AppBundles => "app bundles",
        }
    }

    /// The System Settings pane where the user turns it on again after
    /// denying it (macOS never asks twice).
    pub fn settings_url(self) -> &'static str {
        match self {
            Category::AppBundles => "x-apple.systempreferences:com.apple.preference.security?Privacy_AppBundles",
            Category::AppData => "x-apple.systempreferences:com.apple.preference.security?Privacy",
            _ => "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders",
        }
    }
}

/// Which locations are guarded. The OS's own on macOS ([`Table::os`]); tests
/// mark fixture folders instead.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    /// Folders guarded with everything inside them.
    #[serde(default)]
    pub folders: Vec<(PathBuf, Category)>,
    /// Where volumes mount: every mount point directly inside is guarded.
    #[serde(default)]
    pub volumes: Option<PathBuf>,
    /// Whether the inside of any `.app` bundle is guarded.
    #[serde(default)]
    pub app_bundles: bool,
}

impl Table {
    /// This machine's table: macOS's guarded locations, nothing elsewhere.
    pub fn os() -> Table {
        imp::table()
    }

    /// The guarded category `path` is in, if any: placeholders already
    /// filled in, links resolved one step at a time so nothing inside a
    /// guarded location is ever touched, even to check it exists.
    pub fn category_of(&self, path: &Path) -> Option<Category> {
        self.location_of(path).map(|(category, _)| category)
    }

    /// The protection and its location, without reading inside it. Container,
    /// volume and bundle approvals must not authorize unrelated locations.
    pub fn location_of(&self, path: &Path) -> Option<(Category, PathBuf)> {
        if self.is_empty() || !path.is_absolute() {
            return None;
        }
        let (mut current, mut pending) = components(path);
        pending.reverse();
        let mut hops = 0;
        while let Some(name) = pending.pop() {
            if self.lexical(&current).is_some() {
                current.push(name);
                current.extend(pending.iter().rev());
                return self.lexical(&current);
            }
            let next = current.join(&name);
            if self.is_volume(&next) {
                return Some((Category::Volumes, next));
            }
            let Ok(meta) = fs::symlink_metadata(&next) else {
                // Missing (or unreadable): the rest can only be judged by name.
                current = next;
                continue;
            };
            if !meta.file_type().is_symlink() {
                current = next;
                continue;
            }
            hops += 1;
            let Ok(target) = fs::read_link(&next) else { return self.lexical(&next) };
            if hops > 40 {
                return self.lexical(&next);
            }
            let resolved = if target.is_absolute() { target } else { current.join(target) };
            let (root, mut again) = components(&resolved);
            again.reverse();
            pending.extend(again);
            current = root;
        }
        self.lexical(&current)
    }

    fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.volumes.is_none() && !self.app_bundles
    }

    /// The category of a path with every link already resolved.
    fn lexical(&self, path: &Path) -> Option<(Category, PathBuf)> {
        if let Some((root, category)) = self.folders.iter().find(|(root, _)| path.starts_with(root)) {
            let location = if *category == Category::AppData {
                path.strip_prefix(root).ok()?.components().next().map_or_else(|| root.clone(), |c| root.join(c))
            } else {
                root.clone()
            };
            return Some((*category, location));
        }
        if let Some(volumes) = &self.volumes
            && let Ok(rest) = path.strip_prefix(volumes)
            && let Some(first) = rest.components().next()
            && imp::is_mount_point(&volumes.join(first))
        {
            return Some((Category::Volumes, volumes.join(first)));
        }
        if self.app_bundles {
            let mut bundle = PathBuf::new();
            for component in path.components() {
                bundle.push(component);
                if component.as_os_str().to_string_lossy().to_lowercase().ends_with(".app") {
                    return Some((Category::AppBundles, bundle));
                }
            }
        }
        None
    }

    /// A volume's mount point itself: judged before it's looked at.
    fn is_volume(&self, path: &Path) -> bool {
        self.volumes.as_deref().is_some_and(|v| path.parent() == Some(v)) && imp::is_mount_point(path)
    }
}

/// `path`'s normal components, `..` applied.
fn components(path: &Path) -> (PathBuf, Vec<PathBuf>) {
    let mut root = PathBuf::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => out.push(PathBuf::from(name)),
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(_) | Component::RootDir => root.push(component.as_os_str()),
            Component::CurDir => {}
        }
    }
    (root, out)
}

/// What reading a guarded location found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Granted,
    /// macOS refused without asking: the user denied it before.
    Denied,
    /// No successful access was established (missing location or I/O failure).
    Unverified,
}

/// Reads `path` (its nearest existing folder) the way a Save would, which
/// makes macOS ask the user if it hasn't yet, and waits for the answer. For
/// app bundles it also writes and removes a file: only writes are guarded
/// there.
pub fn probe(path: &Path, category: Category) -> Access {
    let Some(folder) = path.ancestors().find(|p| fs::metadata(p).is_ok_and(|m| m.is_dir())) else {
        return Access::Unverified;
    };
    if let Err(e) = fs::read_dir(folder) {
        return access(&e);
    }
    if category == Category::AppBundles {
        let file = folder.join(format!(".savescummer-probe-{}", std::process::id()));
        match fs::write(&file, b"") {
            Ok(()) => {
                let _ = fs::remove_file(&file);
            }
            Err(e) => return access(&e),
        }
    }
    Access::Granted
}

/// A permission error (`EPERM`, "operation not permitted") is the privacy
/// refusal; another I/O failure doesn't establish access.
pub fn access(e: &io::Error) -> Access {
    if is_privacy_refusal(e) { Access::Denied } else { Access::Unverified }
}

/// Whether an error is macOS's privacy refusal. Plain file permissions give
/// `EACCES` instead.
pub fn is_privacy_refusal(e: &io::Error) -> bool {
    imp::is_privacy_refusal(e)
}

/// This build's identity, which the grants belong to: macOS forgets them
/// when the app's code signature changes, so every new build starts over.
pub fn code_identity() -> Option<String> {
    imp::code_identity()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(root: &Path) -> Table {
        Table { folders: vec![(root.join("Documents"), Category::Documents)], volumes: None, app_bundles: true }
    }

    #[test]
    fn a_path_is_judged_by_where_it_really_is() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join("Documents/Game")).unwrap();
        fs::create_dir_all(root.join("Library")).unwrap();
        let t = table(&root);
        assert_eq!(t.category_of(&root.join("Documents/Game/save.dat")), Some(Category::Documents));
        assert_eq!(t.category_of(&root.join("Documents")), Some(Category::Documents));
        assert_eq!(t.category_of(&root.join("Library/Game")), None);
        assert_eq!(t.category_of(&root.join("Library/../Documents/x")), Some(Category::Documents));
        assert_eq!(t.category_of(&root.join("Games/Foo.app/Contents/saves")), Some(Category::AppBundles));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("Documents/Game"), root.join("Library/Linked")).unwrap();
            assert_eq!(t.category_of(&root.join("Library/Linked/save.dat")), Some(Category::Documents));
        }
    }

    #[test]
    fn an_empty_table_guards_nothing() {
        assert_eq!(Table::default().category_of(Path::new("/Users/x/Documents")), None);
    }

    #[test]
    fn a_readable_folder_probes_as_granted() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(probe(&dir.path().join("missing/save"), Category::Documents), Access::Granted);
        assert_eq!(probe(dir.path(), Category::AppBundles), Access::Granted);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "the probe file is gone");
    }

    #[test]
    fn plain_permission_errors_are_not_privacy_refusals() {
        assert!(!is_privacy_refusal(&io::Error::from(io::ErrorKind::NotFound)));
        assert_eq!(access(&io::Error::from(io::ErrorKind::PermissionDenied)), Access::Unverified);
        assert_eq!(access(&io::Error::from(io::ErrorKind::NotFound)), Access::Unverified);
    }

    #[test]
    fn protected_locations_have_separate_container_and_bundle_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let containers = root.join("Containers");
        let t =
            Table { folders: vec![(containers.clone(), Category::AppData)], app_bundles: true, ..Default::default() };
        for name in ["GameOne", "GameTwo"] {
            let container = containers.join(name);
            assert_eq!(t.location_of(&container.join("Data/saves")), Some((Category::AppData, container)));
        }
        let bundle = root.join("Game.app");
        assert_eq!(t.location_of(&bundle.join("Contents/saves")), Some((Category::AppBundles, bundle)));
        #[cfg(unix)]
        {
            fs::create_dir_all(&containers).unwrap();
            std::os::unix::fs::symlink(containers.join("GameOne"), root.join("LinkedGame")).unwrap();
            assert_eq!(
                t.location_of(&root.join("LinkedGame/saves")),
                Some((Category::AppData, containers.join("GameOne")))
            );
        }
    }
}
