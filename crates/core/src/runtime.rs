use crate::*;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

pub struct Runtime {
    state: Mutex<State>,
    repository: Arc<dyn Repository>,
    io: Arc<dyn SnapshotIo>,
    policy: Arc<dyn PathPolicy>,
    clock: Arc<dyn Clock>,
    executing: Mutex<BTreeSet<Id>>,
    observation_run: Id,
    summary_cache: Mutex<std::collections::BTreeMap<Id, (u64, Option<Snapshot>)>>,
}

struct WorkingState<'a> {
    guard: MutexGuard<'a, State>,
    value: State,
}
impl std::ops::Deref for WorkingState<'_> {
    type Target = State;
    fn deref(&self) -> &State {
        &self.value
    }
}
impl std::ops::DerefMut for WorkingState<'_> {
    fn deref_mut(&mut self) -> &mut State {
        &mut self.value
    }
}

impl Runtime {
    pub fn open(
        repository: Arc<dyn Repository>,
        io: Arc<dyn SnapshotIo>,
        policy: Arc<dyn PathPolicy>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let mut state = repository.boot()?;
        // A fresh monitor reconstructs activity; persisted stack is not evidence
        // of launches or closes while the host was stopped.
        state.active_stack.clear();
        let runtime = Self {
            state: Mutex::new(state),
            repository,
            io,
            policy,
            clock,
            executing: Mutex::new(BTreeSet::new()),
            observation_run: new_id(),
            summary_cache: Mutex::new(Default::default()),
        };
        runtime.recover_startup()?;
        for game_id in runtime.games()?.keys() {
            if let Err(error) = runtime.refresh(game_id)
                && error.code == ErrorCode::Storage
            {
                return Err(error);
            }
        }
        Ok(runtime)
    }
    fn lock(&self) -> Result<WorkingState<'_>> {
        let guard = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Storage, "runtime state lock poisoned"))?;
        let value = guard.clone();
        Ok(WorkingState { guard, value })
    }
    fn lock_game(&self, game: &str) -> Result<WorkingState<'_>> {
        let mut state = self.lock()?;
        state.snapshots = self
            .repository
            .game_snapshots(game)?
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        Ok(state)
    }
    fn commit(&self, guard: &mut WorkingState<'_>, mut next: State) -> Result<()> {
        next.revision = guard.revision + 1;
        let changes = MetadataChanges::between(guard, &next);
        self.repository.commit_changes(&changes)?;
        next.history.clear();
        next.visible_history.clear();
        next.snapshots.clear();
        next.clear_history.clear();
        retain_current_operations(&mut next);
        *guard.guard = next.clone();
        guard.value = next;
        Ok(())
    }
    pub fn games(&self) -> Result<std::collections::BTreeMap<Id, Game>> {
        Ok(self.lock()?.games.clone())
    }
    /// Watch polls only live availability and a scalar revision. It never reads
    /// audit rows or constructs the summary when nothing has changed.
    pub fn revision(&self) -> Result<u64> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Storage, "runtime state lock poisoned"))?;
        let changes = state
            .games
            .values()
            .filter_map(|game| {
                let available = self.io.accessible_dir(&game.data_dir).unwrap_or(false);
                (state
                    .availability
                    .get(&game.id)
                    .is_none_or(|a| a.data_available != available))
                .then(|| (game.id.clone(), available))
            })
            .collect::<Vec<_>>();
        if !changes.is_empty() {
            for (game, available) in changes {
                state.availability.entry(game).or_default().data_available = available;
            }
            state.revision += 1;
        }
        Ok(state.revision)
    }
    fn visit_operations(
        &self,
        game: &str,
        mut visit: impl FnMut(&Operation) -> Result<()>,
    ) -> Result<()> {
        let mut after = String::new();
        loop {
            let page = self.repository.operation_page(game, &after, 200)?;
            if page.is_empty() {
                return Ok(());
            }
            for op in page {
                visit(&op)?;
                after = op.id;
            }
        }
    }
    pub fn summary(&self) -> Result<State> {
        let mut current = self.lock()?;
        let mut state = current.clone();
        let mut cache = self
            .summary_cache
            .lock()
            .map_err(|_| Error::new(ErrorCode::Storage, "summary lock poisoned"))?;
        for game in state.games.values() {
            let status = self.repository.history_status(&game.id)?;
            if !cache
                .get(&game.id)
                .is_some_and(|(revision, _)| *revision == status.revision)
            {
                let checkpoint = self
                    .repository
                    .game_snapshots(&game.id)?
                    .into_iter()
                    .filter(|s| self.checkpoint_eligible(s, game, SnapshotKind::Saved))
                    .max_by_key(|s| (s.selection_time, s.registration_order));
                cache.insert(game.id.clone(), (status.revision, checkpoint));
            }
            let checkpoint = cache.get(&game.id).and_then(|(_, s)| s.as_ref());
            state.availability.insert(
                game.id.clone(),
                GameAvailability {
                    data_available: self.io.accessible_dir(&game.data_dir).unwrap_or(false),
                    default_snapshot_id: checkpoint.map(|s| s.id.clone()),
                },
            );
            if let Some(checkpoint) = checkpoint {
                state
                    .snapshots
                    .insert(checkpoint.id.clone(), checkpoint.clone());
            }
            state.history_status.insert(game.id.clone(), status);
        }
        if current.availability != state.availability
            || current.history_status != state.history_status
        {
            current.guard.availability = state.availability.clone();
            current.guard.history_status = state.history_status.clone();
            current.guard.revision += 1;
            state.revision = current.guard.revision;
        }
        Ok(state)
    }
    pub fn history_page(
        &self,
        game_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<HistoryPage> {
        self.history_page_at(game_id, cursor, limit, None)
    }
    pub fn history_page_at(
        &self,
        game_id: &str,
        cursor: Option<&str>,
        limit: usize,
        anchor_id: Option<&str>,
    ) -> Result<HistoryPage> {
        if cursor.is_some() && anchor_id.is_some() {
            return Err(Error::new(
                ErrorCode::InvalidRequest,
                "use a cursor or an anchor, not both",
            ));
        }
        if limit == 0 || limit > 200 {
            return Err(Error::new(
                ErrorCode::InvalidRequest,
                "history page size must be between 1 and 200",
            ));
        }
        let state = self.lock()?;
        let game = state
            .games
            .get(game_id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown game"))?;
        let status = self.repository.history_status(game_id)?;
        let before = if let Some(cursor) = cursor {
            let (host, game, revision, before): (String, String, u64, u64) =
                serde_json::from_str(cursor)
                    .map_err(|_| Error::new(ErrorCode::InvalidRequest, "invalid history cursor"))?;
            if host != self.observation_run || game != game_id || revision != status.revision {
                return Err(Error::new(
                    ErrorCode::CursorExpired,
                    "history changed; reload the first page",
                ));
            }
            before
        } else if let Some(anchor) = anchor_id {
            let entry = self
                .repository
                .history_entry(anchor)?
                .filter(|h| h.game_id == game_id);
            if let Some(entry) = entry {
                let before = entry.sequence.saturating_add(1).min(i64::MAX as u64);
                if self
                    .repository
                    .history_rows(game_id, before, 1)?
                    .first()
                    .is_some_and(|h| h.id == anchor)
                {
                    before
                } else {
                    i64::MAX as u64
                }
            } else {
                i64::MAX as u64
            }
        } else {
            i64::MAX as u64
        };
        let entries = self.repository.history_rows(game_id, before, limit + 1)?;
        let mut rows = Vec::new();
        let mut size = 0;
        let mut more = false;
        for entry in entries {
            if rows.len() == limit {
                more = true;
                break;
            }
            let snapshot = action_checkpoint(&entry)
                .map(|id| self.repository.snapshot(id))
                .transpose()?
                .flatten();
            let action = snapshot.as_ref().map(|s| match entry.kind {
                HistoryKind::Saved | HistoryKind::ExistingBackup => Action::Load {
                    target: Some(s.id.clone()),
                },
                _ => Action::Revert {
                    target: s.id.clone(),
                },
            });
            let available = snapshot
                .as_ref()
                .is_some_and(|s| self.checkpoint_eligible(s, game, s.kind));
            let display_time = if entry.kind == HistoryKind::ExistingBackup {
                snapshot
                    .as_ref()
                    .map_or(entry.recorded_at, |s| s.selection_time)
            } else {
                entry.recorded_at
            };
            let target_time = entry
                .target_id
                .as_ref()
                .map(|id| self.repository.history_entry(id))
                .transpose()?
                .flatten()
                .map(|h| h.recorded_at);
            let row = HistoryRow {
                entry,
                display_time,
                target_time,
                action,
                available,
            };
            let bytes = serde_json::to_vec(&row)
                .map_err(|e| Error::new(ErrorCode::Storage, e.to_string()))?
                .len();
            if bytes > 512 * 1024 {
                return Err(Error::new(
                    ErrorCode::ResponseTooLarge,
                    "one history row exceeds the page byte budget",
                ));
            }
            if size + bytes > 512 * 1024 {
                more = true;
                break;
            }
            size += bytes;
            rows.push(row);
        }
        let next_cursor = if more {
            rows.last().map(|row| {
                serde_json::to_string(&(
                    &self.observation_run,
                    game_id,
                    status.revision,
                    row.entry.sequence,
                ))
                .unwrap()
            })
        } else {
            None
        };
        Ok(HistoryPage {
            game_id: game_id.into(),
            revision: status.revision,
            rows,
            next_cursor,
        })
    }
    pub fn settings(&self) -> Result<Settings> {
        Ok(self.lock()?.settings.clone())
    }
    pub fn has_pending_operations(&self) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Storage, "runtime state lock poisoned"))?
            .operations
            .values()
            .any(|o| o.status == OperationStatus::Pending))
    }
    pub fn set_play_sounds(&self, enabled: bool) -> Result<()> {
        let mut state = self.lock()?;
        if state.settings.play_sounds == enabled {
            return Ok(());
        }
        let mut next = state.clone();
        next.settings.play_sounds = enabled;
        self.commit(&mut state, next)
    }
    pub fn set_launch_on_startup(&self, enabled: bool) -> Result<()> {
        let mut state = self.lock()?;
        if state.settings.launch_on_startup == enabled {
            return Ok(());
        }
        let mut next = state.clone();
        next.settings.launch_on_startup = enabled;
        self.commit(&mut state, next)
    }
    pub fn active_game(&self) -> Result<Id> {
        self.lock()?
            .active_stack
            .first()
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "no monitored game is running"))
    }
    pub fn explore_parent(&self, game_id: &str) -> Result<PathBuf> {
        let state = self.lock()?;
        let game = state
            .games
            .get(game_id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown game"))?;
        let parent = game
            .data_dir
            .parent()
            .ok_or_else(|| Error::new(ErrorCode::InvalidPath, "data directory has no parent"))?;
        if !self.io.accessible_dir(parent)? {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "data directory parent is unavailable",
            ));
        }
        Ok(parent.to_path_buf())
    }
    pub fn explorer_targets(&self, path: &std::path::Path) -> Result<Vec<ExplorerTarget>> {
        let path = self.policy.resolve(path)?;
        // Refresh before resolving a menu. Execution still revalidates the exact
        // generation, so an open menu cannot retarget a replaced backup.
        let ids: Vec<_> = self.lock()?.games.values().filter(|game| {
            !self.policy.same_location(&path, &game.data_dir)
                && matches!((path.parent(), game.data_dir.parent()), (Some(a), Some(b)) if self.policy.same_location(a, b))
        }).map(|game| game.id.clone()).collect();
        for id in ids {
            if let Err(error) = self.refresh(&id)
                && !matches!(
                    error.code,
                    ErrorCode::Busy
                        | ErrorCode::RecoveryNeeded
                        | ErrorCode::InvalidPath
                        | ErrorCode::Unavailable
                )
            {
                return Err(error);
            }
        }
        let state = self.lock()?;
        let mut targets = vec![];
        for game in state.games.values() {
            if game.configuration_error.is_some() {
                continue;
            }
            if self.policy.same_location(&path, &game.data_dir) && self.io.accessible_dir(&path)? {
                targets.push(ExplorerTarget {
                    game_id: game.id.clone(),
                    action: Action::Save,
                });
            }
            for snapshot in self.repository.game_snapshots(&game.id)? {
                if self.checkpoint_eligible(&snapshot, game, SnapshotKind::Saved)
                    && self.policy.same_location(&path, &snapshot.path)
                {
                    targets.push(ExplorerTarget {
                        game_id: game.id.clone(),
                        action: Action::Load {
                            target: Some(snapshot.id.clone()),
                        },
                    });
                }
            }
        }
        Ok(targets)
    }
    pub fn operation_for_request(&self, request_id: &str) -> Result<Option<Id>> {
        Ok(self.repository.request_operation(request_id)?.map(|o| o.id))
    }
    /// Explicit diagnostic/test export. Normal clients use summary and pages.
    pub fn state(&self) -> Result<State> {
        let current = self.lock()?;
        let mut state = self.repository.load()?;
        state.revision = current.revision;
        state.active_stack = current.active_stack.clone();
        state.discovery_errors = current.discovery_errors.clone();
        state.operations.extend(current.operations.clone());
        drop(current);
        for snapshot in state.snapshots.values_mut() {
            snapshot.available = state
                .games
                .get(&snapshot.game_id)
                .is_some_and(|game| self.checkpoint_eligible(snapshot, game, snapshot.kind));
        }
        state.visible_history = crate::history::visible(&state);
        state.availability = state
            .games
            .values()
            .map(|game| {
                let default_snapshot_id = state
                    .snapshots
                    .values()
                    .filter(|s| {
                        s.game_id == game.id && s.kind == SnapshotKind::Saved && s.available
                    })
                    .max_by_key(|s| (s.selection_time, s.registration_order))
                    .map(|s| s.id.clone());
                (
                    game.id.clone(),
                    GameAvailability {
                        data_available: self.io.accessible_dir(&game.data_dir).unwrap_or(false),
                        default_snapshot_id,
                    },
                )
            })
            .collect();
        // Live directories can appear after first launch without producing a
        // history event. Publish these read-model changes to revision watchers.
        // The projection is transient; the next durable commit carries revision.
        let mut current = self.lock()?;
        if current.revision == state.revision && current.availability != state.availability {
            current.guard.availability = state.availability.clone();
            current.guard.revision += 1;
            state.revision = current.guard.revision;
        }
        Ok(state)
    }
    fn checkpoint_eligible(&self, snapshot: &Snapshot, game: &Game, kind: SnapshotKind) -> bool {
        self.checkpoint_matches(snapshot, game, kind)
            && snapshot.removed_at.is_none()
            && snapshot.available
    }
    fn checkpoint_matches(&self, snapshot: &Snapshot, game: &Game, kind: SnapshotKind) -> bool {
        snapshot.game_id == game.id
            && snapshot.kind == kind
            && snapshot
                .original_data_dir
                .as_ref()
                .is_some_and(|path| self.policy.same_location(path, &game.data_dir))
    }
    fn next_snapshot_order(state: &mut State) -> u64 {
        state.snapshot_order += 1;
        state.snapshot_order
    }
    fn check_idle(state: &State, game: &str) -> Result<()> {
        if state
            .operations
            .values()
            .any(|o| o.game_id == game && o.status == OperationStatus::Pending)
        {
            return Err(Error::new(
                ErrorCode::Busy,
                "an operation is already running for this game",
            ));
        }
        if state
            .operations
            .values()
            .any(|o| o.game_id == game && o.status == OperationStatus::RecoveryNeeded)
        {
            return Err(Error::new(
                ErrorCode::RecoveryNeeded,
                "resolve the interrupted operation first",
            ));
        }
        Ok(())
    }
    fn validated(&self, state: &State, game: &Game) -> Result<PathBuf> {
        if let Some(error) = &game.configuration_error {
            return Err(Error::new(ErrorCode::InvalidPath, error));
        }
        let others = state
            .games
            .values()
            .filter(|g| g.id != game.id && g.configuration_error.is_none())
            .map(|g| (g.id.clone(), g.data_dir.clone()))
            .collect::<Vec<_>>();
        let resolved = self.policy.validate(&game.data_dir, &others)?;
        if resolved != game.data_dir {
            return Err(Error::new(
                ErrorCode::InvalidPath,
                "data directory alias changed; configure the location again",
            ));
        }
        Ok(resolved)
    }
    pub fn configure(
        &self,
        id: Id,
        name: String,
        path: PathBuf,
        executables: Vec<PathBuf>,
    ) -> Result<Game> {
        self.configure_game(id, name, path, executables, None, None)
    }
    fn configure_game(
        &self,
        id: Id,
        name: String,
        path: PathBuf,
        executables: Vec<PathBuf>,
        origin: Option<GameOrigin>,
        installed: Option<bool>,
    ) -> Result<Game> {
        if id.is_empty() || name.trim().is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidRequest,
                "game ID and name must not be empty",
            ));
        }
        let mut state = self.lock()?;
        Self::check_idle(&state, &id)?;
        let others = state
            .games
            .values()
            .filter(|g| g.id != id && g.configuration_error.is_none())
            .map(|g| (g.id.clone(), g.data_dir.clone()))
            .collect::<Vec<_>>();
        let data_dir = self.policy.validate(&path, &others)?;
        // Reservations include uncached checkpoints and old operation journals.
        self.repository.visit_reserved_paths(&mut |retained| {
            if data_dir.starts_with(retained) || retained.starts_with(&data_dir) {
                return Err(Error::new(
                    ErrorCode::InvalidPath,
                    "data directory overlaps retained snapshot data",
                ));
            }
            Ok(())
        })?;
        let executables = executables
            .iter()
            .map(|p| self.policy.resolve(p))
            .collect::<Result<Vec<_>>>()?;
        let existing = state.games.get(&id);
        let game = Game {
            id: id.clone(),
            name: name.trim().into(),
            origin: origin.unwrap_or_else(|| existing.map_or(GameOrigin::Custom, |g| g.origin)),
            info: existing.map(|g| g.info.clone()).unwrap_or_default(),
            data_dir,
            executables,
            installed: installed.unwrap_or_else(|| existing.is_none_or(|g| g.installed)),
            configuration_error: None,
            detected_locations: existing
                .map(|g| g.detected_locations.clone())
                .unwrap_or_default(),
            user_configured: true,
        };
        let mut next = state.clone();
        next.games.insert(id, game.clone());
        self.commit(&mut state, next)?;
        Ok(game)
    }
    pub fn add_custom_game(
        &self,
        name: String,
        path: PathBuf,
        executable: PathBuf,
        installed: bool,
    ) -> Result<Game> {
        if name.trim().is_empty() || executable.as_os_str().is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidRequest,
                "name, executable and save location must not be empty",
            ));
        }
        self.configure_game(
            format!("custom-{}", new_id()),
            name,
            path,
            vec![executable],
            Some(GameOrigin::Custom),
            Some(installed),
        )
    }
    /// Record scanner facts without replacing a user's validated configuration.
    /// Called by the composition root; the policy and ambiguity rules live here.
    pub fn record_discovery(
        &self,
        id: Id,
        name: String,
        info: String,
        locations: Vec<GameLocation>,
    ) -> Result<()> {
        let existing = self.lock()?.games.get(&id).cloned();
        if locations.is_empty() {
            if let Some(game) = existing {
                self.set_game_info(&game.id, &info)?;
            }
            return Ok(());
        }
        let mut failure = None;
        let automatic = existing
            .as_ref()
            .is_none_or(|game| !game.user_configured && game.configuration_error.is_some());
        if automatic && locations.len() == 1 {
            let location = &locations[0];
            if let Err(error) = self.configure_game(
                id.clone(),
                name.clone(),
                location.data_dir.clone(),
                location.executables.clone(),
                Some(GameOrigin::Known),
                Some(true),
            ) {
                if matches!(error.code, ErrorCode::Busy | ErrorCode::RecoveryNeeded) {
                    return Ok(());
                }
                failure = Some(error.message);
            }
        } else if automatic {
            failure = Some(
                "Choose one of the detected installations or data locations in Configure".into(),
            );
        }
        let mut state = self.lock()?;
        Self::check_idle(&state, &id)?;
        let mut next = state.clone();
        let game = next.games.entry(id.clone()).or_insert_with(|| Game {
            id,
            name,
            origin: GameOrigin::Known,
            info: info.clone(),
            data_dir: locations[0].data_dir.clone(),
            executables: locations[0].executables.clone(),
            installed: true,
            configuration_error: failure.clone(),
            detected_locations: vec![],
            user_configured: false,
        });
        game.origin = GameOrigin::Known;
        game.info = info;
        game.detected_locations = locations;
        if automatic {
            game.user_configured = false;
            game.configuration_error = failure;
        }
        game.installed = true;
        self.commit(&mut state, next)
    }
    pub fn select_detected_location(
        &self,
        game_id: &str,
        location: Option<&GameLocation>,
    ) -> Result<Game> {
        let game = self
            .lock()?
            .games
            .get(game_id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown game"))?;
        let selected = match location {
            Some(location) if game.detected_locations.contains(location) => location,
            Some(_) => {
                return Err(Error::new(
                    ErrorCode::InvalidTarget,
                    "location is no longer a detected candidate; rescan",
                ));
            }
            None if game.detected_locations.len() == 1 => &game.detected_locations[0],
            None => {
                return Err(Error::new(
                    ErrorCode::InvalidTarget,
                    "select an explicit detected location",
                ));
            }
        };
        // An explicit choice is an override, including Reset: it stays stable
        // through future rescans and can be reset again when defaults change.
        self.configure(
            game.id,
            game.name,
            selected.data_dir.clone(),
            selected.executables.clone(),
        )
    }
    pub fn set_discovery_errors(&self, errors: Vec<String>) -> Result<()> {
        let mut state = self.lock()?;
        if state.discovery_errors != errors {
            state.guard.discovery_errors = errors;
            state.guard.revision += 1;
        }
        Ok(())
    }
    pub fn classify_game_origins(&self, known_ids: &BTreeSet<Id>) -> Result<()> {
        let mut state = self.lock()?;
        let mut next = state.clone();
        let mut changed = false;
        for game in next.games.values_mut() {
            if game.origin != GameOrigin::Legacy {
                continue;
            }
            let origin = if known_ids.contains(&game.id) {
                GameOrigin::Known
            } else {
                GameOrigin::Custom
            };
            if game.origin != origin {
                game.origin = origin;
                changed = true;
            }
        }
        if changed {
            self.commit(&mut state, next)?;
        }
        Ok(())
    }
    pub fn set_installed(&self, game_id: &str, installed: bool) -> Result<()> {
        let mut state = self.lock()?;
        let mut next = state.clone();
        let game = next
            .games
            .get_mut(game_id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown game"))?;
        if game.installed == installed {
            return Ok(());
        }
        game.installed = installed;
        self.commit(&mut state, next)
    }
    pub fn refresh(&self, game_id: &str) -> Result<()> {
        let mut state = self.lock_game(game_id)?;
        Self::check_idle(&state, game_id)?;
        let game = state
            .games
            .get(game_id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown game"))?;
        if let Some(error) = &game.configuration_error {
            return Err(Error::new(ErrorCode::InvalidPath, error));
        }
        self.validated(&state, &game)?;
        let mut next = state.clone();
        let mut changed = false;
        for snapshot in next
            .snapshots
            .values_mut()
            .filter(|s| s.game_id == game_id && s.removed_at.is_none())
        {
            let observation = (|| -> Result<Option<(String, String)>> {
                let parent = snapshot
                    .path
                    .parent()
                    .ok_or_else(|| Error::new(ErrorCode::InvalidPath, "snapshot has no parent"))?;
                if !self.io.accessible_dir(parent)? || self.policy.resolve(parent)? != parent {
                    return Err(Error::new(
                        ErrorCode::Unavailable,
                        "backup parent is unavailable or changed",
                    ));
                }
                if !self.io.accessible_dir(&snapshot.path)? {
                    return Ok(None);
                }
                Ok(Some((
                    self.io.identity(&snapshot.path)?,
                    self.io.fingerprint(&snapshot.path)?,
                )))
            })();
            let available = match observation {
                Ok(Some((identity, fingerprint)))
                    if identity == snapshot.identity && fingerprint == snapshot.fingerprint =>
                {
                    true
                }
                Ok(observation) => {
                    snapshot.removed_at = Some(self.clock.now_ms());
                    snapshot.removal_reason = Some(if observation.is_none() {
                        RemovalReason::Deleted
                    } else {
                        RemovalReason::Changed
                    });
                    changed = true;
                    false
                }
                Err(_) => false,
            };
            changed |= snapshot.available != available;
            snapshot.available = available;
        }
        let mut candidates = self.io.saved_candidates(&game.data_dir).unwrap_or_default();
        candidates.sort();
        for path in candidates {
            if self
                .repository
                .snapshot_at_path(&path)?
                .is_some_and(|s| s.game_id != game_id)
            {
                continue;
            }
            if let Some(existing) = next
                .snapshots
                .values_mut()
                .find(|s| s.path == path && s.removed_at.is_none())
            {
                // Some v1 manual checkpoints outlived their configured location
                // without a journal recording its path. Associate only after a
                // normal scan verifies the same generation at this game's DIR.
                if existing.game_id == game.id
                    && existing.kind == SnapshotKind::Saved
                    && existing.original_data_dir.is_none()
                    && existing.available
                {
                    existing.original_data_dir = Some(game.data_dir.clone());
                    changed = true;
                }
                continue;
            }
            let inspection = (|| -> Result<_> {
                Ok((
                    self.io.modified_ms(&path)?,
                    self.io.identity(&path)?,
                    self.io.fingerprint(&path)?,
                ))
            })();
            let Ok((selection_time, identity, fingerprint)) = inspection else {
                continue;
            };
            if self
                .repository
                .unpublished_snapshot(game_id, &path, &identity)?
            {
                continue;
            }
            let snapshot = Snapshot {
                id: new_id(),
                game_id: game.id.clone(),
                original_data_dir: Some(game.data_dir.clone()),
                registration_order: Self::next_snapshot_order(&mut next),
                path,
                identity,
                fingerprint,
                removed_at: None,
                removal_reason: None,
                kind: SnapshotKind::Saved,
                saved_at: None,
                selection_time,
                discovered_at: self.clock.now_ms(),
                available: true,
            };
            self.append_history(
                &mut next,
                &game,
                HistoryKind::ExistingBackup,
                Some(snapshot.id.clone()),
                None,
                None,
            );
            next.snapshots.insert(snapshot.id.clone(), snapshot);
            changed = true;
        }
        if changed {
            self.commit(&mut state, next)?;
        }
        Ok(())
    }
    pub fn set_game_info(&self, game_id: &str, info: &str) -> Result<()> {
        let mut state = self.lock()?;
        let game = state
            .games
            .get(game_id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown game"))?;
        if game.info == info {
            return Ok(());
        }
        let mut next = state.clone();
        next.games.get_mut(game_id).unwrap().info = info.into();
        self.commit(&mut state, next)
    }
    pub fn accept(&self, game_id: &str, action: Action, request_id: Id) -> Result<Id> {
        if request_id.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidRequest,
                "request_id must not be empty",
            ));
        }
        // Idempotency is checked before refreshing or checking the busy state.
        {
            let _state = self.lock()?;
            if let Some(op) = self.repository.request_operation(&request_id)? {
                return if op.game_id == game_id && op.action == action {
                    Ok(op.id.clone())
                } else {
                    Err(Error::new(
                        ErrorCode::InvalidRequest,
                        "request_id already used for another command",
                    ))
                };
            }
        }
        // Reconcile external backup changes before checking a Flush preview's
        // revision, just as restore commands refresh before selecting a source.
        if matches!(
            action,
            Action::Save
                | Action::Load { .. }
                | Action::Revert { .. }
                | Action::Delete { .. }
                | Action::Flush { .. }
                | Action::Forget { .. }
        ) {
            self.refresh(game_id)?;
        }
        let mut state = self.lock_game(game_id)?;
        if let Some(op) = self.repository.request_operation(&request_id)? {
            return if op.game_id == game_id && op.action == action {
                Ok(op.id.clone())
            } else {
                Err(Error::new(
                    ErrorCode::InvalidRequest,
                    "request_id already used for another command",
                ))
            };
        }
        let game = state
            .games
            .get(game_id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown game"))?;
        self.validated(&state, &game)?;
        if let Action::Recover { operation, .. } = &action {
            if state
                .operations
                .values()
                .any(|o| o.game_id == game_id && o.status == OperationStatus::Pending)
            {
                return Err(Error::new(
                    ErrorCode::Busy,
                    "recovery attempt already running",
                ));
            }
            let old = state
                .operations
                .get(operation)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown interrupted operation"))?;
            if old.game_id != game_id
                || !self.policy.same_location(&old.live, &game.data_dir)
                || old.status != OperationStatus::RecoveryNeeded
            {
                return Err(Error::new(
                    ErrorCode::InvalidTarget,
                    "operation does not need recovery at this location",
                ));
            }
        } else {
            Self::check_idle(&state, game_id)?;
        }
        if let Action::Flush { confirmed_revision } | Action::Forget { confirmed_revision } =
            &action
            && *confirmed_revision != state.revision
        {
            return Err(Error::new(
                ErrorCode::ConfirmationRequired,
                "obtain a new cleanup preview and confirm its revision",
            ));
        }
        if matches!(action, Action::Forget { .. }) && game.origin != GameOrigin::Custom {
            return Err(Error::new(
                ErrorCode::InvalidTarget,
                "only custom games can be forgotten",
            ));
        }
        let (source, target) = self.resolve_target(&state, &game, &action)?;
        if let Some(snapshot) = &source
            && (snapshot.removed_at.is_some()
                || !snapshot.available
                || !self.io.accessible_dir(&snapshot.path)?
                || self.io.identity(&snapshot.path)? != snapshot.identity
                || self.io.fingerprint(&snapshot.path)? != snapshot.fingerprint)
        {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "the exact requested snapshot is unavailable or has been replaced",
            ));
        }
        if matches!(
            action,
            Action::Save | Action::Load { .. } | Action::Revert { .. }
        ) && !self.io.accessible_dir(&game.data_dir)?
        {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "current game data directory is missing",
            ));
        }
        let id = new_id();
        let name = game
            .data_dir
            .file_name()
            .ok_or_else(|| Error::new(ErrorCode::InvalidPath, "directory has no name"))?
            .to_string_lossy();
        let sibling = |suffix: &str| {
            game.data_dir
                .with_file_name(format!("{name}.{suffix}-{id}"))
        };
        let operation = Operation {
            id: id.clone(),
            request_id,
            game_id: game.id,
            action,
            status: OperationStatus::Pending,
            phase: Phase::Preparing,
            live: game.data_dir.clone(),
            source: source.as_ref().map(|s| s.path.clone()),
            source_identity: source.as_ref().map(|s| s.identity.clone()),
            source_fingerprint: source.as_ref().map(|s| s.fingerprint.clone()),
            source_id: source.map(|s| s.id),
            target_id: target,
            snapshot_path: None,
            recovery: sibling("recovery"),
            recovery_staging: sibling("recovery-staging"),
            staging: sibling("staging"),
            original: sibling("original"),
            recovery_complete: false,
            staging_identity: None,
            original_identity: None,
            recovery_identity: None,
            recovery_fingerprint: None,
            started_at: self.clock.now_ms(),
            bytes_copied: 0,
            error: None,
            resolution: None,
        };
        let mut next = state.clone();
        next.operations.insert(id.clone(), operation);
        self.commit(&mut state, next)?;
        Ok(id)
    }
    fn resolve_target(
        &self,
        state: &State,
        game: &Game,
        action: &Action,
    ) -> Result<(Option<Snapshot>, Option<Id>)> {
        let (target, kind) = match action {
            Action::Load { target } => (target.as_ref(), SnapshotKind::Saved),
            Action::Revert { target } => (Some(target), SnapshotKind::Recovery),
            Action::Delete { target } => {
                let snapshot = self
                    .repository
                    .snapshot(target)?
                    .ok_or_else(|| Error::new(ErrorCode::InvalidTarget, "unknown checkpoint ID"))?;
                if snapshot.game_id != game.id
                    || snapshot.removed_at.is_some()
                    || !snapshot
                        .original_data_dir
                        .as_ref()
                        .is_some_and(|path| self.policy.same_location(path, &game.data_dir))
                {
                    return Err(Error::new(
                        ErrorCode::InvalidTarget,
                        "checkpoint belongs to another game or data directory",
                    ));
                }
                return Ok((Some(snapshot), None));
            }
            _ => return Ok((None, None)),
        };
        let snapshot = if let Some(id) = target {
            self.repository
                .snapshot(id)?
                .ok_or_else(|| Error::new(ErrorCode::InvalidTarget, "unknown checkpoint ID"))?
        } else {
            state
                .snapshots
                .values()
                .filter(|s| self.checkpoint_eligible(s, game, kind))
                .max_by_key(|s| (s.selection_time, s.registration_order))
                .ok_or_else(|| Error::new(ErrorCode::Unavailable, "no available saved checkpoint"))?
                .clone()
        };
        if !self.checkpoint_matches(&snapshot, game, kind) {
            return Err(Error::new(
                ErrorCode::InvalidTarget,
                "checkpoint belongs to another game, data directory, or action kind",
            ));
        }
        // History is optional audit context, never the source of eligibility.
        let history = self.repository.checkpoint_history(&snapshot.id)?;
        Ok((Some(snapshot), history.map(|h| h.id)))
    }
    pub fn operation(&self, id: &str) -> Result<Operation> {
        let state = self.lock()?;
        state
            .operations
            .get(id)
            .cloned()
            .or(self.repository.operation(id)?)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown operation"))
    }
    fn update(&self, id: &str, change: impl FnOnce(&mut Operation)) -> Result<()> {
        let mut state = self.lock()?;
        if !state.operations.contains_key(id) {
            let op = self
                .repository
                .operation(id)?
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown operation"))?;
            state.operations.insert(id.into(), op);
        }
        let mut next = state.clone();
        change(
            next.operations
                .get_mut(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown operation"))?,
        );
        self.commit(&mut state, next)
    }
    fn phase(&self, id: &str, phase: Phase) -> Result<()> {
        self.update(id, |o| o.phase = phase)
    }
    fn copy(&self, id: &str, from: &std::path::Path, to: &std::path::Path) -> Result<()> {
        let base = self.operation(id)?.bytes_copied;
        self.io.copy(from, to, &mut |bytes| {
            // Progress is ephemeral; durable phase transitions commit the latest
            // value. Avoid one SQLite transaction per copied buffer.
            if let Ok(mut state) = self.state.lock()
                && let Some(op) = state.operations.get_mut(id)
            {
                op.bytes_copied = base + bytes;
                state.revision += 1;
            }
        })
    }
    /// The host must dispatch an accepted ID exactly once. Accepted work is owned
    /// by the host, never by the lifetime of a client connection.
    pub fn execute(&self, id: &str) -> Result<()> {
        {
            let mut executing = self
                .executing
                .lock()
                .map_err(|_| Error::new(ErrorCode::Storage, "execution lock poisoned"))?;
            if !executing.insert(id.into()) {
                return Ok(());
            }
        }
        let _guard = ExecutionGuard {
            runtime: self,
            id: id.into(),
        };
        let op = self.operation(id)?;
        if op.status != OperationStatus::Pending {
            return Ok(());
        }
        let validation = (|| {
            let state = self.lock()?;
            self.validated(&state, &state.games[&op.game_id])
                .map(|_| ())
        })();
        let result = validation.and_then(|_| match op.action {
            Action::Save => self.save(id),
            Action::Load { .. } | Action::Revert { .. } => self.restore(id, false),
            Action::Delete { .. } => self.delete_checkpoint(id),
            Action::Flush { .. } => self.flush(id),
            Action::Forget { .. } => self.forget(id),
            Action::Recover { .. } => self.resolve_recovery(id),
        });
        if let Err(error) = result {
            let mut current = self.operation(id)?;
            let uncertain = matches!(
                current.phase,
                Phase::MovingOriginal
                    | Phase::OriginalMoved
                    | Phase::InstallingReplacement
                    | Phase::ReplacementInstalled
            );
            let safe = if uncertain {
                self.rollback_if_safe(&current).unwrap_or(false)
            } else {
                true
            };
            current.status = if safe {
                OperationStatus::Failed
            } else {
                OperationStatus::RecoveryNeeded
            };
            current.error = Some(error.clone());
            if let Err(storage_error) = self.update(id, |o| *o = current.clone()) {
                // Keep the game blocked in memory if even the failure record
                // cannot commit. The prior durable phase remains recoverable.
                current.status = OperationStatus::RecoveryNeeded;
                current.error = Some(storage_error.clone());
                let mut state = self.lock()?;
                state.guard.operations.insert(id.into(), current);
                state.guard.revision += 1;
                return Err(storage_error);
            }
            return Err(error);
        }
        Ok(())
    }
    fn delete_checkpoint(&self, id: &str) -> Result<()> {
        let op = self.operation(id)?;
        let source = op
            .source
            .as_ref()
            .ok_or_else(|| Error::new(ErrorCode::Storage, "missing checkpoint path"))?;
        let resolved = self.policy.resolve(source)?;
        let library = self.lock()?.clone();
        if resolved != *source
            || library.games.values().any(|game| {
                game.data_dir.starts_with(&resolved) || resolved.starts_with(&game.data_dir)
            })
        {
            return Err(Error::new(
                ErrorCode::InvalidPath,
                "refusing to delete a changed alias or live data",
            ));
        }
        self.io.remove(source)?;

        let mut state = self.lock_game(&op.game_id)?;
        let mut next = state.clone();
        let checkpoint = next
            .snapshots
            .get_mut(op.source_id.as_deref().unwrap_or_default())
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "checkpoint record disappeared"))?;
        checkpoint.available = false;
        checkpoint.removed_at = Some(self.clock.now_ms());
        checkpoint.removal_reason = Some(RemovalReason::Deleted);
        let operation = next.operations.get_mut(id).unwrap();
        operation.phase = Phase::Finished;
        operation.status = OperationStatus::Completed;
        self.commit(&mut state, next)
    }
    fn save(&self, id: &str) -> Result<()> {
        let op = self.operation(id)?;
        self.copy(id, &op.live, &op.staging)?;
        let identity = self.io.identity(&op.staging)?;
        self.update(id, |o| o.staging_identity = Some(identity))?;
        let mut competing = Vec::new();
        for _ in 0..100 {
            let state = self.lock_game(&op.game_id)?.clone();
            let mut reserved = state
                .snapshots
                .values()
                .filter(|s| s.removed_at.is_none())
                .map(|s| s.path.clone())
                .chain(
                    state
                        .operations
                        .values()
                        .filter(|o| o.status != OperationStatus::Completed)
                        .flat_map(|o| o.snapshot_path.iter().cloned()),
                )
                .collect::<Vec<_>>();
            reserved.extend(self.repository.reserved_snapshot_paths(&op.game_id)?);
            reserved.extend(competing.iter().cloned());
            let destination = self.io.next_saved_path(&op.live, &reserved)?;
            if self.repository.snapshot_path_reserved(&destination)? {
                competing.push(destination);
                continue;
            }
            self.policy.validate(
                &destination,
                &state
                    .games
                    .values()
                    .filter(|g| g.configuration_error.is_none())
                    .map(|g| (g.id.clone(), g.data_dir.clone()))
                    .collect::<Vec<_>>(),
            )?;
            self.update(id, |o| {
                o.snapshot_path = Some(destination.clone());
                o.phase = Phase::PublishingSave;
            })?;
            match self.io.rename(&op.staging, &destination) {
                Ok(()) => return self.finish_snapshot(id, HistoryKind::Saved),
                Err(_) if self.io.exists(&destination)? => continue,
                Err(error) => return Err(error),
            }
        }
        Err(Error::new(
            ErrorCode::Io,
            "too many competing snapshot names",
        ))
    }
    fn preserve_current(&self, id: &str) -> Result<()> {
        let op = self.operation(id)?;
        self.copy(id, &op.live, &op.recovery_staging)?;
        self.io.rename(&op.recovery_staging, &op.recovery)?;
        let identity = self.io.identity(&op.recovery)?;
        let fingerprint = self.io.fingerprint(&op.recovery)?;
        self.update(id, |o| {
            o.recovery_complete = true;
            o.recovery_identity = Some(identity);
            o.recovery_fingerprint = Some(fingerprint);
            o.phase = Phase::RecoveryReady;
        })
    }
    fn restore(&self, id: &str, allow_missing: bool) -> Result<()> {
        let op = self.operation(id)?;
        let source = op
            .source
            .as_ref()
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "no restoration source"))?;
        if !self.io.accessible_dir(source)?
            || op.source_identity.as_ref() != Some(&self.io.identity(source)?)
            || op.source_fingerprint.as_ref() != Some(&self.io.fingerprint(source)?)
        {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "restoration source is missing",
            ));
        }
        let live_exists = self.io.accessible_dir(&op.live)?;
        if !live_exists && !allow_missing {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "current data disappeared before preservation",
            ));
        }
        if live_exists {
            self.preserve_current(id)?;
        }
        self.copy(id, source, &op.staging)?;
        self.phase(id, Phase::StageReady)?;
        if live_exists {
            let identity = self.io.identity(&op.live)?;
            self.update(id, |o| {
                o.original_identity = Some(identity);
                o.phase = Phase::MovingOriginal;
            })?;
            self.io.rename(&op.live, &op.original)?;
            self.phase(id, Phase::OriginalMoved)?;
        }
        self.phase(id, Phase::InstallingReplacement)?;
        self.io.rename(&op.staging, &op.live)?;
        self.phase(id, Phase::ReplacementInstalled)?;
        if matches!(op.action, Action::Recover { .. }) {
            self.finish_recovery(id, "restored_before")
        } else {
            self.finish_snapshot(
                id,
                if matches!(op.action, Action::Revert { .. }) {
                    HistoryKind::Reverted
                } else {
                    HistoryKind::Loaded
                },
            )
        }
    }
    fn append_history(
        &self,
        state: &mut State,
        game: &Game,
        kind: HistoryKind,
        snapshot: Option<Id>,
        recovery: Option<Id>,
        target: Option<Id>,
    ) {
        state.history_sequence += 1;
        let sequence = state.history_sequence;
        state.history.push(History {
            id: new_id(),
            observation_run: self.observation_run.clone(),
            sequence,
            game_id: game.id.clone(),
            kind,
            recorded_at: self.clock.now_ms(),
            snapshot_id: snapshot,
            recovery_id: recovery,
            target_id: target,
        });
    }
    fn register_recovery(
        &self,
        state: &mut State,
        op: &Operation,
        time: u64,
    ) -> Result<Option<Id>> {
        if !op.recovery_complete {
            return Ok(None);
        }
        if let Some(existing) = state.snapshots.values().find(|s| s.path == op.recovery) {
            return Ok(Some(existing.id.clone()));
        }
        let id = new_id();
        let identity = self.io.identity(&op.recovery)?;
        let fingerprint = self.io.fingerprint(&op.recovery)?;
        let registration_order = Self::next_snapshot_order(state);
        state.snapshots.insert(
            id.clone(),
            Snapshot {
                id: id.clone(),
                game_id: op.game_id.clone(),
                original_data_dir: Some(op.live.clone()),
                registration_order,
                path: op.recovery.clone(),
                identity,
                fingerprint,
                removed_at: None,
                removal_reason: None,
                kind: SnapshotKind::Recovery,
                saved_at: Some(time),
                selection_time: time,
                discovered_at: time,
                available: true,
            },
        );
        Ok(Some(id))
    }
    fn finish_snapshot(&self, id: &str, kind: HistoryKind) -> Result<()> {
        let game_id = self.operation(id)?.game_id;
        let mut state = self.lock_game(&game_id)?;
        let mut next = state.clone();
        let op = next.operations[id].clone();
        let game = next.games[&op.game_id].clone();
        let time = self.clock.now_ms();
        let recovery = self.register_recovery(&mut next, &op, time)?;
        let snapshot_id = if kind == HistoryKind::Saved {
            let snapshot_id = new_id();
            let path = op
                .snapshot_path
                .clone()
                .ok_or_else(|| Error::new(ErrorCode::Storage, "missing published snapshot path"))?;
            let identity = self.io.identity(&path)?;
            let fingerprint = self.io.fingerprint(&path)?;
            let registration_order = Self::next_snapshot_order(&mut next);
            next.snapshots.insert(
                snapshot_id.clone(),
                Snapshot {
                    id: snapshot_id.clone(),
                    game_id: game.id.clone(),
                    original_data_dir: Some(op.live.clone()),
                    registration_order,
                    path,
                    identity,
                    fingerprint,
                    removed_at: None,
                    removal_reason: None,
                    kind: SnapshotKind::Saved,
                    saved_at: Some(time),
                    selection_time: time,
                    discovered_at: time,
                    available: true,
                },
            );
            Some(snapshot_id)
        } else {
            op.source_id
        };
        self.append_history(&mut next, &game, kind, snapshot_id, recovery, op.target_id);
        let operation = next.operations.get_mut(id).unwrap();
        operation.phase = Phase::Finished;
        operation.status = OperationStatus::Completed;
        self.commit(&mut state, next)
    }
    fn rollback_if_safe(&self, op: &Operation) -> Result<bool> {
        let state = self.lock()?.clone();
        self.validated(&state, &state.games[&op.game_id])?;
        if self.policy.resolve(&op.original)? != op.original {
            return Err(Error::new(
                ErrorCode::InvalidPath,
                "retained original path alias changed",
            ));
        }
        let live = self.io.exists(&op.live)?;
        let original = self.io.accessible_dir(&op.original)?;
        if !live
            && original
            && op.original_identity.as_ref() == Some(&self.io.identity(&op.original)?)
        {
            self.io.rename(&op.original, &op.live)?;
            return self.io.accessible_dir(&op.live);
        }
        // Intent was committed but the original rename never happened.
        Ok(op.phase == Phase::MovingOriginal && live && !original)
    }
    fn recover_startup(&self) -> Result<()> {
        let pending = self
            .lock()?
            .operations
            .values()
            .filter(|o| o.status == OperationStatus::Pending)
            .cloned()
            .collect::<Vec<_>>();
        for op in pending {
            let uncertain = matches!(
                op.phase,
                Phase::MovingOriginal
                    | Phase::OriginalMoved
                    | Phase::InstallingReplacement
                    | Phase::ReplacementInstalled
            );
            let safe = if uncertain {
                self.rollback_if_safe(&op).unwrap_or(false)
            } else {
                true
            };
            self.update(&op.id, |o| {
                o.status = if safe {
                    OperationStatus::Failed
                } else {
                    OperationStatus::RecoveryNeeded
                };
                o.error = Some(Error::new(
                    if safe {
                        ErrorCode::Io
                    } else {
                        ErrorCode::RecoveryNeeded
                    },
                    if safe && uncertain {
                        "interrupted operation: original data restored or unchanged"
                    } else {
                        "operation interrupted by host termination"
                    },
                ));
                if safe && uncertain {
                    o.resolution = Some("automatic_rollback".into());
                }
            })?;
        }
        Ok(())
    }
    fn resolve_recovery(&self, id: &str) -> Result<()> {
        let op = self.operation(id)?;
        let Action::Recover { operation, choice } = &op.action else {
            unreachable!()
        };
        let old = self.operation(operation)?;
        match choice {
            RecoveryChoice::KeepCurrent => {
                if !self.io.accessible_dir(&old.live)? {
                    return Err(Error::new(
                        ErrorCode::Unavailable,
                        "current data is missing or inaccessible",
                    ));
                }
                self.finish_recovery(id, "kept_current")
            }
            RecoveryChoice::Retry => {
                if self.rollback_if_safe(&old)? {
                    self.finish_recovery(id, "automatic_rollback")
                } else {
                    Err(Error::new(
                        ErrorCode::RecoveryNeeded,
                        "automatic recovery cannot overwrite current data; choose keep_current or restore_before",
                    ))
                }
            }
            RecoveryChoice::RestoreBefore => {
                let (source, identity) = if old.recovery_complete
                    && self.io.accessible_dir(&old.recovery)?
                    && old.recovery_identity.as_ref() == Some(&self.io.identity(&old.recovery)?)
                    && old.recovery_fingerprint.as_ref()
                        == Some(&self.io.fingerprint(&old.recovery)?)
                {
                    (old.recovery, old.recovery_identity)
                } else if self.io.accessible_dir(&old.original)?
                    && old.original_identity.as_ref() == Some(&self.io.identity(&old.original)?)
                {
                    (old.original, old.original_identity)
                } else {
                    return Err(Error::new(
                        ErrorCode::Unavailable,
                        "pre-operation recovery data is unavailable",
                    ));
                };
                let fingerprint = self.io.fingerprint(&source)?;
                self.update(id, |o| {
                    o.source = Some(source);
                    o.source_identity = identity;
                    o.source_fingerprint = Some(fingerprint);
                })?;
                self.restore(id, true)
            }
        }
    }
    fn finish_recovery(&self, id: &str, resolution: &str) -> Result<()> {
        let game_id = self.operation(id)?.game_id;
        let mut state = self.lock_game(&game_id)?;
        let mut next = state.clone();
        let op = next.operations[id].clone();
        self.register_recovery(&mut next, &op, self.clock.now_ms())?;
        let Action::Recover { operation, .. } = &op.action else {
            unreachable!()
        };
        // Resolving an interrupted recovery attempt must also resolve its ancestry.
        let mut current = Some(operation.clone());
        while let Some(key) = current {
            if !next.operations.contains_key(&key) {
                let old = self
                    .repository
                    .operation(&key)?
                    .ok_or_else(|| Error::new(ErrorCode::Storage, "missing recovery ancestry"))?;
                state.operations.insert(key.clone(), old.clone());
                next.operations.insert(key.clone(), old);
            }
            let old = next
                .operations
                .get_mut(&key)
                .ok_or_else(|| Error::new(ErrorCode::Storage, "missing recovery ancestry"))?;
            old.status = OperationStatus::Resolved;
            old.resolution = Some(resolution.into());
            current = if let Action::Recover { operation, .. } = &old.action {
                Some(operation.clone())
            } else {
                None
            };
        }
        let attempt = next.operations.get_mut(id).unwrap();
        attempt.status = OperationStatus::Resolved;
        attempt.phase = Phase::Finished;
        attempt.resolution = Some(resolution.into());
        self.commit(&mut state, next)
    }
    pub fn flush_preview(&self, game_id: &str) -> Result<FlushPreview> {
        self.refresh(game_id)?;
        let state = self.lock_game(game_id)?;
        Self::check_idle(&state, game_id)?;
        let mut paths = BTreeSet::new();
        let mut saved = 0;
        let mut recovery = 0;
        for snapshot in state
            .snapshots
            .values()
            .filter(|s| s.game_id == game_id && s.removed_at.is_none())
        {
            if self.io.exists(&snapshot.path)? {
                paths.insert(snapshot.path.clone());
                match snapshot.kind {
                    SnapshotKind::Saved => saved += 1,
                    SnapshotKind::Recovery => recovery += 1,
                }
            }
        }
        self.visit_operations(game_id, |operation| {
            for path in operation.retained_paths() {
                if self.io.exists(&path)? {
                    paths.insert(path);
                }
            }
            if let (Some(path), Some(identity)) =
                (&operation.snapshot_path, &operation.staging_identity)
                && self.io.exists(path)?
                && self.io.identity(path)? == *identity
            {
                paths.insert(path.clone());
            }
            Ok(())
        })?;
        Ok(FlushPreview {
            next_cursor: None,
            revision: state.revision,
            retained: paths.len().saturating_sub(saved + recovery),
            saved,
            recovery,
            paths: paths.into_iter().collect(),
        })
    }
    fn flush(&self, id: &str) -> Result<()> {
        self.cleanup(id, false)
    }
    fn forget(&self, id: &str) -> Result<()> {
        self.cleanup(id, true)
    }
    fn cleanup(&self, id: &str, forget: bool) -> Result<()> {
        let op = self.operation(id)?;
        let state = self.lock_game(&op.game_id)?.clone();
        if forget
            && state
                .games
                .get(&op.game_id)
                .is_none_or(|game| game.origin != GameOrigin::Custom)
        {
            return Err(Error::new(
                ErrorCode::InvalidTarget,
                "only custom games can be forgotten",
            ));
        }
        let mut paths = BTreeSet::new();
        for snapshot in state
            .snapshots
            .values()
            .filter(|s| s.game_id == op.game_id && s.removed_at.is_none())
        {
            paths.insert(snapshot.path.clone());
        }
        self.visit_operations(&op.game_id, |operation| {
            if operation.id == id {
                return Ok(());
            }
            paths.extend(operation.retained_paths());
            if let (Some(path), Some(identity)) =
                (&operation.snapshot_path, &operation.staging_identity)
                && self.io.exists(path)?
                && self.io.identity(path)? == *identity
            {
                paths.insert(path.clone());
            }
            Ok(())
        })?;
        let mut failure = None;
        for path in &paths {
            // Re-resolve each destination and all live paths before destructive work.
            let resolved = self.policy.resolve(path)?;
            if resolved != *path
                || state
                    .games
                    .values()
                    .any(|g| g.data_dir.starts_with(&resolved) || resolved.starts_with(&g.data_dir))
            {
                return Err(Error::new(
                    ErrorCode::InvalidPath,
                    "refusing to delete a changed alias or live data",
                ));
            }
            if let Err(error) = self.io.remove(path) {
                failure = Some(error);
            }
        }
        let mut guard = self.lock_game(&op.game_id)?;
        let mut next = guard.clone();
        for snapshot in next
            .snapshots
            .values_mut()
            .filter(|s| s.game_id == op.game_id)
        {
            snapshot.available = snapshot.removed_at.is_none()
                && self.io.accessible_dir(&snapshot.path).unwrap_or(false);
            if snapshot.removed_at.is_none() && !self.io.exists(&snapshot.path)? {
                snapshot.removed_at = Some(self.clock.now_ms());
                snapshot.removal_reason = Some(RemovalReason::Deleted);
            }
        }
        if failure.is_none() {
            next.snapshots.retain(|_, s| s.game_id != op.game_id);
            next.history.retain(|h| h.game_id != op.game_id);
            next.clear_history.push(op.game_id.clone());
            if forget {
                next.games.remove(&op.game_id);
                next.active_stack.retain(|game| game != &op.game_id);
                next.availability.remove(&op.game_id);
                next.history_status.remove(&op.game_id);
            }
            // Keep durable request IDs so retries cannot execute a second flush.
            for old in next
                .operations
                .values_mut()
                .filter(|o| o.game_id == op.game_id)
            {
                old.snapshot_path = None;
            }
        }
        let current = next.operations.get_mut(id).unwrap();
        current.status = if failure.is_some() {
            OperationStatus::Failed
        } else {
            OperationStatus::Completed
        };
        current.error = failure.clone();
        current.phase = Phase::Finished;
        self.commit(&mut guard, next)?;
        if forget
            && failure.is_none()
            && let Ok(mut cache) = self.summary_cache.lock()
        {
            cache.remove(&op.game_id);
        }
        if let Some(error) = failure {
            Err(error)
        } else {
            Ok(())
        }
    }
    pub fn record_activity(&self, stack: Vec<Id>, started: Vec<Id>, closed: Vec<Id>) -> Result<()> {
        let mut state = self.lock()?;
        if state.active_stack == stack && started.is_empty() && closed.is_empty() {
            return Ok(());
        }
        let mut next = state.clone();
        for (ids, kind) in [
            (started, HistoryKind::GameStarted),
            (closed, HistoryKind::GameClosed),
        ] {
            for id in ids {
                if let Some(game) = next.games.get(&id).cloned() {
                    self.append_history(&mut next, &game, kind, None, None, None);
                }
            }
        }
        next.active_stack = stack;
        self.commit(&mut state, next)
    }
}

struct ExecutionGuard<'a> {
    runtime: &'a Runtime,
    id: Id,
}
impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut running) = self.runtime.executing.lock() {
            running.remove(&self.id);
        }
    }
}
