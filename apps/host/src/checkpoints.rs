//! The checkpoint store: its availability, re-checking checkpoints changed
//! outside the app, cleaning up reserved leftovers, and the visible-history
//! index.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use savescummer_core::history::{RowFacts, RowKind, visibility};
use savescummer_core::{ErrorKind, Presence};
use savescummer_ipc::{OpStatus, Phase};
use savescummer_snapshots::{self as snap, is_reserved_folder};
use savescummer_storage::{self as db, CheckpointRow, HistoryRow};

use crate::host::{Host, Inner, new_id, now};
use crate::ops::Journal;

/// Decides whether the checkpoint store can be reached. The default store is
/// created when missing; a moved store on a missing drive is unavailable.
pub fn check_store(host: &Host) {
    let store = host.lock().store.clone();
    let default = host.data_dir.join("checkpoints");
    let available = match snap::presence(&store) {
        Presence::Present => store.is_dir(),
        Presence::Missing if savescummer_core::common::same_path(&store, &default, host.env.case_insensitive()) => {
            fs::create_dir_all(&store).is_ok()
        }
        _ => false,
    };
    let mut inner = host.lock();
    if inner.store == store && available != inner.store_available {
        inner.store_available = available;
        host.publish(&mut inner);
    }
}

/// Recomputes a game's visible history with the core's rule and writes only
/// the rows whose visibility flipped.
pub fn recompute_visibility(host: &Host, inner: &mut Inner, game_id: &str) {
    let mut storage = host.db();
    let inputs = db::visibility_inputs(storage.conn(), game_id).unwrap_or_default();
    let facts: Vec<RowFacts> = inputs
        .iter()
        .map(|(row, exists)| RowFacts { kind: row.kind, session: row.session.clone(), checkpoint_exists: *exists })
        .collect();
    let visible = visibility(&facts);
    let flips: Vec<(String, bool)> = inputs
        .iter()
        .zip(visible)
        .filter(|((row, _), v)| row.visible != *v)
        .map(|((row, _), v)| (row.id.clone(), v))
        .collect();
    if !flips.is_empty() {
        let _ = storage.write(|c| {
            for (id, v) in &flips {
                db::set_visible(c, id, *v)?;
            }
            Ok(())
        });
    }
    drop(storage);
    if !flips.is_empty() {
        host.bump_history(inner, game_id);
    }
}

/// The full-scan part: re-checks every checkpoint on disk.
///
/// - Gone: retire it.
/// - Replaced or edited: retire it and register the folder as a new
///   checkpoint (a changed recovery checkpoint loses its Revert target and
///   is never registered as a saved one).
/// - Can't tell: unavailable, not retired.
pub fn verify_all(host: &Arc<Host>) {
    let (store, available) = {
        let inner = host.lock();
        (inner.store.clone(), inner.store_available)
    };
    let records = db::all_live_checkpoints(host.db().conn()).unwrap_or_default();
    let mut touched: HashSet<String> = HashSet::new();
    for record in records {
        let path = store.join(&record.folder);
        if record.state == "deleting" {
            // A deletion that didn't finish: try again.
            if snap::remove_disposal(&path).is_ok() {
                let _ = db::set_checkpoint_state(host.db().conn(), &record.id, "deleted");
                touched.insert(record.game_id.clone());
            }
            continue;
        }
        let verdict = if !available { Verdict::Unknown } else { judge(&record, &path) };
        let state = match verdict {
            Verdict::Same => "ok",
            Verdict::Unknown => "unavailable",
            Verdict::Gone | Verdict::Changed => "retired",
        };
        if state != record.state {
            let _ = db::set_checkpoint_state(host.db().conn(), &record.id, state);
            touched.insert(record.game_id.clone());
        }
        if verdict == Verdict::Changed && record.kind == "saved" {
            register_generation(host, &record, &path);
        }
    }
    let mut inner = host.lock();
    for game in touched {
        recompute_visibility(host, &mut inner, &game);
        host.refresh_cache(&mut inner, &game);
        // Eligibility changes even when every row remains visible (for
        // example a checkpoint becoming readable again).
        host.bump_history(&mut inner, &game);
    }
    host.publish(&mut inner);
}

/// How long checkpoints are kept while "Flush checkpoints older than 30
/// days" is on.
pub const MAX_AGE_DAYS: i64 = 30;

/// The full-scan part of "Flush checkpoints older than 30 days": deletes
/// every older checkpoint, saved or recovery, of every game on record,
/// installed or not. A game that is busy or waits for a Retry keeps them
/// until a later scan; a checkpoint changed outside the app is left to
/// [`verify_all`].
pub fn flush_old(host: &Arc<Host>) {
    let store = {
        let inner = host.lock();
        if !inner.flush_old || !inner.store_available || inner.phase == Phase::ShuttingDown {
            return;
        }
        inner.store.clone()
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::days(MAX_AGE_DAYS);
    let mut old: BTreeMap<String, Vec<CheckpointRow>> = BTreeMap::new();
    for record in db::all_live_checkpoints(host.db().conn()).unwrap_or_default() {
        let created = chrono::DateTime::parse_from_rfc3339(&record.created_at);
        if record.state == "ok" && created.is_ok_and(|at| at < cutoff) {
            old.entry(record.game_id.clone()).or_default().push(record);
        }
    }
    for (game_id, records) in old {
        // Reserve the game like an operation, so nothing restores from a
        // checkpoint while it's disposed of.
        let op_id = new_id("op");
        let pending: HashSet<String> = {
            let mut inner = host.lock();
            if inner.store_moving || inner.busy.contains_key(&game_id) || inner.blocked.contains_key(&game_id) {
                continue;
            }
            let operation = crate::ops::op(&op_id, None, Some(&game_id), "flush", OpStatus::Running);
            inner.busy.insert(game_id.clone(), operation);
            host.publish(&mut inner);
            inner.deletes.values().filter_map(|d| d.op.checkpoint.clone()).collect()
        };
        let mut count = 0;
        for record in records.iter().filter(|r| !pending.contains(&r.id)) {
            let path = store.join(&record.folder);
            if judge(record, &path) != Verdict::Same {
                continue;
            }
            match snap::dispose(&path, &new_id("f")) {
                Ok(()) => {
                    let _ = db::set_checkpoint_state(host.db().conn(), &record.id, "deleted");
                    count += 1;
                }
                // Renamed but not removed: the next full scan finishes it.
                Err(snap::DisposeError::Remove(disposal, _)) => {
                    let relative =
                        disposal.strip_prefix(&store).unwrap_or(&disposal).to_string_lossy().replace('\\', "/");
                    let _ = host.db().write(|c| {
                        db::set_checkpoint_folder(c, &record.id, &relative)?;
                        db::set_checkpoint_state(c, &record.id, "deleting")
                    });
                }
                Err(snap::DisposeError::Rename(_)) => {}
            }
        }
        crate::trace(&format!("flushed {count} checkpoint(s) of {game_id} older than {MAX_AGE_DAYS} days"));
        let mut inner = host.lock();
        if inner.busy.get(&game_id).is_some_and(|b| b.id == op_id) {
            inner.busy.remove(&game_id);
        }
        recompute_visibility(host, &mut inner, &game_id);
        host.refresh_cache(&mut inner, &game_id);
        host.bump_history(&mut inner, &game_id);
        host.publish(&mut inner);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Same,
    Changed,
    Gone,
    Unknown,
}

/// Whether a checkpoint folder is still the generation that was recorded.
pub fn judge(record: &CheckpointRow, path: &Path) -> Verdict {
    match snap::presence(path) {
        Presence::Missing => Verdict::Gone,
        Presence::Unknown => Verdict::Unknown,
        Presence::Present => match snap::signature(path) {
            Ok(sig) => {
                // Only compared where both are known: a checkpoint recorded
                // before identities were dropped on this OS keeps its history.
                let same_identity =
                    record.identity.is_none() || sig.identity.is_none() || sig.identity == record.identity;
                if sig.hash == record.signature && same_identity { Verdict::Same } else { Verdict::Changed }
            }
            Err(_) => Verdict::Unknown,
        },
    }
}

/// Registers a replaced or edited saved checkpoint as a new generation with
/// new ids, unlabeled.
fn register_generation(host: &Host, old: &CheckpointRow, path: &Path) {
    let Ok(sig) = snap::signature(path) else { return };
    if sig.hash == "link" {
        return; // a checkpoint replaced by a link is never restored from
    }
    let targets = snap::read_meta(path).map(|m| m.targets).unwrap_or_else(|| old.targets.clone());
    let at = now();
    let row = CheckpointRow {
        seq: 0,
        id: new_id("cp"),
        game_id: old.game_id.clone(),
        kind: "saved".into(),
        folder: old.folder.clone(),
        created_at: old.created_at.clone(),
        label: None,
        targets,
        signature: sig.hash,
        identity: sig.identity,
        size: sig.size,
        state: "ok".into(),
    };
    let history = HistoryRow {
        seq: 0,
        id: new_id("row"),
        game_id: old.game_id.clone(),
        kind: RowKind::Saved,
        at,
        session: None,
        checkpoint_id: Some(row.id.clone()),
        recovery_id: None,
        reverted_row: None,
        removed: 0,
        cloud_check: false,
        cloud_replaced: false,
        visible: true,
    };
    let _ = host.db().write(|c| {
        db::insert_checkpoint(c, &row)?;
        db::insert_row(c, &history)?;
        Ok(())
    });
}

/// Removes reserved leftovers: temporary and disposal folders in the store
/// and `.ssold` files in targets, unless an unresolved interruption still
/// needs them. Measures what stays for the checkpoint size.
pub fn clean_up(host: &Arc<Host>) {
    let mut unfinished = db::unfinished_operations(host.db().conn()).unwrap_or_default();
    unfinished.extend(db::kept_operations(host.db().conn()).unwrap_or_default());
    let mut keep: HashSet<PathBuf> = HashSet::new();
    for op in &unfinished {
        if let Some(journal) = op.journal.as_ref().and_then(|j| serde_json::from_value::<Journal>(j.clone()).ok()) {
            keep.extend(journal.material());
        }
    }
    let deleting: HashSet<String> = db::all_live_checkpoints(host.db().conn())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.state == "deleting")
        .map(|c| c.folder)
        .collect();

    let (store, games) = {
        let inner = host.lock();
        let games: Vec<(String, PathBuf, Option<Vec<savescummer_core::Target>>)> = inner
            .games
            .values()
            .map(|g| {
                let targets = inner.derived.get(&g.id).and_then(|d| d.active.clone().ok());
                (g.id.clone(), inner.store.join(g.store_folder()), targets)
            })
            .collect();
        (inner.store.clone(), games)
    };
    let ci = host.env.case_insensitive();
    let mut leftover_sizes = Vec::new();
    for (game_id, folder, targets) in games {
        let mut leftover = 0;
        if let Ok(entries) = fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !is_reserved_folder(&name) {
                    continue;
                }
                let path = entry.path();
                let relative = format!("{}/{}", folder.file_name().unwrap_or_default().to_string_lossy(), name);
                if keep.contains(&path) || deleting.contains(&relative) {
                    leftover += snap::folder_size(&path);
                    continue;
                }
                if snap::remove_disposal(&path).is_err() {
                    leftover += snap::folder_size(&path);
                }
            }
        }
        for target in targets.unwrap_or_default() {
            if target.presence != Presence::Present {
                continue;
            }
            for old in savescummer_snapshots::load::find_leftover_old(&target.root, &target.filter, ci) {
                if !keep.contains(&old) {
                    let _ = fs::remove_file(&old);
                }
            }
        }
        leftover_sizes.push((game_id, leftover));
    }
    let _ = store;
    let mut inner = host.lock();
    for (game, size) in leftover_sizes {
        inner.caches.entry(game).or_default().leftover_size = size;
    }
    host.publish(&mut inner);
}

/// The kind of error an unavailable checkpoint reports.
pub fn unavailable_reason(record: &CheckpointRow, inner: &Inner, ci: bool) -> Option<ErrorKind> {
    let current = crate::host::current_pairs(inner, &record.game_id);
    if savescummer_core::common::commonality(&record.targets, &current, ci)
        == savescummer_core::common::Commonality::None
    {
        return Some(ErrorKind::DifferentSaveSet);
    }
    if !inner.store_available {
        return Some(ErrorKind::StoreUnavailable);
    }
    if record.state == "unavailable" {
        // Can't tell: unreadable for now, not changed.
        return Some(ErrorKind::CheckpointUnreadable);
    }
    None
}
