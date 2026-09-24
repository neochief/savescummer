//! Recovery at startup: every unfinished operation is resolved by the four
//! fixed rules before its game can be operated on again, with or without a
//! UI. Never roll back over data that may be newer, never retry the
//! requested operation on its own, and never delete kept material while an
//! interruption is unresolved.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use savescummer_core::history::RowKind;
use savescummer_core::recovery::{Rule, classify_load};
use savescummer_core::{ErrorKind, Failure, Presence};
use savescummer_ipc::OpResult;
use savescummer_snapshots::{self as snap, load};
use savescummer_storage::{self as db, CheckpointRow, HistoryRow, OperationRow};

use crate::host::{Host, new_id, now};
use crate::ops::{Journal, restore_row};

/// Resolves every unfinished operation. Called once at startup.
pub fn resolve_all(host: &Arc<Host>) {
    let unfinished = db::unfinished_operations(host.db().conn()).unwrap_or_default();
    for op in unfinished {
        resolve(host, &op);
    }
}

/// Resolves one game's unfinished operations again (Retry on a blocked game).
pub fn retry(host: &Arc<Host>, game_id: &str) -> Result<(), Failure> {
    let unfinished = db::unfinished_operations(host.db().conn()).unwrap_or_default();
    for op in unfinished.iter().filter(|o| o.game_id.as_deref() == Some(game_id)) {
        resolve(host, op);
    }
    let mut inner = host.lock();
    host.publish(&mut inner);
    match inner.blocked.get(game_id) {
        Some(f) => Err(f.clone()),
        None => Ok(()),
    }
}

fn finish(host: &Host, op: &OperationRow, status: &str, result: Option<OpResult>, error: Option<Failure>) {
    let value = serde_json::json!({ "result": result, "error": error });
    let _ = db::finish_operation(host.db().conn(), &op.id, status, &value, &now());
}

fn resolve(host: &Arc<Host>, op: &OperationRow) {
    let journal: Journal = op.journal.as_ref().and_then(|j| serde_json::from_value(j.clone()).ok()).unwrap_or_default();
    let game = op.game_id.clone().unwrap_or_default();
    match op.kind.as_str() {
        "save" => resolve_save(host, op, &journal, &game),
        "load" | "revert" => resolve_restore(host, op, &journal, &game),
        "delete" => resolve_delete(host, op, &journal),
        "move_store" => resolve_move(host, op, &journal),
        // An interrupted Flush: the next full scan retires what's gone.
        _ => finish(host, op, "failed", None, Some(Failure::new(ErrorKind::ShuttingDown, "interrupted"))),
    }
}

/// A published copy of this operation, recognized by its record.
fn published_by(folder: Option<&Path>, op_id: &str, kind: &str) -> Option<std::path::PathBuf> {
    let entries = fs::read_dir(folder?).ok()?;
    entries.flatten().map(|e| e.path()).find(|path| {
        !snap::is_reserved_folder(&path.file_name().unwrap_or_default().to_string_lossy())
            && snap::read_meta(path).is_some_and(|m| m.operation.as_deref() == Some(op_id) && m.kind == kind)
    })
}

fn register(host: &Host, path: &Path, game: &str, kind: &str) -> Option<CheckpointRow> {
    let meta = snap::read_meta(path)?;
    let signature = snap::signature(path).ok()?;
    let store = host.lock().store.clone();
    Some(CheckpointRow {
        seq: 0,
        id: new_id("cp"),
        game_id: game.to_string(),
        kind: kind.to_string(),
        folder: path.strip_prefix(&store).unwrap_or(path).to_string_lossy().replace('\\', "/"),
        created_at: meta.created_at,
        label: None,
        targets: meta.targets,
        signature: signature.hash,
        identity: signature.identity,
        size: signature.size,
        state: "ok".into(),
    })
}

/// A Save: published but not recorded → finish it (R3); otherwise nothing
/// live changed → delete the temporary folder, fail, and leave a notice (R1).
fn resolve_save(host: &Arc<Host>, op: &OperationRow, j: &Journal, game: &str) {
    if let Some(path) = published_by(j.game_folder.as_deref(), &op.id, "saved")
        && let Some(mut checkpoint) = register(host, &path, game, "saved")
    {
        checkpoint.label = j.label.clone();
        let row = HistoryRow {
            seq: 0,
            id: new_id("row"),
            game_id: game.to_string(),
            kind: RowKind::Saved,
            at: checkpoint.created_at.clone(),
            session: j.session.clone(),
            checkpoint_id: Some(checkpoint.id.clone()),
            recovery_id: None,
            reverted_row: None,
            removed: 0,
            cloud_check: false,
            cloud_replaced: false,
            visible: true,
        };
        let result = OpResult { checkpoint: Some(checkpoint.id.clone()), ..Default::default() };
        let value = serde_json::json!({ "result": result });
        let _ = host.db().write(|c| {
            db::insert_checkpoint(c, &checkpoint)?;
            db::insert_row(c, &row)?;
            db::finish_operation(c, &op.id, "succeeded", &value, &now())
        });
        return;
    }
    if let Some(temp) = &j.temp {
        let _ = snap::remove_disposal(temp);
    }
    let at = now();
    let _ = db::add_notice(host.db().conn(), game, "save_interrupted", &at);
    host.lock().notices.insert(game.to_string(), "save_interrupted".into());
    finish(
        host,
        op,
        "failed",
        None,
        Some(Failure::new(ErrorKind::ShuttingDown, "the save was interrupted").game(game)),
    );
}

fn discard_recovery(host: &Host, op: &OperationRow, j: &Journal) {
    let store = host.lock().store.clone();
    // Bind first: a guard in an `if let` condition lives through its body.
    let record = j.recovery.as_ref().and_then(|id| db::checkpoint(host.db().conn(), id).ok().flatten());
    if let (Some(id), Some(record)) = (&j.recovery, record) {
        let _ = snap::dispose(&store.join(&record.folder), &new_id("x"));
        let _ = db::set_checkpoint_state(host.db().conn(), id, "deleted");
    }
    if let Some(path) = published_by(j.game_folder.as_deref(), &op.id, "recovery") {
        let _ = snap::dispose(&path, &new_id("x"));
        let relative = path.strip_prefix(&store).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let _ = host.db().conn().execute("UPDATE checkpoints SET state = 'deleted' WHERE folder = ?1", [relative]);
    }
    if let Some(temp) = &j.temp {
        let _ = snap::remove_disposal(temp);
    }
}

fn resolve_restore(host: &Arc<Host>, op: &OperationRow, j: &Journal, game: &str) {
    let Some(plan) = j.plan.clone().filter(|_| j.phase.ends_with(".apply")) else {
        // Interrupted while copying the recovery checkpoint: nothing live
        // changed (R1). Silent.
        discard_recovery(host, op, j);
        finish(host, op, "failed", None, Some(Failure::new(ErrorKind::ShuttingDown, "interrupted").game(game)));
        return;
    };
    // Every target the plan touches must be readable to verify anything.
    let unreadable = plan.files.iter().any(|f| f.live.parent().is_some_and(|p| snap::presence(p) == Presence::Unknown));
    let rule = if unreadable { Rule::R4 } else { classify_load(&load::observe(&plan)) };
    let recovered = load::recover(&plan, rule);
    match (rule, recovered) {
        (Rule::R1 | Rule::R2, Ok(_)) => {
            // The operation never happened.
            discard_recovery(host, op, j);
            host.lock().blocked.remove(game);
            finish(host, op, "failed", None, Some(Failure::new(ErrorKind::ShuttingDown, "interrupted").game(game)));
        }
        (Rule::R3, Ok(_)) => {
            let kind = op.kind.as_str();
            let row = restore_row(
                game,
                kind,
                j.checkpoint.as_deref().unwrap_or_default(),
                j.recovery.as_deref().unwrap_or_default(),
                j.reverted_row.clone(),
                plan.removed_files() as u32,
                j.session.clone(),
                j.cloud_check,
            );
            let result = OpResult {
                checkpoint: j.checkpoint.clone(),
                recovery: j.recovery.clone(),
                removed_files: Some(plan.removed_files() as u32),
                ..Default::default()
            };
            let value = serde_json::json!({ "result": result });
            let committed = host.db().write(|c| {
                db::insert_row(c, &row)?;
                db::finish_operation(c, &op.id, "succeeded", &value, &now())
            });
            if committed.is_ok() {
                host.lock().blocked.remove(game);
            } else {
                block(
                    host,
                    op,
                    game,
                    Failure::new(ErrorKind::NotRecorded, "the load was applied but couldn't be recorded"),
                );
            }
        }
        (_, result) => {
            // R4, or an undo that failed: keep every file exactly as it is.
            let detail = match result {
                Err(f) => f.detail,
                Ok(_) => "the saves don't match what the interrupted load recorded".into(),
            };
            let paths: Vec<_> = plan.files.iter().map(|f| f.live.clone()).collect();
            let failure = Failure::new(ErrorKind::RollbackFailed, detail).game(game).paths(paths);
            if load::every_name_live(&plan) {
                // Coherent: release the game, keep the material ("kept"
                // operations are never cleaned up as leftovers).
                finish(host, op, "kept", None, Some(failure));
                host.lock().blocked.remove(game);
            } else {
                block(host, op, game, failure);
            }
        }
    }
}

fn block(host: &Host, op: &OperationRow, game: &str, failure: Failure) {
    let value = serde_json::json!({ "error": failure });
    let _ = host.db().conn().execute(
        "UPDATE operations SET status = 'blocked', result = ?2 WHERE id = ?1",
        [op.id.as_str(), &value.to_string()],
    );
    host.lock().blocked.insert(game.to_string(), failure);
}

/// Countdowns live only in memory: a delete that hadn't started is dropped.
/// One that renamed its folder is finished.
fn resolve_delete(host: &Arc<Host>, op: &OperationRow, j: &Journal) {
    if j.phase != "delete" {
        finish(host, op, "cancelled", None, None);
        return;
    }
    let Some(id) = &j.checkpoint else { return finish(host, op, "failed", None, None) };
    let store = host.lock().store.clone();
    let record = db::checkpoint(host.db().conn(), id).ok().flatten();
    let still_there = record
        .as_ref()
        .is_some_and(|r| snap::presence(&store.join(&r.folder)) == Presence::Present && r.state != "deleting");
    if still_there {
        finish(host, op, "failed", None, Some(Failure::new(ErrorKind::ShuttingDown, "interrupted")));
        return;
    }
    if let Some(disposal) = &j.disposal
        && snap::remove_disposal(disposal).is_err()
    {
        let relative = disposal.strip_prefix(&store).unwrap_or(disposal).to_string_lossy().replace('\\', "/");
        let _ = db::set_checkpoint_folder(host.db().conn(), id, &relative);
        let _ = db::set_checkpoint_state(host.db().conn(), id, "deleting");
        finish(
            host,
            op,
            "failed",
            None,
            Some(Failure::new(ErrorKind::DeleteIncomplete, "leftovers are cleaned up later")),
        );
        return;
    }
    let _ = db::set_checkpoint_state(host.db().conn(), id, "deleted");
    finish(
        host,
        op,
        "succeeded",
        Some(OpResult { checkpoint: Some(id.clone()), count: Some(1), ..Default::default() }),
        None,
    );
}

/// Before the switch the old store stays in use: the partial copy goes.
/// After it, the old copies go.
fn resolve_move(host: &Arc<Host>, op: &OperationRow, j: &Journal) {
    match j.phase.as_str() {
        "move.switched" => {
            if let Some(old) = &j.old_store {
                crate::ops::remove_old_store(old);
            }
            finish(host, op, "succeeded", Some(OpResult::default()), None);
        }
        _ => {
            if let Some(new) = &j.new_store {
                let _ = fs::remove_dir_all(new);
            }
            finish(host, op, "failed", None, Some(Failure::new(ErrorKind::ShuttingDown, "interrupted")));
        }
    }
}
