//! macOS privacy permissions (PLAN.md, PLATFORMS). A prompt
//! only ever follows a user action; background work only touches locations
//! already granted.
//!
//! - The guarded locations come from the environment (the OS's table, or a
//!   test's). A path is judged by its location alone, before any read.
//! - Granted scopes are remembered in the data folder for this build's
//!   code identity: a new build starts with none, as macOS does. Other
//!   apps' data is granted for the process's lifetime only (Apple's
//!   rule): it's never remembered across runs.
//! - A game whose save location (or install folder) is in a category not
//!   granted is inactive: listed, but not scanned, watched or targeted.
//! - Asking reads the location, which prompts; only first-launch setup's
//!   Allow access, Scan games, adding or configuring a game and
//!   `RequestAccess` ask.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use savescummer_core::{ErrorKind, Failure};
use savescummer_platform::privacy::{self, Access, Category, Table};
use savescummer_scanner::{Environment, PrivacyAnswer};

use crate::host::{Host, Inner};

const FILE: &str = "privacy.json";

/// What's kept in the data folder.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    identity: String,
    granted: BTreeSet<Category>,
    #[serde(default)]
    locations: BTreeSet<(Category, PathBuf)>,
}

#[derive(Debug, Default)]
struct Grants {
    granted: BTreeSet<(Category, PathBuf)>,
    /// Asked this run and refused: macOS won't ask again.
    denied: BTreeSet<(Category, PathBuf)>,
}

pub struct Privacy {
    table: Table,
    answers: BTreeMap<Category, PrivacyAnswer>,
    file: PathBuf,
    identity: String,
    grants: Mutex<Grants>,
}

/// What asking found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Granted,
    Denied,
    /// No successful access was established: missing location or I/O failure.
    Unverified,
}

impl Privacy {
    pub fn load(data_dir: &Path, env: &Environment) -> Privacy {
        let file = data_dir.join(FILE);
        let identity = privacy::code_identity().unwrap_or_default();
        let saved: Option<Saved> = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok());
        let mut granted = match saved {
            Some(saved) if saved.identity == identity => {
                let mut grants: BTreeSet<_> =
                    saved.granted.into_iter().filter(|c| !scoped(*c)).map(|c| (c, PathBuf::new())).collect();
                grants.extend(saved.locations);
                grants
            }
            Some(saved) if !saved.granted.is_empty() || !saved.locations.is_empty() => {
                let categories: BTreeSet<_> =
                    saved.granted.into_iter().chain(saved.locations.into_iter().map(|(c, _)| c)).collect();
                for category in categories {
                    crate::trace(&format!("a new build: macOS forgot access to {}", category.display_name()));
                }
                BTreeSet::new()
            }
            _ => BTreeSet::new(),
        };
        // Recorded by an earlier build, which remembered everything.
        granted.retain(|(c, _)| remembered(*c));
        Privacy {
            table: env.privacy.clone(),
            answers: env.privacy_answers.clone(),
            file,
            identity,
            grants: Mutex::new(Grants { granted, ..Default::default() }),
        }
    }

    fn grants(&self) -> std::sync::MutexGuard<'_, Grants> {
        self.grants.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The guarded category `path` needs and doesn't have yet.
    pub fn needed(&self, path: &Path) -> Option<Category> {
        let category = self.table.category_of(path)?;
        (!self.grants().granted.contains(&self.scope(path, category))).then_some(category)
    }

    /// The OS scope actually tested. Ordinary folder permissions cover the
    /// category; other containers, volumes and app bundles stay independent.
    pub fn scope(&self, path: &Path, category: Category) -> (Category, PathBuf) {
        let location = if scoped(category) {
            self.table.location_of(path).map_or_else(|| path.to_path_buf(), |(_, root)| root)
        } else {
            PathBuf::new()
        };
        (category, location)
    }

    pub fn is_denied(&self, path: &Path, category: Category) -> bool {
        self.grants().denied.contains(&self.scope(path, category))
    }

    /// Reads `path` to ask macOS for its category, and waits for the answer.
    /// Only a read inside the location counts: when nothing there exists
    /// yet, the answer is [`Answer::Unverified`] and nothing is recorded.
    pub fn ask(&self, path: &Path, category: Category) -> Answer {
        crate::trace(&format!("asking for access to {} ({})", category.display_name(), path.display()));
        let access = match self.answers.get(&category) {
            Some(PrivacyAnswer::Granted) => Access::Granted,
            Some(PrivacyAnswer::Denied) => Access::Denied,
            Some(PrivacyAnswer::Hangs) => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            None if !self.inside(path, category) => {
                crate::trace(&format!("access to {}: nothing there to read yet", category.display_name()));
                return Answer::Unverified;
            }
            None => privacy::probe(path, category),
        };
        let granted = access == Access::Granted;
        if access == Access::Unverified {
            crate::trace(&format!("access to {}: couldn't verify it", category.display_name()));
            return Answer::Unverified;
        }
        let scope = self.scope(path, category);
        {
            let mut grants = self.grants();
            if granted {
                grants.granted.insert(scope.clone());
                grants.denied.remove(&scope);
            } else {
                grants.denied.insert(scope);
            }
        }
        crate::trace(&format!("access to {}: {}", category.display_name(), if granted { "granted" } else { "denied" }));
        self.save();
        if granted { Answer::Granted } else { Answer::Denied }
    }

    /// Whether the folder a probe of `path` reads (its nearest existing
    /// one) is inside the guarded location: reading an ancestor outside it
    /// proves nothing. Other apps' data is guarded per container, so its
    /// parent folders don't count either.
    fn inside(&self, path: &Path, category: Category) -> bool {
        let Some(folder) = path.ancestors().find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_dir())) else {
            return false;
        };
        let root = self.table.folders.iter().any(|(root, c)| *c == Category::AppData && root == folder);
        self.table.category_of(folder) == Some(category) && !(category == Category::AppData && root)
    }

    fn revoke(&self, path: &Path, category: Category) -> bool {
        let removed = self.grants().granted.remove(&self.scope(path, category));
        if removed {
            crate::trace(&format!("access to {} was taken back", category.display_name()));
            self.save();
        }
        removed
    }

    /// Whether macOS took back access to `path`'s category (the user turned
    /// it off in System Settings): a read there is refused. If so, the grant
    /// is forgotten. Only reads where access was granted, so it never asks.
    pub fn taken_back(&self, path: &Path) -> bool {
        let Some(category) = self.table.category_of(path) else { return false };
        if !self.grants().granted.contains(&self.scope(path, category)) {
            return false;
        }
        for folder in path.ancestors() {
            match std::fs::read_dir(folder) {
                Ok(_) => return false,
                Err(e) if privacy::is_privacy_refusal(&e) => return self.revoke(path, category),
                Err(_) => continue,
            }
        }
        false
    }

    fn save(&self) {
        let grants = self.grants();
        let granted = grants.granted.iter().filter(|(c, _)| !scoped(*c)).map(|(c, _)| *c).collect();
        let locations = grants.granted.iter().filter(|(c, _)| scoped(*c) && remembered(*c)).cloned().collect();
        let saved = Saved { identity: self.identity.clone(), granted, locations };
        drop(grants);
        let text = serde_json::to_string_pretty(&saved).expect("grants serialize");
        if let Err(e) = std::fs::write(&self.file, text) {
            crate::trace(&format!("can't record granted access in {}: {e}", self.file.display()));
        }
    }
}

/// Whether a grant outlives the process. macOS grants other apps' data for
/// the process's lifetime.
fn remembered(category: Category) -> bool {
    category != Category::AppData
}

fn scoped(category: Category) -> bool {
    matches!(category, Category::Volumes | Category::AppData | Category::AppBundles)
}

/// Installs the privacy guard: file helpers leave locations not granted yet
/// alone. Reading inside an app bundle is fine: macOS guards only writes
/// there, and a game writing its saves there waits for access anyway
/// (`game_needs`).
pub fn guard(privacy: &Arc<Privacy>) {
    let privacy = Arc::downgrade(privacy);
    savescummer_snapshots::set_guard(move |path| {
        privacy.upgrade().and_then(|p| p.needed(path)).is_some_and(|c| c != Category::AppBundles)
    });
}

/// The category a game waits for, with the path that needs it: its save
/// locations, and its install folders (a Steam library on an external disk).
/// Apps' own bundles only guard writes, so an install folder inside one
/// needs nothing.
pub fn game_needs(host: &Host, targets: &[PathBuf], install_dirs: &[PathBuf]) -> Option<(PathBuf, Category)> {
    let target = targets.iter().find_map(|p| host.privacy.needed(p).map(|c| (p.clone(), c)));
    target.or_else(|| {
        install_dirs
            .iter()
            .find_map(|p| host.privacy.needed(p).filter(|c| *c != Category::AppBundles).map(|c| (p.clone(), c)))
    })
}

/// Installed games waiting for access. Keep separate locations so one
/// container or volume's approval never hides another one's request.
pub fn waiting(inner: &Inner) -> Vec<(Category, PathBuf)> {
    let mut out = BTreeSet::new();
    for (id, derived) in &inner.derived {
        if let Some((path, category)) = &derived.access
            && inner.games.get(id).is_some_and(|g| g.installed)
        {
            out.insert((*category, path.clone()));
        }
    }
    out.into_iter().collect()
}

/// Known discovery locations first, then saves of installed games. Unknown
/// protected folders are never searched just to make a permission request.
pub fn onboarding_waiting(host: &Host) -> Vec<(Category, PathBuf)> {
    let mut locations = Vec::new();
    let mut seen = BTreeSet::new();
    let discovery = host.env.discovery_locations().into_iter().filter_map(|path| {
        host.privacy.needed(&path).filter(|c| *c != Category::AppBundles).map(|category| (category, path))
    });
    let games = waiting(&host.lock());
    for (category, path) in discovery.chain(games) {
        if seen.insert(host.privacy.scope(&path, category)) {
            locations.push((category, path));
        }
    }
    locations
}

/// After a user's scan, ask for the scopes installed games wait for. Background scans leave access guidance in the app's UI.
pub fn after_scan(host: &Arc<Host>, user: bool) {
    if !user {
        return;
    }
    let waiting = waiting(&host.lock());
    if waiting.is_empty() {
        return;
    }
    let mut any = false;
    for (category, path) in &waiting {
        if host.privacy.needed(path).is_some() && !host.privacy.is_denied(path, *category) {
            any |= host.privacy.ask(path, *category) == Answer::Granted;
        }
    }
    if any {
        granted(host);
    } else {
        // A denial shows as such.
        host.publish(&mut host.lock());
    }
}

/// Asks for the category `game` waits for (the UI's Allow access). Blocks
/// until the user answers.
pub fn request_access(host: &Arc<Host>, game: &str) -> Result<serde_json::Value, Failure> {
    let game_id = host.find_game(&host.lock(), game)?;
    crate::library::refresh_game(host, &game_id)?;
    let (game_id, needed) = {
        let inner = host.lock();
        let needed = host
            .privacy
            .needed(&inner.store)
            .map(|category| (inner.store.clone(), category))
            .or_else(|| {
                inner.blocked.get(&game_id).and_then(|f| {
                    f.paths
                        .iter()
                        .find_map(|p| host.privacy.needed(Path::new(p)).map(|category| (PathBuf::from(p), category)))
                })
            })
            .or_else(|| inner.derived.get(&game_id).and_then(|d| d.access.clone()));
        (game_id, needed)
    };
    let Some((path, category)) = needed else {
        return Ok(serde_json::json!({ "game": game_id, "access": "granted" }));
    };
    let answer = host.privacy.ask(&path, category);
    if answer == Answer::Granted {
        granted(host);
        return Ok(serde_json::json!({ "game": game_id, "access": "granted", "category": category }));
    }
    let mut inner = host.lock();
    host.publish(&mut inner);
    if answer == Answer::Unverified {
        return Ok(serde_json::json!({ "game": game_id, "access": "unverified", "category": category }));
    }
    Ok(serde_json::json!({
        "game": game_id,
        "access": "denied",
        "category": category,
        "settings_url": category.settings_url(),
    }))
}

/// After an operation failed: if macOS took back access to where it read,
/// the games there turn inactive and their UI guidance updates.
pub fn after_failure(host: &Arc<Host>, failure: &Failure) {
    let taken = failure.paths.iter().any(|p| host.privacy.taken_back(Path::new(p)));
    if !taken {
        return;
    }
    {
        let mut inner = host.lock();
        crate::library::derive_all(host, &mut inner);
        host.publish(&mut inner);
    }
}

/// Asks before a user-typed location is used (adding or configuring a
/// game), so the prompt shows while the form is open. A location with
/// nothing to read yet is accepted: the game waits for access until there is.
pub fn ask_for(host: &Host, path: &Path) -> Result<(), Failure> {
    let Some(category) = host.privacy.needed(path) else { return Ok(()) };
    if host.privacy.ask(path, category) != Answer::Denied {
        return Ok(());
    }
    Err(Failure::new(ErrorKind::AccessNeeded, category.as_str()).path(path))
}

/// A category was granted: every game waiting for it becomes active now,
/// and a scan resolves them again with their locations readable. A scan
/// running now (the one that asked) read without access, so it's a new one,
/// queued after it.
pub fn granted(host: &Arc<Host>) -> u64 {
    {
        let mut inner = host.lock();
        crate::library::derive_all(host, &mut inner);
        let ids: Vec<_> = inner.games.keys().cloned().collect();
        for id in ids {
            host.refresh_cache(&mut inner, &id);
            host.bump_history(&mut inner, &id);
        }
    }
    let scan = host.scans.request_again(false, "access was granted");
    let watcher = host.watcher.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(watcher) = watcher.as_ref() {
        watcher.set_paths(host.env.watch_locations());
    }
    drop(watcher);
    let mut inner = host.lock();
    crate::monitoring::activate_running(&mut inner);
    host.publish(&mut inner);
    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn approving_one_drive_does_not_approve_another_and_container_grants_expire() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let usb = root.join("USB");
        let network = root.join("Network");
        let containers = root.join("Containers");
        let one = containers.join("One/saves");
        let two = containers.join("Two/saves");
        let env: Environment = serde_json::from_value(json!({
            "platform": "macos", "folders": {},
            "privacy": { "folders": [[usb, "volumes"], [network, "volumes"], [containers, "app_data"]] },
            "privacy_answers": { "volumes": "granted", "app_data": "granted" }
        }))
        .unwrap();
        let privacy = Privacy::load(&root, &env);
        assert_eq!(privacy.ask(&usb, Category::Volumes), Answer::Granted);
        assert_eq!(privacy.needed(&usb.join("Steam/steamapps")), None);
        assert_eq!(privacy.needed(&network), Some(Category::Volumes));
        assert_eq!(privacy.ask(&one, Category::AppData), Answer::Granted);
        assert_eq!(privacy.needed(&one), None);
        assert_eq!(privacy.needed(&two), Some(Category::AppData));
        let restarted = Privacy::load(&root, &env);
        assert_eq!(restarted.needed(&usb), None);
        assert_eq!(restarted.needed(&network), Some(Category::Volumes));
        assert_eq!(restarted.needed(&one), Some(Category::AppData));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_is_unverified_and_never_cached_as_granted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let folder = root.join("Documents");
        std::fs::create_dir(&folder).unwrap();
        let env: Environment = serde_json::from_value(json!({
            "platform": "macos", "folders": {}, "privacy": { "folders": [[folder, "documents"]] }
        }))
        .unwrap();
        let privacy = Privacy::load(&root, &env);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o0)).unwrap();
        let read = std::fs::read_dir(&folder);
        let answer = privacy.ask(&folder, Category::Documents);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        if read.is_err() {
            // A root test runner can read regardless of mode.
            assert_eq!(answer, Answer::Unverified);
            assert_eq!(privacy.needed(&folder), Some(Category::Documents));
        }
    }
}
