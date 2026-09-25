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
    AccessInfo, Availability, CheckpointBrief, EventBody, GameKind, GameSummary, OpStatus, Operation, Phase, ScanInfo,
    SettingsInfo, State, StoreInfo,
};
use savescummer_scanner::Environment;
use savescummer_storage::{self as db, CheckpointRow, Storage};

use crate::model::{Derived, Game};
use crate::options::Options;

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
}

/// A Delete counting down, waiting or running.
#[derive(Debug, Clone)]
pub struct PendingDelete {
    pub op: Operation,
    pub deadline: Instant,
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
    /// Running games' current session ids.
    pub sessions: HashMap<String, String>,
    /// Running games' processes, as the monitor last saw them.
    pub processes: BTreeMap<String, Vec<u32>>,
    /// The game in front, as the monitor last saw it, even one waiting for
    /// access (which is never on the stack).
    pub front: Option<String>,
    pub busy: HashMap<String, Operation>,
    pub blocked: HashMap<String, Failure>,
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
    pub launch_on_startup: bool,
    /// macOS: turned off in System Settings, where only the user can turn
    /// it on again.
    pub launch_needs_approval: bool,
    pub revision: u64,
    /// Games whose hotkey operation should play sounds when it finishes.
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
        let games = inner.games.values().filter(|g| g.installed).map(|g| self.summary(inner, g)).collect();
        let catalog = self.catalog.read().unwrap_or_else(|e| e.into_inner());
        State {
            instance: self.instance.clone(),
            revision: inner.revision,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            phase: inner.phase,
            settings: SettingsInfo {
                play_sounds: inner.play_sounds,
                launch_on_startup: inner.launch_on_startup,
                launch_on_startup_available: savescummer_platform::autostart::available(),
                launch_on_startup_needs_approval: inner.launch_needs_approval,
                checkpoint_store: inner.store.to_string_lossy().into_owned(),
            },
            store: StoreInfo { path: inner.store.to_string_lossy().into_owned(), available: inner.store_available },
            scan: inner.scan.clone(),
            active_stack: inner.stack.entries().to_vec(),
            hotkey_target: hotkey_target(inner).map(|(g, _)| g),
            catalog_revision: catalog.bundle.source.revision.clone(),
            games,
            deletes: inner.deletes.values().map(with_remaining).collect(),
        }
    }

    fn summary(&self, inner: &Inner, game: &Game) -> GameSummary {
        let cache = inner.caches.get(&game.id).cloned().unwrap_or_default();
        let derived = inner.derived.get(&game.id).cloned().unwrap_or_default();
        let (save, load) = availability(inner, game, &derived, &cache);
        GameSummary {
            id: game.id.clone(),
            name: game.name.clone(),
            kind: game.kind.clone(),
            catalog_id: game.catalog_id.clone(),
            install_tag: game.install_tag.clone(),
            store: game.store().map(str::to_string),
            installed: game.installed,
            running: inner.stack.contains(&game.id),
            info: game.info.clone(),
            executable: game.main_executable().map(|p| p.to_string_lossy().into_owned()),
            save,
            load,
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

fn with_remaining(delete: &PendingDelete) -> Operation {
    let mut op = delete.op.clone();
    if op.status == OpStatus::CountingDown {
        op.remaining_ms = Some(delete.deadline.saturating_duration_since(Instant::now()).as_millis() as u64);
    }
    op
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

/// Why Save and Load are or aren't available right now.
pub fn availability(inner: &Inner, game: &Game, derived: &Derived, cache: &GameCache) -> (Availability, Availability) {
    let common = || -> Option<ErrorKind> {
        if let Err(f) = &derived.active {
            return Some(match f.kind {
                ErrorKind::NoSaveLocation | ErrorKind::AccessNeeded => f.kind,
                _ => ErrorKind::InvalidTarget,
            });
        }
        if !inner.store_available {
            return Some(ErrorKind::StoreUnavailable);
        }
        if inner.store_moving {
            return Some(ErrorKind::Busy);
        }
        if inner.blocked.contains_key(&game.id) {
            return Some(ErrorKind::Blocked);
        }
        if inner.busy.contains_key(&game.id) {
            return Some(ErrorKind::Busy);
        }
        if let Ok(targets) = &derived.active
            && targets.iter().any(|t| t.presence == Presence::Unknown)
        {
            return Some(ErrorKind::TargetUnavailable);
        }
        None
    };
    let base = common();
    let save = match base {
        Some(reason) => Availability::no(reason),
        None if !derived.has_data => Availability::no(ErrorKind::NoGameData),
        None => Availability::yes(),
    };
    let load = match base {
        Some(reason) => Availability::no(reason),
        None if cache.latest.is_none() => Availability::no(ErrorKind::NoSaves),
        None => Availability::yes(),
    };
    (save, load)
}

/// Which game the hotkeys act on: the selected game while the window is
/// focused, otherwise the top of the ACTIVE STACK.
pub fn hotkey_target(inner: &Inner) -> Option<(String, &'static str)> {
    if inner.ui.focused
        && let Some(selected) = &inner.ui.selected
        && inner.games.contains_key(selected)
    {
        return Some((selected.clone(), "window"));
    }
    inner.stack.top().map(|g| (g.to_string(), "stack"))
}

fn placeholder_state(instance: &str) -> State {
    State {
        instance: instance.to_string(),
        revision: 0,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        phase: Phase::Starting,
        settings: SettingsInfo {
            play_sounds: true,
            launch_on_startup: false,
            launch_on_startup_available: false,
            launch_on_startup_needs_approval: false,
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
            sessions: HashMap::new(),
            processes: BTreeMap::new(),
            front: None,
            busy: HashMap::new(),
            blocked: HashMap::new(),
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
