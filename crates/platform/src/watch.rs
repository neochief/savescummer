//! Watching store locations (Steam `steamapps` folders, `libraryfolders.vdf`,
//! the Epic manifests folder...) so a newly installed game shows up without a
//! manual scan.
//!
//! Folders are watched non-recursively. A single file is watched through its
//! parent folder, filtered to that file name. A burst of changes causes one
//! callback, [`DEBOUNCE`] after the last change.
//!
//! On Windows the watcher also takes registry keys (the uninstall keys and
//! the GOG games key); their changes are debounced together with folder
//! changes.
//!
//! Everything, including opening the OS watches, runs on one worker thread:
//! opening a watch on a sleeping disk or an offline share can take seconds,
//! and the caller must never wait for that.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher as _};

#[cfg(windows)]
mod registry;

/// Quiet time after the last change before `on_change` runs.
pub const DEBOUNCE: Duration = Duration::from_secs(2);

enum Msg {
    SetPaths(Vec<PathBuf>),
    Changed,
    Stop,
}

/// A registry hive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

/// A registry key watched with its subkeys.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegistryKey {
    pub hive: Hive,
    pub path: String,
}

/// Watches a set of paths; see the module docs.
pub struct Watcher {
    tx: Sender<Msg>,
    #[cfg(windows)]
    registry: Option<registry::KeyWatcher>,
}

impl Watcher {
    pub fn new(on_change: Box<dyn Fn() + Send + 'static>) -> Watcher {
        let (tx, rx) = mpsc::channel();
        let events = tx.clone();
        #[cfg(windows)]
        let registry = registry::KeyWatcher::new(tx.clone());
        let _ = std::thread::Builder::new().name("savescummer-watch".into()).spawn(move || run(rx, events, on_change));
        Watcher {
            tx,
            #[cfg(windows)]
            registry,
        }
    }

    /// Replaces the watched set. Paths that don't exist are skipped; pass
    /// them again later (e.g. after a library appears) to pick them up.
    pub fn set_paths(&self, paths: Vec<PathBuf>) {
        let _ = self.tx.send(Msg::SetPaths(paths));
    }

    /// Replaces the watched registry keys. Keys that don't exist are skipped;
    /// pass them again later to pick them up. Other platforms ignore this.
    pub fn set_registry_keys(&self, keys: Vec<RegistryKey>) {
        #[cfg(windows)]
        if let Some(registry) = &self.registry {
            registry.set_keys(keys);
        }
        #[cfg(not(windows))]
        let _ = keys;
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // The worker drops the OS watches and exits; no need to wait for it.
        let _ = self.tx.send(Msg::Stop);
    }
}

/// Which changes count: everything in a watched folder, or only named files
/// in a folder watched on behalf of single files.
#[derive(Default)]
struct Filter {
    whole: HashSet<PathBuf>,
    files: HashMap<PathBuf, Vec<std::ffi::OsString>>,
}

impl Filter {
    fn build(paths: &[PathBuf]) -> Filter {
        let mut filter = Filter::default();
        for path in paths {
            if path.is_dir() {
                filter.whole.insert(path.clone());
            } else if path.is_file()
                && let (Some(parent), Some(name)) = (path.parent(), path.file_name())
            {
                filter.files.entry(parent.to_path_buf()).or_default().push(name.to_owned());
            }
            // Missing: skipped until a later set_paths.
        }
        // A folder watched as a whole already covers its files.
        filter.files.retain(|dir, _| !filter.whole.contains(dir));
        filter
    }

    fn dirs(&self) -> impl Iterator<Item = &PathBuf> {
        self.whole.iter().chain(self.files.keys())
    }

    fn matches(&self, path: &Path) -> bool {
        let Some(parent) = path.parent() else {
            return true;
        };
        if self.whole.contains(parent) || self.whole.contains(path) {
            return true;
        }
        let Some(name) = path.file_name() else {
            return true;
        };
        self.files.get(parent).is_some_and(|names| names.iter().any(|n| same_name(n, name)))
    }
}

/// File names compare case-insensitively where the file system does.
fn same_name(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn run(rx: mpsc::Receiver<Msg>, events: Sender<Msg>, on_change: Box<dyn Fn() + Send + 'static>) {
    let filter = Arc::new(Mutex::new(Filter::default()));
    let handler_filter = Arc::clone(&filter);
    let handler = move |result: notify::Result<notify::Event>| {
        let relevant = match result {
            // Overflow or an OS error: something changed, we don't know what.
            Err(_) => true,
            Ok(event) => {
                event.need_rescan()
                    || event.paths.is_empty()
                    || (!matches!(event.kind, EventKind::Access(_))
                        && event
                            .paths
                            .iter()
                            .any(|p| handler_filter.lock().unwrap_or_else(|e| e.into_inner()).matches(p)))
            }
        };
        if relevant {
            let _ = events.send(Msg::Changed);
        }
    };
    // Without an OS watcher we can't watch anything; periodic and focus scans
    // remain the safety net.
    let Ok(mut os) = notify::recommended_watcher(handler) else {
        return;
    };
    let mut watched: Vec<PathBuf> = Vec::new();
    let mut due: Option<Instant> = None;

    loop {
        let msg = match due {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match msg {
            Ok(Msg::Changed) => due = Some(Instant::now() + DEBOUNCE),
            Ok(Msg::SetPaths(paths)) => {
                for dir in watched.drain(..) {
                    let _ = os.unwatch(&dir);
                }
                let next = Filter::build(&paths);
                for dir in next.dirs() {
                    if os.watch(dir, RecursiveMode::NonRecursive).is_ok() {
                        watched.push(dir.clone());
                    }
                }
                *filter.lock().unwrap_or_else(|e| e.into_inner()) = next;
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                due = None;
                // A panicking callback must not end watching for good.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&on_change));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting() -> (Arc<AtomicUsize>, Watcher) {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let watcher = Watcher::new(Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        (count, watcher)
    }

    /// Lets the worker open its watches before the test changes anything.
    fn settle() {
        std::thread::sleep(Duration::from_millis(300));
    }

    fn wait_past_debounce() {
        std::thread::sleep(DEBOUNCE + Duration::from_millis(1500));
    }

    #[test]
    fn a_burst_causes_one_callback_and_a_later_change_another() {
        let dir = tempfile::tempdir().unwrap();
        let (count, watcher) = counting();
        watcher.set_paths(vec![dir.path().to_path_buf()]);
        settle();

        for i in 0..5 {
            std::fs::write(dir.path().join(format!("f{i}.acf")), "x").unwrap();
        }
        std::thread::sleep(Duration::from_millis(1000));
        assert_eq!(count.load(Ordering::SeqCst), 0, "debounced, not immediate");
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 1);

        std::fs::write(dir.path().join("f0.acf"), "changed").unwrap();
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn set_paths_moves_the_watch_and_missing_paths_are_ignored() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let (count, watcher) = counting();
        watcher.set_paths(vec![old.path().to_path_buf()]);
        settle();
        watcher.set_paths(vec![
            new.path().to_path_buf(),
            new.path().join("does-not-exist"),
            PathBuf::from("Z:\\surely\\missing\\steamapps"),
        ]);
        settle();

        std::fs::write(old.path().join("ignored.acf"), "x").unwrap();
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 0, "old folder no longer watched");

        std::fs::write(new.path().join("seen.acf"), "x").unwrap();
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_single_file_is_watched_through_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        let vdf = dir.path().join("libraryfolders.vdf");
        std::fs::write(&vdf, "a").unwrap();
        let (count, watcher) = counting();
        watcher.set_paths(vec![vdf.clone()]);
        settle();

        std::fs::write(dir.path().join("unrelated.txt"), "x").unwrap();
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 0, "other files are filtered out");

        std::fs::write(&vdf, "b").unwrap();
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}

#[cfg(all(test, windows))]
mod registry_tests {
    use super::registry::scratch::{ScratchKey, set_value};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting() -> (Arc<AtomicUsize>, Watcher) {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let watcher = Watcher::new(Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        (count, watcher)
    }

    fn wait_past_debounce() {
        std::thread::sleep(DEBOUNCE + Duration::from_millis(1500));
    }

    #[test]
    fn a_new_subkey_or_value_is_a_change_and_missing_keys_wait_for_later() {
        let uninstall = ScratchKey::new("uninstall");
        let other = ScratchKey::new("other");
        let (count, watcher) = counting();
        let missing = format!(r"{}\NotYet", other.path);
        watcher.set_registry_keys(vec![
            RegistryKey { hive: Hive::CurrentUser, path: uninstall.path.clone() },
            RegistryKey { hive: Hive::CurrentUser, path: missing.clone() },
        ]);
        std::thread::sleep(Duration::from_millis(300));

        // An installer adds its entry: one debounced change for the burst.
        let entry = format!(r"{}\{{ABC}}_is1", uninstall.path);
        set_value(&entry, "DisplayName", "Game");
        set_value(&entry, "InstallLocation", r"C:\Games\Game");
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // Still armed after firing.
        set_value(&entry, "DisplayVersion", "1.1");
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 2);

        // A key that didn't exist when set is picked up by the next set.
        set_value(&missing, "x", "1");
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 2, "not watched yet");
        watcher.set_registry_keys(vec![RegistryKey { hive: Hive::CurrentUser, path: missing.clone() }]);
        std::thread::sleep(Duration::from_millis(300));
        set_value(&missing, "y", "2");
        set_value(&entry, "ignored", "now unwatched");
        wait_past_debounce();
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }
}
