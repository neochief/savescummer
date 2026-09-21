use crate::*;
use std::collections::BTreeMap;

/// One atomic durable mutation. Empty collections never mean "delete all".
#[derive(Debug, Clone, Default)]
pub struct MetadataChanges {
    pub revision: u64,
    pub history_sequence: u64,
    pub snapshot_order: u64,
    pub settings: Option<Settings>,
    pub games: Vec<Game>,
    pub snapshots: Vec<Snapshot>,
    pub history: Vec<History>,
    pub operations: Vec<Operation>,
    pub deleted_games: Vec<Id>,
    pub deleted_snapshots: Vec<Id>,
    pub deleted_history: Vec<Id>,
    pub deleted_operations: Vec<Id>,
    pub clear_history: Vec<Id>,
}

fn changed<T: Clone + PartialEq>(
    before: &BTreeMap<Id, T>,
    after: &BTreeMap<Id, T>,
) -> (Vec<T>, Vec<Id>) {
    (
        after
            .iter()
            .filter(|(id, value)| before.get(*id) != Some(*value))
            .map(|(_, value)| value.clone())
            .collect(),
        before
            .keys()
            .filter(|id| !after.contains_key(*id))
            .cloned()
            .collect(),
    )
}

impl MetadataChanges {
    /// The caller supplies only its working set, never a whole-library history.
    pub fn between(before: &State, after: &State) -> Self {
        let (games, deleted_games) = changed(&before.games, &after.games);
        let (snapshots, deleted_snapshots) = changed(&before.snapshots, &after.snapshots);
        let (operations, deleted_operations) = changed(&before.operations, &after.operations);
        let old = before
            .history
            .iter()
            .map(|h| (h.id.clone(), h.clone()))
            .collect();
        let new = after
            .history
            .iter()
            .map(|h| (h.id.clone(), h.clone()))
            .collect();
        let (history, deleted_history) = changed(&old, &new);
        Self {
            revision: after.revision,
            history_sequence: after.history_sequence,
            snapshot_order: after.snapshot_order,
            settings: (before.settings != after.settings).then(|| after.settings.clone()),
            games,
            snapshots,
            history,
            operations,
            deleted_games,
            deleted_snapshots,
            deleted_history,
            deleted_operations,
            clear_history: after.clear_history.clone(),
        }
    }
    pub fn apply(&self, state: &mut State) {
        state.revision = self.revision;
        state.history_sequence = self.history_sequence;
        state.snapshot_order = self.snapshot_order;
        if let Some(settings) = &self.settings {
            state.settings = settings.clone();
        }
        for id in &self.deleted_games {
            state.games.remove(id);
        }
        for id in &self.deleted_snapshots {
            state.snapshots.remove(id);
        }
        for id in &self.deleted_operations {
            state.operations.remove(id);
        }
        state.history.retain(|h| {
            !self.deleted_history.contains(&h.id) && !self.clear_history.contains(&h.game_id)
        });
        state
            .snapshots
            .retain(|_, s| !self.clear_history.contains(&s.game_id));
        for operation in state.operations.values_mut() {
            if self.clear_history.contains(&operation.game_id) {
                operation.snapshot_path = None;
            }
        }
        for g in &self.games {
            state.games.insert(g.id.clone(), g.clone());
        }
        for s in &self.snapshots {
            state.snapshots.insert(s.id.clone(), s.clone());
        }
        for o in &self.operations {
            state.operations.insert(o.id.clone(), o.clone());
        }
        for h in &self.history {
            state.history.retain(|old| old.id != h.id);
            state.history.push(h.clone());
        }
        state.history.sort_by_key(|h| h.sequence);
    }
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct HistoryStatus {
    pub revision: u64,
    pub has_visible_history: bool,
    pub can_flush: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct HistoryRow {
    #[serde(flatten)]
    pub entry: History,
    pub display_time: u64,
    pub target_time: Option<u64>,
    pub action: Option<Action>,
    pub available: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct HistoryPage {
    pub game_id: Id,
    pub revision: u64,
    pub rows: Vec<HistoryRow>,
    pub next_cursor: Option<String>,
}

/// The wire summary grows with the library, not with retained audit records.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct LibraryState {
    pub revision: u64,
    #[serde(default)]
    pub scan_in_progress: bool,
    pub artwork_revision: u64,
    pub artwork: BTreeMap<Id, GameArtwork>,
    pub settings: Settings,
    pub games: BTreeMap<Id, Game>,
    pub active_stack: Vec<Id>,
    pub availability: BTreeMap<Id, GameAvailability>,
    /// Only the default saved checkpoint for each game, never all checkpoints.
    pub snapshots: BTreeMap<Id, Snapshot>,
    /// Blocking operations and the most recent terminal result per game.
    pub operations: BTreeMap<Id, Operation>,
    pub history_status: BTreeMap<Id, HistoryStatus>,
    pub discovery_errors: Vec<String>,
}

impl From<State> for LibraryState {
    fn from(s: State) -> Self {
        let game_ids = s
            .games
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        Self {
            revision: s.revision,
            scan_in_progress: false,
            artwork_revision: s.artwork_revision,
            artwork: s.artwork,
            settings: s.settings,
            games: s.games,
            active_stack: s.active_stack,
            availability: s.availability,
            snapshots: s.snapshots,
            operations: s
                .operations
                .into_iter()
                .filter(|(_, operation)| game_ids.contains(&operation.game_id))
                .collect(),
            history_status: s.history_status,
            discovery_errors: s.discovery_errors,
        }
    }
}
