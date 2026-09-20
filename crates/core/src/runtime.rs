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
}

impl Runtime {
    pub fn open(
        repository: Arc<dyn Repository>,
        io: Arc<dyn SnapshotIo>,
        policy: Arc<dyn PathPolicy>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let mut state = repository.load()?;
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
        };
        runtime.recover_startup()?;
        for game_id in runtime.state()?.games.keys() {
            if let Err(error) = runtime.refresh(game_id)
                && error.code == ErrorCode::Storage
            {
                return Err(error);
            }
        }
        Ok(runtime)
    }
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Storage, "runtime state lock poisoned"))
    }
    fn commit(&self, guard: &mut State, mut next: State) -> Result<()> {
        next.revision = guard.revision + 1;
        self.repository.commit(&next)?;
        *guard = next;
        Ok(())
    }
    pub fn settings(&self) -> Result<Settings> {
        Ok(self.lock()?.settings.clone())
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
            for snapshot in state.snapshots.values() {
                if self.checkpoint_eligible(snapshot, game, SnapshotKind::Saved)
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
        Ok(self
            .lock()?
            .operations
            .values()
            .find(|operation| operation.request_id == request_id)
            .map(|operation| operation.id.clone()))
    }
    pub fn state(&self) -> Result<State> {
        let mut state = self.lock()?.clone();
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
            current.availability = state.availability.clone();
            current.revision += 1;
            state.revision = current.revision;
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
    fn next_snapshot_order(state: &State) -> u64 {
        state
            .snapshots
            .values()
            .map(|s| s.registration_order)
            .max()
            .unwrap_or(0)
            + 1
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
        // Reserve all retained snapshot locations, including previous data locations.
        for snapshot in state.snapshots.values().filter(|s| s.removed_at.is_none()) {
            if data_dir.starts_with(&snapshot.path) || snapshot.path.starts_with(&data_dir) {
                return Err(Error::new(
                    ErrorCode::InvalidPath,
                    "data directory overlaps retained snapshot data",
                ));
            }
        }
        for operation in state.operations.values() {
            for retained in operation.retained_paths() {
                if data_dir.starts_with(&retained) || retained.starts_with(&data_dir) {
                    return Err(Error::new(
                        ErrorCode::InvalidPath,
                        "data directory overlaps retained operation data",
                    ));
                }
            }
        }
        let executables = executables
            .iter()
            .map(|p| self.policy.resolve(p))
            .collect::<Result<Vec<_>>>()?;
        let game = Game {
            id: id.clone(),
            name,
            info: state
                .games
                .get(&id)
                .map(|g| g.info.clone())
                .unwrap_or_default(),
            data_dir,
            executables,
            installed: true,
            configuration_error: None,
            detected_locations: state
                .games
                .get(&id)
                .map(|g| g.detected_locations.clone())
                .unwrap_or_default(),
            user_configured: true,
        };
        let mut next = state.clone();
        next.games.insert(id, game.clone());
        self.commit(&mut state, next)?;
        Ok(game)
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
            if let Err(error) = self.configure(
                id.clone(),
                name.clone(),
                location.data_dir.clone(),
                location.executables.clone(),
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
            info: info.clone(),
            data_dir: locations[0].data_dir.clone(),
            executables: locations[0].executables.clone(),
            installed: true,
            configuration_error: failure.clone(),
            detected_locations: vec![],
            user_configured: false,
        });
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
            state.discovery_errors = errors;
            state.revision += 1;
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
        let mut state = self.lock()?;
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
            if state.operations.values().any(|o| {
                o.status != OperationStatus::Completed
                    && o.snapshot_path.as_ref() == Some(&path)
                    && o.staging_identity.as_ref() == Some(&identity)
            }) {
                continue;
            }
            let snapshot = Snapshot {
                id: new_id(),
                game_id: game.id.clone(),
                original_data_dir: Some(game.data_dir.clone()),
                registration_order: Self::next_snapshot_order(&next),
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
            let state = self.lock()?;
            if let Some(op) = state
                .operations
                .values()
                .find(|o| o.request_id == request_id)
            {
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
            Action::Save | Action::Load { .. } | Action::Revert { .. } | Action::Flush { .. }
        ) {
            self.refresh(game_id)?;
        }
        let mut state = self.lock()?;
        if let Some(op) = state
            .operations
            .values()
            .find(|o| o.request_id == request_id)
        {
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
        if let Action::Flush { confirmed_revision } = &action
            && *confirmed_revision != state.revision
        {
            return Err(Error::new(
                ErrorCode::ConfirmationRequired,
                "obtain a new flush preview and confirm its revision",
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
            _ => return Ok((None, None)),
        };
        let snapshot = if let Some(id) = target {
            state
                .snapshots
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::InvalidTarget, "unknown checkpoint ID"))?
        } else {
            state
                .snapshots
                .values()
                .filter(|s| self.checkpoint_eligible(s, game, kind))
                .max_by_key(|s| (s.selection_time, s.registration_order))
                .ok_or_else(|| {
                    Error::new(ErrorCode::Unavailable, "no available saved checkpoint")
                })?
        };
        if !self.checkpoint_matches(snapshot, game, kind) {
            return Err(Error::new(
                ErrorCode::InvalidTarget,
                "checkpoint belongs to another game, data directory, or action kind",
            ));
        }
        // History is optional audit context, never the source of eligibility.
        let history = state.history.iter().find(|h| {
            h.game_id == game.id
                && match kind {
                    SnapshotKind::Saved => {
                        matches!(h.kind, HistoryKind::Saved | HistoryKind::ExistingBackup)
                            && h.snapshot_id.as_ref() == Some(&snapshot.id)
                    }
                    SnapshotKind::Recovery => {
                        matches!(h.kind, HistoryKind::Loaded | HistoryKind::Reverted)
                            && h.recovery_id.as_ref() == Some(&snapshot.id)
                    }
                }
        });
        Ok((Some(snapshot.clone()), history.map(|h| h.id.clone())))
    }
    pub fn operation(&self, id: &str) -> Result<Operation> {
        self.lock()?
            .operations
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown operation"))
    }
    fn update(&self, id: &str, change: impl FnOnce(&mut Operation)) -> Result<()> {
        let mut state = self.lock()?;
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
            Action::Flush { .. } => self.flush(id),
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
                state.operations.insert(id.into(), current);
                state.revision += 1;
                return Err(storage_error);
            }
            return Err(error);
        }
        Ok(())
    }
    fn save(&self, id: &str) -> Result<()> {
        let op = self.operation(id)?;
        self.copy(id, &op.live, &op.staging)?;
        let identity = self.io.identity(&op.staging)?;
        self.update(id, |o| o.staging_identity = Some(identity))?;
        for _ in 0..100 {
            let state = self.state()?;
            let reserved = state
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
            let destination = self.io.next_saved_path(&op.live, &reserved)?;
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
        let sequence = state.history.last().map_or(1, |h| h.sequence + 1);
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
        state.snapshots.insert(
            id.clone(),
            Snapshot {
                id: id.clone(),
                game_id: op.game_id.clone(),
                original_data_dir: Some(op.live.clone()),
                registration_order: Self::next_snapshot_order(state),
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
        let mut state = self.lock()?;
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
            next.snapshots.insert(
                snapshot_id.clone(),
                Snapshot {
                    id: snapshot_id.clone(),
                    game_id: game.id.clone(),
                    original_data_dir: Some(op.live.clone()),
                    registration_order: Self::next_snapshot_order(&next),
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
        let state = self.state()?;
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
            .state()?
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
        let mut state = self.lock()?;
        let mut next = state.clone();
        let op = next.operations[id].clone();
        self.register_recovery(&mut next, &op, self.clock.now_ms())?;
        let Action::Recover { operation, .. } = &op.action else {
            unreachable!()
        };
        // Resolving an interrupted recovery attempt must also resolve its ancestry.
        let mut current = Some(operation.clone());
        while let Some(key) = current {
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
        let state = self.lock()?;
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
        for operation in state.operations.values().filter(|o| o.game_id == game_id) {
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
        }
        Ok(FlushPreview {
            revision: state.revision,
            retained: paths.len().saturating_sub(saved + recovery),
            saved,
            recovery,
            paths: paths.into_iter().collect(),
        })
    }
    fn flush(&self, id: &str) -> Result<()> {
        let op = self.operation(id)?;
        let state = self.state()?;
        let mut paths = BTreeSet::new();
        for snapshot in state
            .snapshots
            .values()
            .filter(|s| s.game_id == op.game_id && s.removed_at.is_none())
        {
            paths.insert(snapshot.path.clone());
        }
        for operation in state
            .operations
            .values()
            .filter(|o| o.game_id == op.game_id && o.id != id)
        {
            paths.extend(operation.retained_paths());
            if let (Some(path), Some(identity)) =
                (&operation.snapshot_path, &operation.staging_identity)
                && self.io.exists(path)?
                && self.io.identity(path)? == *identity
            {
                paths.insert(path.clone());
            }
        }
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
        let mut guard = self.lock()?;
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
