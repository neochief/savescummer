use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub fn action_checkpoint(entry: &History) -> Option<&str> {
    match entry.kind {
        HistoryKind::Saved | HistoryKind::ExistingBackup => entry.snapshot_id.as_deref(),
        HistoryKind::Loaded | HistoryKind::Reverted => entry.recovery_id.as_deref(),
        _ => None,
    }
}

/// Session context needed by a marker. Storage supplies indexed neighbours and
/// anchor existence; the core defines which interval and observation epoch count.
pub struct MarkerWindow {
    pub start: (u64, u64),
    pub end: Option<(u64, u64)>,
    pub inclusive: bool,
    pub observation_run: Option<Id>,
}
pub fn marker_window(
    entry: &History,
    previous: Option<&History>,
    next: Option<&History>,
) -> Option<MarkerWindow> {
    match entry.kind {
        HistoryKind::GameStarted => {
            let closed = next.is_some_and(|h| h.kind == HistoryKind::GameClosed);
            let same_run = next.is_some_and(|h| h.observation_run == entry.observation_run);
            Some(MarkerWindow {
                start: (entry.recorded_at, entry.sequence),
                end: next.map(|h| (h.recorded_at, h.sequence)),
                inclusive: closed,
                observation_run: (!(closed && same_run)).then(|| entry.observation_run.clone()),
            })
        }
        HistoryKind::GameClosed => {
            let start = previous.filter(|h| {
                h.kind == HistoryKind::GameStarted && h.observation_run == entry.observation_run
            })?;
            Some(MarkerWindow {
                start: (start.recorded_at, start.sequence),
                end: Some((entry.recorded_at, entry.sequence)),
                inclusive: true,
                observation_run: None,
            })
        }
        _ => None,
    }
}

/// Session markers provide context for surviving actions; audit history itself
/// remains intact so retiring a checkpoint never rewrites another action's target.
pub(crate) fn visible(state: &State) -> Vec<History> {
    let action_snapshot = |entry: &History| {
        match entry.kind {
            HistoryKind::Saved | HistoryKind::ExistingBackup => entry.snapshot_id.as_ref(),
            HistoryKind::Loaded | HistoryKind::Reverted => entry.recovery_id.as_ref(),
            _ => None,
        }
        .and_then(|id| state.snapshots.get(id))
    };
    let mut ids = BTreeSet::new();
    let mut anchors = BTreeMap::<Id, Vec<((u64, u64), Id)>>::new();
    for entry in &state.history {
        if let Some(snapshot) = action_snapshot(entry).filter(|s| s.removed_at.is_none()) {
            ids.insert(entry.id.clone());
            let time = if entry.kind == HistoryKind::ExistingBackup {
                snapshot.selection_time
            } else {
                entry.recorded_at
            };
            anchors
                .entry(entry.game_id.clone())
                .or_default()
                .push(((time, entry.sequence), entry.observation_run.clone()));
        }
    }
    let mut sessions = BTreeMap::<Id, &History>::new();
    for entry in &state.history {
        let key = entry.game_id.clone();
        match entry.kind {
            HistoryKind::GameStarted => {
                if let Some(start) = sessions.insert(key.clone(), entry)
                    && anchors.get(&key).is_some_and(|times| {
                        times.iter().any(|(time, run)| {
                            run == &start.observation_run
                                && *time >= (start.recorded_at, start.sequence)
                                && *time < (entry.recorded_at, entry.sequence)
                        })
                    })
                {
                    ids.insert(start.id.clone());
                }
            }
            HistoryKind::GameClosed => {
                if let Some(start) = sessions.remove(&key)
                    && anchors.get(&key).is_some_and(|times| {
                        times.iter().any(|(time, run)| {
                            *time >= (start.recorded_at, start.sequence)
                                && *time <= (entry.recorded_at, entry.sequence)
                                && (start.observation_run == entry.observation_run
                                    || run == &start.observation_run)
                        })
                    })
                {
                    ids.insert(start.id.clone());
                    if start.observation_run == entry.observation_run {
                        ids.insert(entry.id.clone());
                    }
                }
            }
            _ => (),
        }
    }
    for (key, start) in sessions {
        if anchors.get(&key).is_some_and(|times| {
            times.iter().any(|(time, run)| {
                run == &start.observation_run && *time >= (start.recorded_at, start.sequence)
            })
        }) {
            ids.insert(start.id.clone());
        }
    }
    let mut visible = state
        .history
        .iter()
        .filter(|h| ids.contains(&h.id))
        .cloned()
        .collect::<Vec<_>>();
    visible.sort_by_key(|h| {
        let time = if h.kind == HistoryKind::ExistingBackup {
            action_snapshot(h).map_or(h.recorded_at, |s| s.selection_time)
        } else {
            h.recorded_at
        };
        (time, h.sequence)
    });
    visible
}
