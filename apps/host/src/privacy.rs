//! macOS privacy permissions (PLAN-MACOS.md, PRIVACY PERMISSIONS). A prompt
//! only ever follows a user action; background work only touches locations
//! already granted.
//!
//! - The guarded locations come from the environment (the OS's table, or a
//!   test's). A path is judged by its location alone, before any read.
//! - Granted categories are remembered in the data folder for this build's
//!   code identity: a new build starts with none, as macOS does.
//! - A game whose save location (or install folder) is in a category not
//!   granted is inactive: listed, but not scanned, watched or targeted.
//! - Asking reads the location, which prompts; only first run, Scan games,
//!   adding or configuring a game and `RequestAccess` ask.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
}

#[derive(Debug, Default)]
struct Grants {
    granted: BTreeSet<Category>,
    /// Asked this run and refused: macOS won't ask again.
    denied: BTreeSet<Category>,
    /// Already notified about this run.
    notified: BTreeSet<Category>,
}

pub struct Privacy {
    table: Table,
    answers: BTreeMap<Category, PrivacyAnswer>,
    file: PathBuf,
    identity: String,
    /// No record at all: the app's first run may ask, once.
    first_run: AtomicBool,
    grants: Mutex<Grants>,
}

impl Privacy {
    pub fn load(data_dir: &Path, env: &Environment) -> Privacy {
        let file = data_dir.join(FILE);
        let identity = privacy::code_identity().unwrap_or_default();
        let saved: Option<Saved> = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok());
        let first_run = saved.is_none();
        let granted = match saved {
            Some(saved) if saved.identity == identity => saved.granted,
            Some(saved) if !saved.granted.is_empty() => {
                crate::trace(&format!(
                    "a new build: macOS forgot access to {}",
                    saved.granted.iter().map(|c| c.display_name()).collect::<Vec<_>>().join(", ")
                ));
                BTreeSet::new()
            }
            _ => BTreeSet::new(),
        };
        Privacy {
            table: env.privacy.clone(),
            answers: env.privacy_answers.clone(),
            file,
            identity,
            first_run: AtomicBool::new(first_run),
            grants: Mutex::new(Grants { granted, ..Default::default() }),
        }
    }

    fn grants(&self) -> std::sync::MutexGuard<'_, Grants> {
        self.grants.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn first_run(&self) -> bool {
        self.first_run.load(Ordering::SeqCst)
    }

    /// The first run's question was asked (or there was nothing to ask):
    /// from now on this is like any other run, this launch included.
    pub fn end_first_run(&self) {
        self.first_run.store(false, Ordering::SeqCst);
        self.save();
    }

    /// The guarded category `path` needs and doesn't have yet.
    pub fn needed(&self, path: &Path) -> Option<Category> {
        let category = self.table.category_of(path)?;
        (!self.grants().granted.contains(&category)).then_some(category)
    }

    pub fn is_denied(&self, category: Category) -> bool {
        self.grants().denied.contains(&category)
    }

    /// Reads `path` to ask macOS for its category, and waits for the answer.
    /// Returns whether access is granted now.
    pub fn ask(&self, path: &Path, category: Category) -> bool {
        crate::trace(&format!("asking for access to {} ({})", category.display_name(), path.display()));
        let access = match self.answers.get(&category) {
            Some(PrivacyAnswer::Granted) => Access::Granted,
            Some(PrivacyAnswer::Denied) => Access::Denied,
            Some(PrivacyAnswer::Hangs) => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            None => privacy::probe(path, category),
        };
        let granted = access == Access::Granted;
        {
            let mut grants = self.grants();
            if granted {
                grants.granted.insert(category);
                grants.denied.remove(&category);
            } else {
                grants.denied.insert(category);
            }
        }
        crate::trace(&format!("access to {}: {}", category.display_name(), if granted { "granted" } else { "denied" }));
        self.save();
        granted
    }

    fn revoke(&self, category: Category) -> bool {
        let removed = self.grants().granted.remove(&category);
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
        if !self.grants().granted.contains(&category) {
            return false;
        }
        for folder in path.ancestors() {
            match std::fs::read_dir(folder) {
                Ok(_) => return false,
                Err(e) if privacy::is_privacy_refusal(&e) => return self.revoke(category),
                Err(_) => continue,
            }
        }
        false
    }

    /// Categories not notified about in this run yet; marks them notified.
    fn first_notices(&self, categories: impl IntoIterator<Item = Category>) -> Vec<Category> {
        let mut grants = self.grants();
        categories.into_iter().filter(|c| grants.notified.insert(*c)).collect()
    }

    fn save(&self) {
        let saved = Saved { identity: self.identity.clone(), granted: self.grants().granted.clone() };
        let text = serde_json::to_string_pretty(&saved).expect("grants serialize");
        if let Err(e) = std::fs::write(&self.file, text) {
            crate::trace(&format!("can't record granted access in {}: {e}", self.file.display()));
        }
    }
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

/// Installed games waiting for each category, with a path that needs it.
fn waiting(inner: &Inner) -> BTreeMap<Category, (PathBuf, Vec<String>)> {
    let mut out: BTreeMap<Category, (PathBuf, Vec<String>)> = BTreeMap::new();
    for (id, derived) in &inner.derived {
        if let Some((path, category)) = &derived.access
            && inner.games.get(id).is_some_and(|g| g.installed)
        {
            out.entry(*category).or_insert_with(|| (path.clone(), Vec::new())).1.push(id.clone());
        }
    }
    out
}

/// After a scan: a user's scan (or the first run) asks for every category
/// installed games wait for; a background scan only notifies, once per
/// category per run.
pub fn after_scan(host: &Arc<Host>, user: bool) {
    let waiting = waiting(&host.lock());
    if waiting.is_empty() {
        return;
    }
    if user {
        let mut any = false;
        for (category, (path, _)) in &waiting {
            if !host.privacy.is_denied(*category) {
                any |= host.privacy.ask(path, *category);
            }
        }
        if any {
            granted(host);
        } else {
            // A denial shows as such.
            host.publish(&mut host.lock());
        }
        return;
    }
    if first_run_asks(host) {
        // The app's first run asks once it's ready (see `lib.rs`).
        return;
    }
    for category in host.privacy.first_notices(waiting.keys().copied()) {
        let games = waiting[&category].1.len();
        let text = format!(
            "SaveScummer needs access to {} for {games} game{}. Open SaveScummer to allow it.",
            category.display_name(),
            if games == 1 { "" } else { "s" }
        );
        notify(host, &text);
    }
}

/// The app's first run, opened by the user: it may ask for what the games
/// it found wait for.
pub fn first_run_asks(host: &Host) -> bool {
    host.privacy.first_run() && !host.opts.minimized
}

/// Asks for the category `game` waits for (the UI's Allow access). Blocks
/// until the user answers.
pub fn request_access(host: &Arc<Host>, game: &str) -> Result<serde_json::Value, Failure> {
    let (game_id, needed) = {
        let mut inner = host.lock();
        let game_id = host.find_game(&inner, game)?;
        crate::library::derive_one(host, &mut inner, &game_id);
        let needed = inner.derived.get(&game_id).and_then(|d| d.access.clone());
        (game_id, needed)
    };
    let Some((path, category)) = needed else {
        return Ok(serde_json::json!({ "game": game_id, "access": "granted" }));
    };
    if host.privacy.ask(&path, category) {
        granted(host);
        return Ok(serde_json::json!({ "game": game_id, "access": "granted", "category": category }));
    }
    let mut inner = host.lock();
    host.publish(&mut inner);
    Ok(serde_json::json!({
        "game": game_id,
        "access": "denied",
        "category": category,
        "settings_url": category.settings_url(),
    }))
}

/// After an operation failed: if macOS took back access to where it read,
/// the games there turn inactive and the user hears about it.
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
    after_scan(host, false);
}

/// Asks before a user-typed location is used (adding or configuring a
/// game), so the prompt shows while the form is open.
pub fn ask_for(host: &Host, path: &Path) -> Result<(), Failure> {
    let Some(category) = host.privacy.needed(path) else { return Ok(()) };
    if host.privacy.ask(path, category) {
        return Ok(());
    }
    Err(Failure::new(ErrorKind::AccessNeeded, category.as_str()).path(path))
}

/// A category was granted: every game waiting for it becomes active now,
/// and a scan resolves them again with their locations readable. A scan
/// running now (the one that asked) read without access, so it's a new one,
/// queued after it.
fn granted(host: &Arc<Host>) {
    crate::library::derive_all(host, &mut host.lock());
    host.scans.request_again(false, "access was granted");
    let watcher = host.watcher.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(watcher) = watcher.as_ref() {
        watcher.set_paths(host.env.watch_locations());
    }
    drop(watcher);
    let mut inner = host.lock();
    crate::monitoring::activate_running(&mut inner);
    host.publish(&mut inner);
}

/// The hotkeys' check: a game in front that waits for access fails, never
/// letting the keys fall through to another game, and the notice repeats.
pub fn hotkey_refusal(host: &Host) -> Option<Failure> {
    let (failure, text) = {
        let inner = host.lock();
        let front = inner.front.as_ref()?;
        let (_, category) = inner.derived.get(front)?.access.as_ref()?;
        let name = inner.games.get(front).map_or(front.as_str(), |g| g.name.as_str());
        (
            Failure::new(ErrorKind::AccessNeeded, category.as_str()).game(front),
            format!("SaveScummer needs access to {} for {name}.", category.display_name()),
        )
    };
    notify(host, &text);
    Some(failure)
}

fn notify(host: &Host, text: &str) {
    crate::trace(&format!("notification: {text}"));
    if let Some(integration) = host.integration.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        integration.notify("SaveScummer", text);
    }
}
