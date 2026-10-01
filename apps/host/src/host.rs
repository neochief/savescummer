//! The host's shared state: games, running operations, the ACTIVE STACK and
//! caches, behind one lock, plus the database and the published summary.
//!
//! Lock order is always `inner` before `db`. File work never holds either.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Instant;

use savescummer_catalog::Bundle;
use savescummer_core::common::{Commonality, commonality};
use savescummer_core::stack::ActiveStack;
use savescummer_core::{ErrorKind, Failure, Filter, Presence};
use savescummer_ipc::{
    AccessInfo, CheckpointBrief, EventBody, GameKind, GameSummary, Operation, Phase, ScanInfo, SettingsInfo, State,
    StoreInfo,
};
use savescummer_platform::integration::{Shortcut, Shortcuts, shortcut_text};
use savescummer_scanner::Environment;
use savescummer_storage::{self as db, CheckpointRow, Storage};

use crate::model::{Derived, Game};
use crate::options::Options;
use crate::policy::{Action, Facts};

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

pub struct CatalogState {
    pub bundle: Arc<Bundle>,
    /// `embedded`, `downloaded` or `file`.
    pub source: String,
    /// When the host last looked for a newer catalog.
    pub checked_at: Option<String>,
    /// Why the last look failed.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UiReport {
    pub focused: bool,
    pub visible: bool,
    pub selected: Option<String>,
    pub capturing_shortcut: bool,
}

/// A Delete waiting or running.
#[derive(Debug, Clone)]
pub struct PendingDelete {
    pub op: Operation,
}

#[derive(Debug, Clone, Default)]
pub struct GameCache {
    pub latest: Option<CheckpointBrief>,
    pub has_history: bool,
    pub size: u64,
    pub leftover_size: u64,
    pub history_version: u64,
    pub labels_version: u64,
}

pub struct Inner {
    pub phase: Phase,
    pub games: BTreeMap<String, Game>,
    pub derived: HashMap<String, Derived>,
    pub caches: HashMap<String, GameCache>,
    pub stack: ActiveStack,
    /// When the user last switched to each game, as saved in the database.
    /// Orders the library.
    pub last_focused: HashMap<String, String>,
    /// Running games' current session ids.
    pub sessions: HashMap<String, String>,
    /// Running games' processes, as the monitor last saw them.
    pub processes: BTreeMap<String, Vec<u32>>,
    pub processes_observed: bool,
    /// The game in front, as the monitor last saw it, even one waiting for
    /// access (which is never on the stack).
    pub front: Option<String>,
    pub busy: HashMap<String, Operation>,
    pub blocked: HashMap<String, Failure>,
    /// Unreadable recorded recovery paths, checked outside the shared state lock.
    pub recovery_unavailable: HashMap<String, Failure>,
    pub last_results: HashMap<String, Operation>,
    pub notices: HashMap<String, String>,
    pub deletes: BTreeMap<String, PendingDelete>,
    /// Every operation of this host run, by id.
    pub ops: HashMap<String, Operation>,
    pub scan: ScanInfo,
    pub ui: UiReport,
    /// Open connections that identified themselves as the UI.
    pub ui_connections: usize,
    /// When the host last started a UI that hasn't connected yet.
    pub ui_started: Option<Instant>,
    pub last_focus_scan: Option<Instant>,
    pub store: PathBuf,
    pub store_available: bool,
    pub store_moving: bool,
    pub play_sounds: bool,
    pub flush_old: bool,
    pub shortcuts: Shortcuts,
    pub launch_on_startup: bool,
    /// macOS: turned off in System Settings, where only the user can turn
    /// it on again.
    pub launch_needs_approval: bool,
    pub revision: u64,
    /// Hotkey operations that may need a failure notification.
    pub hotkey_ops: HashSet<String>,
    /// Cached art by Steam app id.
    pub artwork: HashMap<u64, savescummer_ipc::Artwork>,
}

pub struct Host {
    pub opts: Options,
    pub data_dir: PathBuf,
    pub env: Environment,
    pub instance: String,
    pub endpoint: String,
    pub catalog: RwLock<CatalogState>,
    pub db: Mutex<Storage>,
    pub inner: Mutex<Inner>,
    pub state_tx: tokio::sync::watch::Sender<Arc<State>>,
    pub events_tx: tokio::sync::broadcast::Sender<EventBody>,
    pub sounds: Option<savescummer_platform::sounds::Player>,
    pub integration: Mutex<Option<savescummer_platform::integration::Integration>>,
    /// Requests for an artwork pass; see [`crate::artwork`].
    pub artwork_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    pub shutdown: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    pub scans: crate::scan::ScanQueue,
    pub monitor_dirty: std::sync::atomic::AtomicBool,
    pub watcher: Mutex<Option<savescummer_platform::watch::Watcher>>,
    pub privacy: Arc<crate::privacy::Privacy>,
    crash_at: Option<(String, usize)>,
}

impl Host {
    pub fn new(
        opts: Options,
        data_dir: PathBuf,
        env: Environment,
        endpoint: String,
        catalog: CatalogState,
        storage: Storage,
        inner: Inner,
    ) -> Arc<Host> {
        let instance = new_id("host");
        let (state_tx, _) = tokio::sync::watch::channel(Arc::new(placeholder_state(&instance)));
        let (events_tx, _) = tokio::sync::broadcast::channel(64);
        let crash_at = std::env::var("SAVESCUMMER_TEST_CRASH_AT").ok().and_then(|spec| {
            let (name, n) = spec.rsplit_once(':').unwrap_or((spec.as_str(), "1"));
            Some((name.to_string(), n.parse().ok()?))
        });
        let sounds = (!opts.no_integrations).then(savescummer_platform::sounds::Player::new);
        let privacy = Arc::new(crate::privacy::Privacy::load(&data_dir, &env));
        Arc::new(Host {
            opts,
            data_dir,
            env,
            instance,
            endpoint,
            catalog: RwLock::new(catalog),
            db: Mutex::new(storage),
            inner: Mutex::new(inner),
            state_tx,
            events_tx,
            sounds,
            integration: Mutex::new(None),
            artwork_tx: Mutex::new(None),
            shutdown: Mutex::new(None),
            scans: crate::scan::ScanQueue::default(),
            monitor_dirty: std::sync::atomic::AtomicBool::new(true),
            watcher: Mutex::new(None),
            privacy,
            crash_at,
        })
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn db(&self) -> MutexGuard<'_, Storage> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn bundle(&self) -> Arc<Bundle> {
        self.catalog.read().unwrap_or_else(|e| e.into_inner()).bundle.clone()
    }

    /// Crashes the process at a named point when a test asks for it
    /// (`SAVESCUMMER_TEST_CRASH_AT=load.set_aside:2`).
    pub fn crash_point(&self, name: &str, n: usize) {
        // SAVESCUMMER_TEST_DELAY_AT=saved.copy:1:1500 sleeps there instead,
        // so tests can hold an operation open.
        if let Ok(spec) = std::env::var("SAVESCUMMER_TEST_DELAY_AT") {
            let parts: Vec<&str> = spec.split(':').collect();
            if parts.len() == 3 && parts[0] == name && parts[1].parse() == Ok(n) {
                std::thread::sleep(std::time::Duration::from_millis(parts[2].parse().unwrap_or(0)));
            }
        }
        if let Some((point, at)) = &self.crash_at
            && point == name
            && *at == n
        {
            crate::trace(&format!("test crash at {name}:{n}"));
            hard_exit();
        }
    }

    /// Publishes a new state to every watcher.
    pub fn publish(&self, inner: &mut Inner) {
        inner.revision += 1;
        let state = self.build_state(inner);
        self.state_tx.send_replace(Arc::new(state));
    }

    pub fn current_state(&self) -> Arc<State> {
        self.state_tx.borrow().clone()
    }

    pub fn build_state(&self, inner: &Inner) -> State {
        let mut installed: Vec<&Game> = inner.games.values().filter(|g| g.installed).collect();
        sort_library(&mut installed, inner.stack.entries(), &inner.last_focused);
        let games = installed.into_iter().map(|g| self.summary(inner, g)).collect();
        let catalog = self.catalog.read().unwrap_or_else(|e| e.into_inner());
        State {
            instance: self.instance.clone(),
            revision: inner.revision,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            phase: inner.phase,
            settings: SettingsInfo {
                play_sounds: inner.play_sounds,
                save_shortcut: shortcut_text(inner.shortcuts[0]),
                load_shortcut: shortcut_text(inner.shortcuts[1]),
                launch_on_startup: inner.launch_on_startup,
                launch_on_startup_available: savescummer_platform::autostart::available(),
                launch_on_startup_needs_approval: inner.launch_needs_approval,
                flush_old_checkpoints: inner.flush_old,
                checkpoint_store: self.portable().contract(&inner.store),
            },
            store: StoreInfo { path: self.portable().contract(&inner.store), available: inner.store_available },
            scan: inner.scan.clone(),
            active_stack: inner.stack.entries().to_vec(),
            hotkey_target: hotkey_target(inner).map(|(g, _)| g),
            catalog_revision: catalog.bundle.source.revision.clone(),
            games,
            deletes: inner.deletes.values().map(|delete| delete.op.clone()).collect(),
        }
    }

    fn summary(&self, inner: &Inner, game: &Game) -> GameSummary {
        let cache = inner.caches.get(&game.id).cloned().unwrap_or_default();
        let derived = inner.derived.get(&game.id).cloned().unwrap_or_default();
        let facts = self.action_facts(inner, &game.id, None);
        GameSummary {
            id: game.id.clone(),
            name: game.name.clone(),
            kind: game.kind.clone(),
            catalog_id: game.catalog_id.clone(),
            install_tag: game.install_tag.clone(),
            store: game.store().map(str::to_string),
            installed: game.installed,
            running: inner.stack.contains(&game.id),
            can_play: game.installed && (game.steam_launch_id().is_some() || game.main_executable().is_some()),
            can_close: inner.stack.contains(&game.id)
                && inner.processes.get(&game.id).is_some_and(|pids| !pids.is_empty())
                && game.expert_mode,
            expert_mode: game.expert_mode,
            info: game.info.clone(),
            executable: game.main_executable().map(|p| self.portable().contract(&p)),
            executable_overridden: game.executable.is_some(),
            save: facts.availability(Action::Save),
            load: facts.availability(Action::Load),
            restore: facts.availability(Action::Restore),
            delete: facts.availability(Action::Delete),
            flush: facts.availability(Action::Flush),
            configure: facts.availability(Action::Configure),
            retry: facts.availability(Action::Retry),
            guidance: facts.guidance(),
            config_error: derived.active.as_ref().err().cloned(),
            access: derived.access.as_ref().map(|(_, category)| AccessInfo {
                category: category.as_str().to_string(),
                denied: self.privacy.is_denied(*category),
                settings_url: category.settings_url().to_string(),
            }),
            latest: cache.latest.clone(),
            has_history: cache.has_history,
            checkpoints_size: inner.store_available.then_some(cache.size + cache.leftover_size),
            busy: inner.busy.get(&game.id).cloned(),
            blocked: inner.blocked.get(&game.id).cloned(),
            last_result: inner.last_results.get(&game.id).cloned(),
            notice: inner.notices.get(&game.id).cloned(),
            labels_version: cache.labels_version,
            history_version: cache.history_version,
            artwork: crate::artwork::steam_app(self, game.catalog_id.as_deref())
                .and_then(|app| inner.artwork.get(&app).cloned()),
        }
    }

    /// Re-reads a game's cached summary facts from the database: the latest
    /// usable checkpoint, whether it has history, the checkpoint size.
    pub fn refresh_cache(&self, inner: &mut Inner, game_id: &str) {
        let Some(game) = inner.games.get(game_id) else { return };
        let current = current_pairs(inner, game_id);
        let ci = self.env.case_insensitive();
        let storage = self.db();
        let checkpoints = db::existing_checkpoints(storage.conn(), &game.id).unwrap_or_default();
        let has_history = db::has_visible_history(storage.conn(), &game.id).unwrap_or(false);
        drop(storage);
        let size: u64 = checkpoints.iter().map(|c| c.size).sum();
        let latest = latest_usable(&checkpoints, &current, ci).map(|c| CheckpointBrief {
            id: c.id.clone(),
            label: c.label.clone(),
            created_at: c.created_at.clone(),
        });
        let cache = inner.caches.entry(game_id.to_string()).or_default();
        cache.latest = latest;
        cache.has_history = has_history;
        cache.size = size;
    }

    pub fn bump_history(&self, inner: &mut Inner, game_id: &str) {
        inner.caches.entry(game_id.to_string()).or_default().history_version += 1;
    }

    pub fn bump_labels(&self, inner: &mut Inner, game_id: &str) {
        inner.caches.entry(game_id.to_string()).or_default().labels_version += 1;
        let _ = self.events_tx.send(EventBody::Labels { game: game_id.to_string() });
    }

    /// Writes and reads paths the way users see and keep them.
    pub fn portable(&self) -> crate::portable::Portable {
        crate::portable::Portable::new(&self.env.folders, self.env.case_insensitive())
    }

    /// Finds a game by id, or by a name matching exactly one game.
    pub fn find_game(&self, inner: &Inner, selector: &str) -> Result<String, Failure> {
        if inner.games.contains_key(selector) {
            return Ok(selector.to_string());
        }
        let matches: Vec<&Game> = inner
            .games
            .values()
            .filter(|g| {
                g.installed && (g.name.eq_ignore_ascii_case(selector) || g.catalog_id.as_deref() == Some(selector))
            })
            .collect();
        match matches.as_slice() {
            [one] => Ok(one.id.clone()),
            [] => Err(Failure::new(ErrorKind::NotFound, format!("no game {selector:?}"))),
            _ => Err(Failure::new(
                ErrorKind::InvalidRequest,
                format!(
                    "{selector:?} matches several games: {}",
                    matches.iter().map(|g| g.id.as_str()).collect::<Vec<_>>().join(", ")
                ),
            )),
        }
    }

    /// The game's folder in the checkpoint store.
    pub fn game_store_dir(&self, inner: &Inner, game_id: &str) -> Option<PathBuf> {
        inner.games.get(game_id).map(|g| inner.store.join(g.store_folder()))
    }

    pub fn request_shutdown(&self) {
        if let Some(tx) = self.shutdown.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(());
        }
    }
}

/// Current targets as (real root, filter) pairs for the "in common" rule.
pub fn current_pairs(inner: &Inner, game_id: &str) -> Vec<(PathBuf, Filter)> {
    match inner.derived.get(game_id).map(|d| &d.active) {
        Some(Ok(targets)) => targets.iter().map(|t| (t.root.clone(), t.filter.clone())).collect(),
        _ => Vec::new(),
    }
}

/// The newest usable saved checkpoint: ordered by save time, ties broken by
/// registration order. Recovery checkpoints are never picked.
pub fn latest_usable<'a>(
    checkpoints: &'a [CheckpointRow],
    current: &[(PathBuf, Filter)],
    ci: bool,
) -> Option<&'a CheckpointRow> {
    checkpoints
        .iter()
        .filter(|c| c.kind == "saved" && c.state == "ok")
        .filter(|c| commonality(&c.targets, current, ci) != Commonality::None)
        .max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.seq.cmp(&b.seq)))
}

/// The game runs and writes its progress only when it exits, so nothing on
/// disk is worth a Save, and a Load would be overwritten when it exits.
pub fn exit_first(inner: &Inner, game_id: &str) -> bool {
    inner.games.get(game_id).is_some_and(|g| !g.expert_mode)
        && (inner.stack.contains(game_id) || inner.processes.contains_key(game_id))
}

impl Host {
    /// Gather cheap observations only. The policy itself performs no I/O.
    pub fn action_facts(&self, inner: &Inner, game_id: &str, owner: Option<&str>) -> Facts {
        let derived = inner.derived.get(game_id).cloned().unwrap_or_default();
        let contextual = |mut f: Failure| {
            if f.kind == ErrorKind::AccessNeeded {
                let category = f
                    .paths
                    .iter()
                    .find_map(|p| self.privacy.needed(Path::new(p)))
                    .or_else(|| derived.access.as_ref().map(|(_, c)| *c));
                if let Some(category) = category {
                    f.access = Some(Box::new(AccessInfo {
                        category: category.as_str().into(),
                        denied: self.privacy.is_denied(category),
                        settings_url: category.settings_url().into(),
                    }));
                }
            }
            f.with_game_if_missing(game_id)
        };
        let store_failure = if let Some(category) = self.privacy.needed(&inner.store) {
            Some(contextual(Failure::new(ErrorKind::AccessNeeded, category.as_str()).path(&inner.store)))
        } else if !inner.store_available {
            Some(
                Failure::new(ErrorKind::StoreUnavailable, "the checkpoint store can't be reached")
                    .path(&inner.store)
                    .game(game_id),
            )
        } else {
            None
        };
        let location_failure = derived.active.as_ref().err().cloned().map(contextual);
        let target_failure = derived.active.as_ref().ok().and_then(|targets| {
            targets.iter().find(|t| t.presence == Presence::Unknown).map(|t| {
                Failure::new(ErrorKind::TargetUnavailable, "the save location can't be read")
                    .path(&t.root)
                    .game(game_id)
            })
        });
        let blocked = inner.blocked.get(game_id).cloned();
        let recovery_failure = blocked.as_ref().and_then(|f| {
            if let Some(game) = inner.games.get(game_id)
                && !game.expert_mode
                && (!inner.processes_observed || game.executables().is_empty())
            {
                return Some(
                    Failure::new(ErrorKind::Io, "couldn't establish whether the game is running").game(game_id),
                );
            }
            if exit_first(inner, game_id) {
                return None;
            }
            if let Some(path) = f.paths.iter().find(|p| self.privacy.needed(Path::new(p)).is_some()) {
                return Some(contextual(Failure::new(ErrorKind::AccessNeeded, "recovery needs access").path(path)));
            }
            inner.recovery_unavailable.get(game_id).cloned()
        });
        Facts {
            phase: inner.phase,
            busy: inner.busy.get(game_id).is_some_and(|op| Some(op.id.as_str()) != owner),
            store_moving: inner.store_moving,
            blocked,
            recovery_failure,
            store_failure,
            location_failure,
            target_failure,
            exit_locked: exit_first(inner, game_id),
            has_data: derived.has_data,
            has_saves: inner.caches.get(game_id).is_some_and(|cache| cache.latest.is_some()),
        }
    }
}

/// Which game the hotkeys act on: the selected game while the window is
/// focused, otherwise the active game.
pub fn hotkey_target(inner: &Inner) -> Option<(String, &'static str)> {
    if inner.ui.focused
        && let Some(selected) = &inner.ui.selected
        && inner.games.contains_key(selected)
    {
        return Some((selected.clone(), "window"));
    }
    // A foreground game awaiting access is still the intended target. Its
    // request goes through the same policy instead of falling through to another game.
    if let Some(front) = &inner.front
        && inner.derived.get(front).is_some_and(|d| d.access.is_some())
    {
        return Some((front.clone(), "active"));
    }
    inner.stack.active().filter(|g| inner.games.contains_key(*g)).map(|g| (g.to_string(), "active"))
}

/// Library order: running games in stack order, then the rest by when the
/// user last switched to them, newest first, then games never focused, by
/// name. The game just quit stays at the top instead of dropping to
/// wherever its name sorts.
pub fn sort_library(games: &mut [&Game], stack: &[String], last_focused: &HashMap<String, String>) {
    games.sort_by_cached_key(|g| {
        let running = stack.iter().position(|s| *s == g.id).unwrap_or(usize::MAX);
        let focused = last_focused.get(&g.id).map(|at| std::cmp::Reverse(at.clone()));
        // `None` sorts first; never-focused games go last.
        (running, focused.is_none(), focused, g.name.to_lowercase(), g.id.clone())
    });
}

fn placeholder_state(instance: &str) -> State {
    State {
        instance: instance.to_string(),
        revision: 0,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        phase: Phase::Starting,
        settings: SettingsInfo {
            play_sounds: true,
            save_shortcut: String::new(),
            load_shortcut: String::new(),
            launch_on_startup: false,
            launch_on_startup_available: false,
            launch_on_startup_needs_approval: false,
            flush_old_checkpoints: true,
            checkpoint_store: String::new(),
        },
        store: StoreInfo { path: String::new(), available: false },
        scan: ScanInfo { running: None, running_full: None, last_user: None, scans: 0, full_scans: 0 },
        active_stack: Vec::new(),
        hotkey_target: None,
        catalog_revision: String::new(),
        games: Vec::new(),
        deletes: Vec::new(),
    }
}

impl Inner {
    pub fn new(store: PathBuf, play_sounds: bool) -> Inner {
        Inner {
            phase: Phase::Starting,
            games: BTreeMap::new(),
            derived: HashMap::new(),
            caches: HashMap::new(),
            stack: ActiveStack::default(),
            last_focused: HashMap::new(),
            sessions: HashMap::new(),
            processes: BTreeMap::new(),
            processes_observed: false,
            front: None,
            busy: HashMap::new(),
            blocked: HashMap::new(),
            recovery_unavailable: HashMap::new(),
            last_results: HashMap::new(),
            notices: HashMap::new(),
            deletes: BTreeMap::new(),
            ops: HashMap::new(),
            scan: ScanInfo { running: None, running_full: None, last_user: None, scans: 0, full_scans: 0 },
            ui: UiReport::default(),
            ui_connections: 0,
            ui_started: None,
            last_focus_scan: None,
            store,
            store_available: false,
            store_moving: false,
            play_sounds,
            flush_old: true,
            shortcuts: Shortcut::defaults(),
            launch_on_startup: false,
            launch_needs_approval: false,
            revision: 0,
            hotkey_ops: HashSet::new(),
            artwork: HashMap::new(),
        }
    }

    pub fn game(&self, id: &str) -> Result<&Game, Failure> {
        self.games.get(id).ok_or_else(|| Failure::new(ErrorKind::NotFound, format!("no game {id:?}")))
    }
}

/// Ends the process at once, the way a crash or a kill would.
pub fn hard_exit() -> ! {
    savescummer_platform::process::hard_exit(86)
}

pub fn kind_name(kind: &GameKind) -> &'static str {
    match kind {
        GameKind::Known => "known",
        GameKind::Custom => "custom",
    }
}

pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, name: &str) -> Game {
        Game::from_record_json(&format!(r#"{{"id":"{id}","kind":"custom","name":"{name}","installed":true}}"#)).unwrap()
    }

    #[test]
    fn the_library_is_ordered_by_last_focus() {
        let games = [game("a", "Alpha"), game("b", "beta"), game("c", "Gamma"), game("d", "Delta"), game("e", "Echo")];
        let mut list: Vec<&Game> = games.iter().collect();
        let stack = vec!["e".to_string()];
        let focused: HashMap<String, String> = [
            ("c", "2026-10-01T10:00:00.000Z"),
            ("d", "2026-10-01T12:00:00.000Z"),
            ("e", "2026-09-01T00:00:00.000Z"),
        ]
        .into_iter()
        .map(|(g, at)| (g.to_string(), at.to_string()))
        .collect();
        sort_library(&mut list, &stack, &focused);
        let ids: Vec<&str> = list.iter().map(|g| g.id.as_str()).collect();
        // Running first; then newest focus; then never focused, by name.
        assert_eq!(ids, ["e", "d", "c", "a", "b"]);
    }
}
