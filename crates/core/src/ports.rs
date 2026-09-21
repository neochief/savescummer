use crate::*;
use std::path::{Path, PathBuf};

/// Atomic metadata batches and indexed read operations. No filesystem work occurs
/// in a commit. Defaults support small in-memory test repositories.
pub trait Repository: Send + Sync {
    fn load(&self) -> Result<State>;
    fn commit_changes(&self, changes: &MetadataChanges) -> Result<()>;
    /// Explicit fixture/import operation; runtime mutations use commit_changes.
    fn commit(&self, state: &State) -> Result<()> {
        self.commit_changes(&MetadataChanges::between(&self.load()?, state))
    }
    fn boot(&self) -> Result<State> {
        let mut state = self.load()?;
        state.history_sequence = state
            .history_sequence
            .max(state.history.iter().map(|h| h.sequence).max().unwrap_or(0));
        state.snapshot_order = state.snapshot_order.max(
            state
                .snapshots
                .values()
                .map(|s| s.registration_order)
                .max()
                .unwrap_or(0),
        );
        state.history.clear();
        state.snapshots.clear();
        retain_current_operations(&mut state);
        Ok(state)
    }
    fn game_snapshots(&self, game: &str) -> Result<Vec<Snapshot>> {
        Ok(self
            .load()?
            .snapshots
            .into_values()
            .filter(|s| s.game_id == game && s.removed_at.is_none())
            .collect())
    }
    fn game_operations(&self, game: &str) -> Result<Vec<Operation>> {
        Ok(self
            .load()?
            .operations
            .into_values()
            .filter(|o| o.game_id == game)
            .collect())
    }
    fn operation_page(&self, game: &str, after: &str, limit: usize) -> Result<Vec<Operation>> {
        let mut rows = self.game_operations(game)?;
        rows.retain(|o| o.id.as_str() > after);
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows.truncate(limit);
        Ok(rows)
    }
    fn unpublished_snapshot(&self, game: &str, path: &Path, identity: &str) -> Result<bool> {
        Ok(self.game_operations(game)?.iter().any(|o| {
            o.status != OperationStatus::Completed
                && o.snapshot_path.as_deref() == Some(path)
                && o.staging_identity.as_deref() == Some(identity)
        }))
    }
    fn reserved_snapshot_paths(&self, game: &str) -> Result<Vec<PathBuf>> {
        Ok(self
            .game_operations(game)?
            .into_iter()
            .filter(|o| o.status != OperationStatus::Completed)
            .filter_map(|o| o.snapshot_path)
            .collect())
    }
    fn visit_reserved_paths(&self, visit: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
        for path in self.reserved_paths()? {
            visit(&path)?;
        }
        Ok(())
    }
    fn snapshot(&self, id: &str) -> Result<Option<Snapshot>> {
        Ok(self.load()?.snapshots.remove(id))
    }
    fn snapshot_at_path(&self, path: &Path) -> Result<Option<Snapshot>> {
        Ok(self
            .load()?
            .snapshots
            .into_values()
            .find(|s| s.path == path && s.removed_at.is_none()))
    }
    fn snapshot_path_reserved(&self, path: &Path) -> Result<bool> {
        let state = self.load()?;
        Ok(state
            .snapshots
            .values()
            .any(|s| s.path == path && s.removed_at.is_none())
            || state.operations.values().any(|o| {
                o.status != OperationStatus::Completed && o.snapshot_path.as_deref() == Some(path)
            }))
    }
    fn operation(&self, id: &str) -> Result<Option<Operation>> {
        Ok(self.load()?.operations.remove(id))
    }
    fn request_operation(&self, request: &str) -> Result<Option<Operation>> {
        Ok(self
            .load()?
            .operations
            .into_values()
            .find(|o| o.request_id == request))
    }
    fn history_entry(&self, id: &str) -> Result<Option<History>> {
        Ok(self.load()?.history.into_iter().find(|h| h.id == id))
    }
    fn checkpoint_history(&self, id: &str) -> Result<Option<History>> {
        Ok(self.load()?.history.into_iter().find(|h| match h.kind {
            HistoryKind::Saved | HistoryKind::ExistingBackup => {
                h.snapshot_id.as_deref() == Some(id)
            }
            HistoryKind::Loaded | HistoryKind::Reverted => h.recovery_id.as_deref() == Some(id),
            _ => false,
        }))
    }
    fn reserved_paths(&self) -> Result<Vec<PathBuf>> {
        let state = self.load()?;
        Ok(state
            .snapshots
            .values()
            .filter(|s| s.removed_at.is_none())
            .map(|s| s.path.clone())
            .chain(state.operations.values().flat_map(|o| o.retained_paths()))
            .collect())
    }
    fn history_status(&self, game: &str) -> Result<HistoryStatus> {
        let state = self.load()?;
        Ok(HistoryStatus {
            revision: state.revision,
            has_visible_history: crate::history::visible(&state)
                .iter()
                .any(|h| h.game_id == game),
            can_flush: state.history.iter().any(|h| h.game_id == game)
                || state.snapshots.values().any(|s| s.game_id == game)
                || state.operations.values().any(|o| o.game_id == game),
        })
    }
    fn history_rows(&self, game: &str, before: u64, limit: usize) -> Result<Vec<History>> {
        let state = self.load()?;
        let mut rows = crate::history::visible(&state);
        rows.retain(|h| h.game_id == game && h.sequence < before);
        rows.sort_by_key(|h| std::cmp::Reverse(h.sequence));
        rows.truncate(limit);
        Ok(rows)
    }
}

pub fn retain_current_operations(state: &mut State) {
    let mut latest = std::collections::BTreeMap::new();
    for o in state.operations.values().filter(|o| !o.blocks()) {
        let key = (o.started_at, o.id.clone());
        let entry = latest
            .entry(o.game_id.clone())
            .or_insert_with(|| key.clone());
        if key > *entry {
            *entry = key;
        }
    }
    state
        .operations
        .retain(|_, o| o.blocks() || latest.get(&o.game_id).is_some_and(|(_, id)| *id == o.id));
}
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}
pub trait PathPolicy: Send + Sync {
    fn resolve(&self, path: &Path) -> Result<PathBuf>;
    fn validate(&self, path: &Path, others: &[(Id, PathBuf)]) -> Result<PathBuf>;
    fn same_location(&self, recorded: &Path, current: &Path) -> bool;
}
/// Copies must create their destination exclusively, reject links/reparse points,
/// and retain partial destinations on failure. Rename must never replace a path.
pub trait SnapshotIo: Send + Sync {
    fn accessible_dir(&self, path: &Path) -> Result<bool>;
    fn exists(&self, path: &Path) -> Result<bool>;
    fn identity(&self, path: &Path) -> Result<String>;
    fn modified_ms(&self, path: &Path) -> Result<u64>;
    fn fingerprint(&self, path: &Path) -> Result<String>;
    fn copy(&self, source: &Path, destination: &Path, progress: &mut dyn FnMut(u64)) -> Result<()>;
    fn rename(&self, source: &Path, destination: &Path) -> Result<()>;
    fn saved_candidates(&self, live: &Path) -> Result<Vec<PathBuf>>;
    fn next_saved_path(&self, live: &Path, reserved: &[PathBuf]) -> Result<PathBuf>;
    fn remove(&self, path: &Path) -> Result<()>;
}
