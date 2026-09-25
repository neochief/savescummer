//! Operations: every Save, Load, Revert, Delete, Flush and store move goes
//! through the same handling and the same per-game lock. A request for a busy
//! game is rejected at once, never queued. "Accepted" is answered only after
//! the operation is durably recorded, and before each step that changes files
//! the journal records what is about to happen.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use savescummer_core::common::{Commonality, RecordedTarget, common_pairs, commonality};
use savescummer_core::history::RowKind;
use savescummer_core::labels;
use savescummer_core::{ErrorKind, Failure, Presence, SUFFIX_NEW, SUFFIX_OLD, Target};
use savescummer_ipc::{OpResult, OpStatus, Operation, Phase};
use savescummer_monitor::open_files::{self, FileId};
use savescummer_platform::sounds::Cue;
use savescummer_snapshots::{self as snap, Budget, CheckpointMeta, LoadPlan, Retry, TEMP_PREFIX, load};
use savescummer_storage::{self as db, CheckpointRow, HistoryRow, OperationRow};

use crate::checkpoints::{Verdict, judge, recompute_visibility};
use crate::host::{Host, Inner, PendingDelete, new_id, now};

/// What the journal records for an operation, before each step that changes
/// files. Recovery at startup reads it back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_folder: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Load/Revert: the checkpoint restored. Delete: the one deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverted_row: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<LoadPlan>,
    #[serde(default)]
    pub stage: u8,
    #[serde(default)]
    pub cloud_check: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposal: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_store: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_store: Option<PathBuf>,
}

impl Journal {
    /// Files and folders an unresolved interruption still needs.
    pub fn material(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        out.extend(self.temp.clone());
        out.extend(self.disposal.clone());
        if let Some(plan) = &self.plan {
            for file in &plan.files {
                out.push(file.new_path());
                out.push(file.old_path());
            }
        }
        out
    }
}

/// An operation request, after the game was found.
#[derive(Debug, Clone)]
pub enum Request {
    Save { label: Option<String> },
    Load { checkpoint: Option<String> },
    Revert { checkpoint: String },
    Delete { checkpoint: String },
    Flush,
}

impl Request {
    pub fn kind(&self) -> &'static str {
        match self {
            Request::Save { .. } => "save",
            Request::Load { .. } => "load",
            Request::Revert { .. } => "revert",
            Request::Delete { .. } => "delete",
            Request::Flush => "flush",
        }
    }
}

fn op(id: &str, request_id: Option<&str>, game: Option<&str>, kind: &str, status: OpStatus) -> Operation {
    Operation {
        id: id.to_string(),
        request_id: request_id.map(str::to_string),
        game: game.map(str::to_string),
        kind: kind.to_string(),
        status,
        error: None,
        result: None,
        created_at: now(),
        finished_at: None,
        remaining_ms: None,
        checkpoint: None,
    }
}

/// Reads an operation back from its record.
pub fn from_row(row: &OperationRow) -> Operation {
    let status = match row.status.as_str() {
        "succeeded" => OpStatus::Succeeded,
        "failed" | "blocked" | "kept" => OpStatus::Failed,
        "cancelled" => OpStatus::Cancelled,
        "accepted" => OpStatus::Accepted,
        _ => OpStatus::Running,
    };
    let result = row.result.as_ref();
    Operation {
        id: row.id.clone(),
        request_id: row.request_id.clone(),
        game: row.game_id.clone(),
        kind: row.kind.clone(),
        status,
        error: result.and_then(|r| r.get("error")).and_then(|e| serde_json::from_value(e.clone()).ok()),
        result: result.and_then(|r| r.get("result")).and_then(|e| serde_json::from_value(e.clone()).ok()),
        created_at: row.created_at.clone(),
        finished_at: row.finished_at.clone(),
        remaining_ms: None,
        checkpoint: None,
    }
}

/// Finds an operation by id: this run's first, then the records.
pub fn find(host: &Host, id: &str) -> Option<Operation> {
    if let Some(op) = host.lock().ops.get(id) {
        return Some(op.clone());
    }
    db::operation(host.db().conn(), id).ok().flatten().map(|row| from_row(&row))
}

/// Accepts or rejects an operation at once. Accepted means recorded.
pub fn submit(
    host: &Arc<Host>,
    request_id: &str,
    game: &str,
    request: Request,
    hotkey: bool,
) -> Result<Operation, Failure> {
    // Repeating a request id returns the same operation.
    if let Some(existing) = by_request(host, request_id) {
        return Ok(existing);
    }
    let result = submit_new(host, request_id, game, request, hotkey);
    if hotkey {
        match &result {
            Ok(op) if op.kind == "save" => cue(host, Cue::SaveStart),
            Ok(op) if op.kind == "load" => cue(host, Cue::LoadStart),
            Ok(_) => {}
            Err(f) if f.kind == ErrorKind::Busy => cue(host, Cue::Busy),
            Err(f) => {
                cue(host, Cue::Failed);
                notify_failure(host, f);
            }
        }
    }
    result
}

fn by_request(host: &Host, request_id: &str) -> Option<Operation> {
    if let Some(op) = host.lock().ops.values().find(|o| o.request_id.as_deref() == Some(request_id)) {
        return Some(op.clone());
    }
    db::operation_by_request(host.db().conn(), request_id).ok().flatten().map(|row| from_row(&row))
}

fn submit_new(
    host: &Arc<Host>,
    request_id: &str,
    game: &str,
    request: Request,
    hotkey: bool,
) -> Result<Operation, Failure> {
    let kind = request.kind();
    let op_id = new_id("op");
    let game_id;
    {
        let mut inner = host.lock();
        match inner.phase {
            Phase::Starting => return Err(Failure::new(ErrorKind::Starting, "the host is still starting")),
            Phase::ShuttingDown => return Err(Failure::new(ErrorKind::ShuttingDown, "the host is shutting down")),
            Phase::Ready => {}
        }
        game_id = host.find_game(&inner, game)?;
        let fail = |kind: ErrorKind, detail: &str| Failure::new(kind, detail).game(&game_id);
        if inner.store_moving {
            return Err(fail(ErrorKind::Busy, "the checkpoint store is being moved"));
        }
        if inner.blocked.contains_key(&game_id) {
            return Err(fail(ErrorKind::Blocked, "the game waits on an interrupted operation"));
        }
        if let Request::Delete { checkpoint } = &request {
            if inner.deletes.values().any(|d| d.op.checkpoint.as_deref() == Some(checkpoint)) {
                return Err(fail(ErrorKind::InvalidRequest, "this checkpoint is already being deleted"));
            }
        } else if inner.busy.contains_key(&game_id) {
            return Err(fail(ErrorKind::Busy, "another operation runs for this game"));
        }
        if !matches!(request, Request::Delete { .. }) {
            // Reserve the game while checking; a rejection releases it.
            inner.busy.insert(game_id.clone(), op(&op_id, Some(request_id), Some(&game_id), kind, OpStatus::Accepted));
        }
    }
    let release = |host: &Host| {
        let mut inner = host.lock();
        if inner.busy.get(&game_id).is_some_and(|o| o.id == op_id) {
            inner.busy.remove(&game_id);
        }
    };

    let prepared = match preflight(host, &game_id, &request) {
        Ok(p) => p,
        Err(f) => {
            release(host);
            return Err(f.with_game_if_missing(&game_id));
        }
    };

    let mut operation = op(&op_id, Some(request_id), Some(&game_id), kind, OpStatus::Accepted);
    if let Request::Delete { checkpoint } = &request {
        operation.status = OpStatus::CountingDown;
        operation.checkpoint = Some(checkpoint.clone());
    }
    let row = OperationRow {
        id: op_id.clone(),
        request_id: Some(request_id.to_string()),
        game_id: Some(game_id.clone()),
        kind: kind.to_string(),
        status: "accepted".into(),
        journal: None,
        result: None,
        created_at: operation.created_at.clone(),
        finished_at: None,
    };
    let inserted = host.db().write(|c| db::insert_operation(c, &row));
    if let Err(e) = inserted {
        release(host);
        return Err(Failure::new(ErrorKind::NotRecorded, e.to_string()).game(&game_id));
    }
    {
        let mut inner = host.lock();
        inner.ops.insert(op_id.clone(), operation.clone());
        if hotkey {
            inner.hotkey_ops.insert(op_id.clone());
        }
        inner.notices.remove(&game_id);
        match &request {
            Request::Delete { .. } => {
                let deadline = Instant::now() + Duration::from_millis(host.opts.delete_countdown_ms);
                inner.deletes.insert(op_id.clone(), PendingDelete { op: operation.clone(), deadline });
            }
            _ => {
                inner.busy.insert(game_id.clone(), operation.clone());
            }
        }
        host.publish(&mut inner);
    }
    let done = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU64::new(0));
    if matches!(request, Request::Save { .. } | Request::Load { .. } | Request::Revert { .. }) {
        watch_for_stall(host.clone(), op_id.clone(), game_id.clone(), done.clone(), progress.clone());
    }
    let worker_host = host.clone();
    let worker_game = game_id.clone();
    let worker_op = op_id.clone();
    std::thread::spawn(move || {
        snap::count_progress(progress);
        let outcome = match (request, prepared) {
            (Request::Save { label }, _) => run_save(&worker_host, &worker_op, &worker_game, label),
            (Request::Load { .. }, Prepared::Restore(r)) | (Request::Revert { .. }, Prepared::Restore(r)) => {
                run_restore(&worker_host, &worker_op, &worker_game, *r)
            }
            (Request::Delete { checkpoint }, _) => run_delete(&worker_host, &worker_op, &worker_game, &checkpoint),
            (Request::Flush, _) => run_flush(&worker_host, &worker_op, &worker_game),
            _ => Err(Failure::new(ErrorKind::InvalidRequest, "unexpected request")),
        };
        finish(&worker_host, &worker_op, &worker_game, outcome);
        done.store(true, Ordering::SeqCst);
    });
    Ok(operation)
}

/// The safety net for a read that blocks (PLAN-MACOS.md, PRIVACY
/// PERMISSIONS: a guarded location the table misses): after `--stall-secs`
/// without any progress of its own file work, the operation is reported
/// failed. Its thread
/// is left to finish and keeps the game's lock until it does, so no second
/// Load runs over a half-finished one; its real outcome is recorded then.
fn watch_for_stall(host: Arc<Host>, op_id: String, game_id: String, done: Arc<AtomicBool>, progress: Arc<AtomicU64>) {
    let stall = Duration::from_secs(host.opts.stall_secs);
    std::thread::spawn(move || {
        let (mut seen, mut since) = (progress.load(Ordering::Relaxed), Instant::now());
        while !done.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
            let now = progress.load(Ordering::Relaxed);
            if now != seen {
                (seen, since) = (now, Instant::now());
            } else if since.elapsed() >= stall {
                report_stalled(&host, &op_id, &game_id);
                return;
            }
        }
    });
}

fn report_stalled(host: &Arc<Host>, op_id: &str, game_id: &str) {
    let failure =
        Failure::new(ErrorKind::Stalled, "file work stopped responding; macOS may be waiting on a permission prompt")
            .game(game_id);
    crate::trace(&format!("{op_id} for {game_id} stalled; the game stays locked until it ends"));
    let hotkey = {
        let mut inner = host.lock();
        let Some(operation) = inner.ops.get_mut(op_id).filter(|o| !o.status.is_final()) else { return };
        operation.status = OpStatus::Failed;
        operation.error = Some(failure.clone());
        let operation = operation.clone();
        inner.last_results.insert(game_id.to_string(), operation);
        let hotkey = inner.hotkey_ops.remove(op_id);
        host.publish(&mut inner);
        hotkey
    };
    if hotkey {
        cue(host, Cue::Failed);
        notify_failure(host, &failure);
    }
}

enum Prepared {
    Nothing,
    Restore(Box<Restore>),
}

/// Everything a Load or Revert checked before it was accepted.
#[derive(Debug, Clone)]
struct Restore {
    kind: &'static str,
    checkpoint: CheckpointRow,
    reverted_row: Option<String>,
}

/// Every rejection happens here, before anything is created.
fn preflight(host: &Arc<Host>, game_id: &str, request: &Request) -> Result<Prepared, Failure> {
    let mut inner = host.lock();
    crate::library::derive_one(host, &mut inner, game_id);
    // Every location it touches must be allowed before the first read: a
    // prompt during a game may go unseen while the read waits on it.
    if let Some(category) = host.privacy.needed(&inner.store) {
        return Err(Failure::new(ErrorKind::AccessNeeded, category.as_str()).path(&inner.store));
    }
    if !inner.store_available {
        return Err(
            Failure::new(ErrorKind::StoreUnavailable, "the checkpoint store can't be reached").path(&inner.store)
        );
    }
    let derived = inner.derived.get(game_id).cloned().unwrap_or_default();
    match request {
        Request::Delete { checkpoint } => {
            let record = db::checkpoint(host.db().conn(), checkpoint)
                .ok()
                .flatten()
                .filter(|c| c.game_id == game_id && c.exists())
                .ok_or_else(|| Failure::new(ErrorKind::CheckpointChanged, "the checkpoint no longer exists"))?;
            let _ = record;
            Ok(Prepared::Nothing)
        }
        Request::Flush => Ok(Prepared::Nothing),
        Request::Save { .. } => {
            let targets = derived.active.clone()?;
            if let Some(t) = targets.iter().find(|t| t.presence == Presence::Unknown) {
                return Err(Failure::new(ErrorKind::TargetUnavailable, "the save location can't be read").path(&t.root));
            }
            if !derived.has_data {
                return Err(Failure::new(ErrorKind::NoGameData, "no save location matches anything yet")
                    .paths(targets.iter().map(|t| t.root.clone())));
            }
            Ok(Prepared::Nothing)
        }
        Request::Load { .. } | Request::Revert { .. } => {
            let targets = derived.active.clone()?;
            let ci = host.env.case_insensitive();
            let current = crate::host::current_pairs(&inner, game_id);
            let (kind, record, reverted_row) = match request {
                Request::Load { checkpoint: None } => {
                    let all = db::existing_checkpoints(host.db().conn(), game_id).unwrap_or_default();
                    let latest = crate::host::latest_usable(&all, &current, ci)
                        .cloned()
                        .ok_or_else(|| Failure::new(ErrorKind::NoSaves, "no saved checkpoint to load"))?;
                    ("load", latest, None)
                }
                Request::Load { checkpoint: Some(id) } => {
                    let record = checkpoint_of(host, game_id, id)?;
                    if record.kind != "saved" {
                        return Err(Failure::new(
                            ErrorKind::InvalidRequest,
                            "Load restores saved checkpoints; use Revert",
                        ));
                    }
                    ("load", record, None)
                }
                Request::Revert { checkpoint } => {
                    let record = checkpoint_of(host, game_id, checkpoint)?;
                    if record.kind != "recovery" {
                        return Err(Failure::new(ErrorKind::InvalidRequest, "Revert restores recovery checkpoints"));
                    }
                    let row = history_owner(host, &record.id);
                    ("revert", record, row)
                }
                _ => unreachable!(),
            };
            drop(inner);
            if record.state != "ok" {
                return Err(Failure::new(ErrorKind::CheckpointUnreadable, "the checkpoint can't be read now")
                    .path(&record.folder));
            }
            if commonality(&record.targets, &current, ci) == Commonality::None {
                return Err(Failure::new(
                    ErrorKind::DifferentSaveSet,
                    "the checkpoint was made for other save locations",
                )
                .paths(record.targets.iter().map(|t| t.root.clone())));
            }
            let store = host.lock().store.clone();
            let path = store.join(&record.folder);
            match judge(&record, &path) {
                Verdict::Same => {}
                Verdict::Unknown => {
                    return Err(
                        Failure::new(ErrorKind::CheckpointUnreadable, "the checkpoint can't be read").path(&path)
                    );
                }
                _ => {
                    return Err(Failure::new(
                        ErrorKind::CheckpointChanged,
                        "the checkpoint changed outside SaveScummer",
                    )
                    .path(&path));
                }
            }
            // Dry run: every target it touches must be readable, and a root
            // that held data must exist.
            let pairs = restore_pairs(&record.targets, &targets, ci);
            load::plan_load(&path, &pairs, ci)?;
            Ok(Prepared::Restore(Box::new(Restore { kind, checkpoint: record, reverted_row })))
        }
    }
}

fn checkpoint_of(host: &Host, game_id: &str, id: &str) -> Result<CheckpointRow, Failure> {
    db::checkpoint(host.db().conn(), id)
        .ok()
        .flatten()
        .filter(|c| c.game_id == game_id && c.exists())
        .ok_or_else(|| Failure::new(ErrorKind::CheckpointChanged, "the checkpoint no longer exists"))
}

/// The row whose recovery checkpoint this is.
fn history_owner(host: &Host, recovery: &str) -> Option<String> {
    let storage = host.db();
    let mut stmt = storage.conn().prepare("SELECT id FROM history WHERE recovery_id = ?1 LIMIT 1").ok()?;
    stmt.query_row([recovery], |r| r.get(0)).ok()
}

fn restore_pairs(recorded: &[RecordedTarget], current: &[Target], ci: bool) -> Vec<(RecordedTarget, Target)> {
    let current_keys: Vec<_> = current.iter().map(|t| (t.root.clone(), t.filter.clone())).collect();
    common_pairs(recorded, &current_keys, ci)
        .into_iter()
        .map(|(i, j)| (recorded[i].clone(), current[j].clone()))
        .collect()
}

fn journal(host: &Host, op_id: &str, journal: &Journal) -> Result<(), Failure> {
    let value = serde_json::to_value(journal).expect("journal serializes");
    host.db()
        .write(|c| db::set_journal(c, op_id, "running", &value))
        .map_err(|e| Failure::new(ErrorKind::NotRecorded, e.to_string()))
}

fn set_status(host: &Host, op_id: &str, status: OpStatus) {
    let mut inner = host.lock();
    if let Some(op) = inner.ops.get_mut(op_id) {
        op.status = status;
    }
    let updated = inner.ops.get(op_id).cloned();
    if let Some(updated) = updated {
        if let Some(game) = &updated.game
            && inner.busy.get(game).is_some_and(|b| b.id == op_id)
        {
            inner.busy.insert(game.clone(), updated.clone());
        }
        if let Some(pending) = inner.deletes.get_mut(op_id) {
            pending.op.status = status;
        }
    }
    host.publish(&mut inner);
}

fn local_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H.%M.%S").to_string()
}

/// Copies the game's save set into a new checkpoint folder: into a
/// temporary folder first, published under its name only when complete.
fn make_checkpoint(
    host: &Host,
    op_id: &str,
    game_id: &str,
    kind: &str,
    targets: &[Target],
    budget: &Budget,
    journal_base: &mut Journal,
) -> Result<CheckpointRow, Failure> {
    let (game_name, folder) = {
        let inner = host.lock();
        let game = inner.game(game_id)?;
        (game.name.clone(), inner.store.join(game.store_folder()))
    };
    fs::create_dir_all(&folder).map_err(|e| Failure::new(ErrorKind::StoreUnavailable, e.to_string()).path(&folder))?;
    let temp = folder.join(format!("{TEMP_PREFIX}{op_id}-{kind}"));
    journal_base.temp = Some(temp.clone());
    journal_base.game_folder = Some(folder.clone());
    journal(host, op_id, journal_base)?;
    let created_at = now();
    let mut meta = CheckpointMeta {
        format: 1,
        game_id: game_id.to_string(),
        game_name,
        kind: kind.to_string(),
        created_at: created_at.clone(),
        operation: Some(op_id.to_string()),
        targets: Vec::new(),
    };
    let hook_name = format!("{kind}.copy");
    let hook = |_: &str, n: usize| host.crash_point(&hook_name, n);
    let copied = snap::copy_save_set(targets, &temp, &mut meta, host.env.case_insensitive(), budget, &hook);
    if let Err(failure) = copied {
        let _ = snap::remove_disposal(&temp);
        return Err(failure);
    }
    host.crash_point(&format!("{kind}.copied"), 1);
    let final_path = snap::publish(&temp, &folder, &format!("{} {kind}", local_stamp()))?;
    host.crash_point(&format!("{kind}.published"), 1);
    let signature =
        snap::signature(&final_path).map_err(|e| Failure::new(ErrorKind::Io, e.to_string()).path(&final_path))?;
    let store = host.lock().store.clone();
    let relative = final_path.strip_prefix(&store).unwrap_or(&final_path).to_string_lossy().replace('\\', "/");
    Ok(CheckpointRow {
        seq: 0,
        id: new_id("cp"),
        game_id: game_id.to_string(),
        kind: kind.to_string(),
        folder: relative,
        created_at,
        label: None,
        targets: meta.targets,
        signature: signature.hash,
        identity: signature.identity,
        size: signature.size,
        state: "ok".into(),
    })
}

fn current_targets(host: &Host, game_id: &str) -> Result<Vec<Target>, Failure> {
    let mut inner = host.lock();
    crate::library::derive_one(host, &mut inner, game_id);
    let targets = inner
        .derived
        .get(game_id)
        .map(|d| d.active.clone())
        .unwrap_or_else(|| Err(Failure::new(ErrorKind::NoSaveLocation, "no save location")))?;
    if let Some(t) = targets.iter().find(|t| t.presence == Presence::Unknown) {
        return Err(Failure::new(ErrorKind::TargetUnavailable, "the save location can't be read").path(&t.root));
    }
    Ok(targets)
}

fn session_of(host: &Host, game_id: &str) -> Option<String> {
    host.lock().sessions.get(game_id).cloned()
}

fn run_save(host: &Arc<Host>, op_id: &str, game_id: &str, label: Option<String>) -> Result<OpResult, Failure> {
    set_status(host, op_id, OpStatus::Running);
    let targets = current_targets(host, game_id)?;
    let label = label.as_deref().and_then(labels::normalize);
    let mut j = Journal { phase: "save".into(), label: label.clone(), ..Default::default() };
    let retry = Retry::new();
    let mut checkpoint = make_checkpoint(host, op_id, game_id, "saved", &targets, &retry.forward, &mut j)?;
    checkpoint.label = label;
    let row = HistoryRow {
        seq: 0,
        id: new_id("row"),
        game_id: game_id.to_string(),
        kind: RowKind::Saved,
        at: checkpoint.created_at.clone(),
        session: session_of(host, game_id),
        checkpoint_id: Some(checkpoint.id.clone()),
        recovery_id: None,
        reverted_row: None,
        removed: 0,
        cloud_check: false,
        cloud_replaced: false,
        visible: true,
    };
    let result = OpResult { checkpoint: Some(checkpoint.id.clone()), ..Default::default() };
    commit(host, op_id, game_id, Some(&checkpoint), Some(&row), &result)?;
    Ok(result)
}

/// Records a finished operation's files and history in one transaction.
fn commit(
    host: &Host,
    op_id: &str,
    game_id: &str,
    checkpoint: Option<&CheckpointRow>,
    row: Option<&HistoryRow>,
    result: &OpResult,
) -> Result<(), Failure> {
    let at = now();
    let value = serde_json::json!({ "result": result });
    host.db()
        .write(|c| {
            if let Some(checkpoint) = checkpoint {
                db::insert_checkpoint(c, checkpoint)?;
            }
            if let Some(row) = row {
                db::insert_row(c, row)?;
                // A visible row makes its session's markers visible.
                if let Some(session) = &row.session {
                    c.execute(
                        "UPDATE history SET visible = 1 WHERE game_id = ?1 AND session = ?2 AND kind IN ('game_started', 'game_closed')",
                        rusqlite_params(game_id, session),
                    )?;
                }
            }
            db::finish_operation(c, op_id, "succeeded", &value, &at)
        })
        .map_err(|e| Failure::new(ErrorKind::NotRecorded, e.to_string()))?;
    let mut inner = host.lock();
    host.bump_history(&mut inner, game_id);
    Ok(())
}

fn rusqlite_params<'a>(a: &'a str, b: &'a str) -> [&'a str; 2] {
    [a, b]
}

fn run_restore(host: &Arc<Host>, op_id: &str, game_id: &str, prepared: Restore) -> Result<OpResult, Failure> {
    set_status(host, op_id, OpStatus::Running);
    let ci = host.env.case_insensitive();
    let targets = current_targets(host, game_id)?;
    let kind = prepared.kind;
    let source = prepared.checkpoint;
    let store = host.lock().store.clone();
    let source_path = store.join(&source.folder);
    if judge(&source, &source_path) != Verdict::Same {
        return Err(
            Failure::new(ErrorKind::CheckpointChanged, "the checkpoint changed outside SaveScummer").path(&source_path)
        );
    }
    // Steam syncs at launch: check at the next start only when the Load ran
    // with the game closed (a running game rewrites its own saves anyway).
    let is_steam = {
        let inner = host.lock();
        inner.games.get(game_id).is_some_and(|g| g.is_steam()) && !inner.stack.contains(game_id)
    };

    // 1. Keep the current state as a recovery checkpoint, with the same
    //    filters, so every file the Load deletes is in it.
    let mut j = Journal {
        phase: format!("{kind}.recovery"),
        checkpoint: Some(source.id.clone()),
        reverted_row: prepared.reverted_row.clone(),
        session: session_of(host, game_id),
        cloud_check: is_steam,
        ..Default::default()
    };
    // One waiting budget for the whole forward operation, one for undoing it.
    let retry = Retry::new();
    let recovery = make_checkpoint(host, op_id, game_id, "recovery", &targets, &retry.forward, &mut j)?;
    host.db()
        .write(|c| db::insert_checkpoint(c, &recovery).map(|_| ()))
        .map_err(|e| Failure::new(ErrorKind::NotRecorded, e.to_string()))?;
    host.crash_point("load.recovery", 1);

    // 2. Plan, and record the plan before any live file changes.
    let pairs = restore_pairs(&source.targets, &targets, ci);
    let discard_recovery = |host: &Host| {
        let path = store.join(&recovery.folder);
        let _ = snap::dispose(&path, &new_id("x"));
        let _ = db::set_checkpoint_state(host.db().conn(), &recovery.id, "deleted");
    };
    let mut plan = match load::plan_load(&source_path, &pairs, ci) {
        Ok(plan) => plan,
        Err(f) => {
            discard_recovery(host);
            return Err(f);
        }
    };
    j.phase = format!("{kind}.apply");
    j.recovery = Some(recovery.id.clone());
    j.temp = None;
    j.plan = Some(plan.clone());
    j.stage = 0;
    journal(host, op_id, &j)?;

    let hook = |name: &str, n: usize| host.crash_point(name, n);
    let set_aside = |p: &mut LoadPlan| {
        if let Err(failure) = wait_for_open_files(host, game_id, p, &retry.forward) {
            let undone = load::undo_copy_in(p, &retry.rollback).is_ok();
            return Err(load::StageError { failure, undone });
        }
        load::stage_set_aside(p, &retry, &hook)
    };
    let stages: [Stage<'_>; 3] =
        [&|p| load::stage_copy_in(p, &retry, &hook), &set_aside, &|p| load::stage_swap_in(p, &retry, &hook)];
    for (i, stage) in stages.iter().enumerate() {
        if let Err(e) = stage(&mut plan) {
            if e.undone {
                discard_recovery(host);
                return Err(e.failure);
            }
            // The undo failed: keep everything and block the game.
            j.stage = i as u8 + 1;
            j.plan = Some(plan.clone());
            let _ = journal(host, op_id, &j);
            return Err(Failure { kind: ErrorKind::RollbackFailed, ..e.failure }.path(store.join(&recovery.folder)));
        }
        j.stage = i as u8 + 1;
        j.plan = Some(plan.clone());
        journal(host, op_id, &j)?;
    }
    host.crash_point("load.applied", 1);

    // 3. Record the Loaded/Reverted row, then clean up.
    let removed = plan.removed_files() as u32;
    let row = restore_row(
        game_id,
        kind,
        &source.id,
        &recovery.id,
        prepared.reverted_row.clone(),
        removed,
        j.session.clone(),
        is_steam,
    );
    let result = OpResult {
        checkpoint: Some(source.id.clone()),
        recovery: Some(recovery.id.clone()),
        removed_files: Some(removed),
        ..Default::default()
    };
    commit(host, op_id, game_id, None, Some(&row), &result)?;
    load::stage_clean_up(&plan, &hook);
    Ok(result)
}

/// Before Load stage 2 (PLAN-HOST, LOAD, "Interference from the running
/// game"): the files stage 2 would rename, checked against the game's
/// processes as the monitor sees them.
fn wait_for_open_files(host: &Host, game_id: &str, plan: &LoadPlan, budget: &Budget) -> Result<(), Failure> {
    if !open_files::SUPPORTED {
        return Ok(());
    }
    let files: Vec<(&Path, FileId)> = plan
        .files
        .iter()
        .filter(|f| f.original.is_some())
        .filter_map(|f| Some((f.live.as_path(), FileId::of(&f.live)?)))
        .collect();
    let pids = || host.lock().processes.get(game_id).cloned().unwrap_or_default();
    wait_until_closed(game_id, &files, pids, budget)
}

/// While `pids` hold any of `files` open, waits within `budget`; refuses if
/// one is still open when it runs out. Where nothing is found open but some
/// process couldn't be inspected, goes on as before and logs it.
fn wait_until_closed(
    game_id: &str,
    files: &[(&Path, FileId)],
    pids: impl Fn() -> Vec<u32>,
    budget: &Budget,
) -> Result<(), Failure> {
    let ids: Vec<FileId> = files.iter().map(|(_, id)| *id).collect();
    loop {
        let pids = pids();
        if pids.is_empty() || ids.is_empty() {
            return Ok(());
        }
        let found = open_files::inspect(&pids, &ids);
        if found.open.is_empty() {
            if !found.incomplete.is_empty() {
                let why: Vec<String> = found.incomplete.iter().map(|(pid, why)| format!("pid {pid}: {why}")).collect();
                crate::trace(&format!(
                    "open-file check for {game_id} couldn't inspect every process, loading as before: {}",
                    why.join("; ")
                ));
            }
            return Ok(());
        }
        if budget.wait() {
            continue;
        }
        let held: Vec<String> =
            found.open.iter().map(|&(pid, i)| format!("pid {pid} holds {}", files[i].0.display())).collect();
        let detail = format!("still open after waiting: {}", held.join("; "));
        crate::trace(&format!("load of {game_id} refused before changing any file, {detail}"));
        let open: std::collections::BTreeSet<usize> = found.open.iter().map(|&(_, i)| i).collect();
        return Err(Failure::new(ErrorKind::HeldOpen, detail).paths(open.into_iter().map(|i| files[i].0)));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn restore_row(
    game_id: &str,
    kind: &str,
    source: &str,
    recovery: &str,
    reverted_row: Option<String>,
    removed: u32,
    session: Option<String>,
    cloud_check: bool,
) -> HistoryRow {
    HistoryRow {
        seq: 0,
        id: new_id("row"),
        game_id: game_id.to_string(),
        kind: if kind == "revert" { RowKind::Reverted } else { RowKind::Loaded },
        at: now(),
        session,
        checkpoint_id: Some(source.to_string()),
        recovery_id: Some(recovery.to_string()),
        reverted_row,
        removed,
        cloud_check,
        cloud_replaced: false,
        visible: true,
    }
}

fn run_delete(host: &Arc<Host>, op_id: &str, game_id: &str, checkpoint: &str) -> Result<OpResult, Failure> {
    // The countdown lives only in memory.
    loop {
        let (cancelled, deadline, shutting_down) = {
            let inner = host.lock();
            match inner.deletes.get(op_id) {
                None => (true, Instant::now(), false),
                Some(d) => (d.op.status == OpStatus::Cancelled, d.deadline, inner.phase == Phase::ShuttingDown),
            }
        };
        if cancelled {
            return Err(Failure::new(ErrorKind::InvalidRequest, "cancelled"));
        }
        if Instant::now() >= deadline || shutting_down {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Then it waits for the game's turn and runs through the same lock.
    set_status(host, op_id, OpStatus::Waiting);
    loop {
        let mut inner = host.lock();
        if !inner.deletes.contains_key(op_id) {
            return Err(Failure::new(ErrorKind::InvalidRequest, "cancelled"));
        }
        if let Some(f) = inner.blocked.get(game_id) {
            return Err(f.clone());
        }
        if !inner.busy.contains_key(game_id) && !inner.store_moving {
            let mut operation = inner.ops.get(op_id).cloned().expect("known operation");
            operation.status = OpStatus::Running;
            inner.busy.insert(game_id.to_string(), operation);
            break;
        }
        drop(inner);
        std::thread::sleep(Duration::from_millis(20));
    }
    set_status(host, op_id, OpStatus::Running);
    let record = db::checkpoint(host.db().conn(), checkpoint)
        .ok()
        .flatten()
        .filter(|c| c.exists())
        .ok_or_else(|| Failure::new(ErrorKind::CheckpointChanged, "the checkpoint no longer exists"))?;
    let store = host.lock().store.clone();
    let path = store.join(&record.folder);
    match judge(&record, &path) {
        Verdict::Same => {}
        Verdict::Unknown => {
            return Err(Failure::new(ErrorKind::CheckpointUnreadable, "the checkpoint can't be read").path(&path));
        }
        _ => {
            return Err(Failure::new(ErrorKind::DeleteMismatch, "the checkpoint no longer matches what was recorded")
                .path(&path));
        }
    }
    let token = op_id.to_string();
    let disposal = path.parent().unwrap_or(&store).join(format!("{}{token}", snap::DISPOSAL_PREFIX));
    let j = Journal {
        phase: "delete".into(),
        checkpoint: Some(record.id.clone()),
        disposal: Some(disposal.clone()),
        ..Default::default()
    };
    journal(host, op_id, &j)?;
    let outcome = match snap::dispose(&path, &token) {
        Ok(()) => {
            let _ = db::set_checkpoint_state(host.db().conn(), &record.id, "deleted");
            Ok(OpResult { checkpoint: Some(record.id.clone()), count: Some(1), ..Default::default() })
        }
        Err(snap::DisposeError::Rename(f)) => Err(f),
        Err(snap::DisposeError::Remove(disposal, f)) => {
            let relative = disposal.strip_prefix(&store).unwrap_or(&disposal).to_string_lossy().replace('\\', "/");
            let _ = host.db().write(|c| {
                db::set_checkpoint_folder(c, &record.id, &relative)?;
                db::set_checkpoint_state(c, &record.id, "deleting")
            });
            Err(f)
        }
    };
    let mut inner = host.lock();
    recompute_visibility(host, &mut inner, game_id);
    outcome
}

/// Cancels a Delete countdown. An accepted cancel guarantees nothing is
/// deleted; a cancel that arrives too late gets the real state back.
pub fn cancel_delete(host: &Host, op_id: &str) -> Result<Operation, Failure> {
    let mut inner = host.lock();
    let Some(pending) = inner.deletes.get_mut(op_id) else {
        drop(inner);
        return find(host, op_id).ok_or_else(|| Failure::new(ErrorKind::NotFound, "no such operation"));
    };
    if pending.op.status != OpStatus::CountingDown {
        return Ok(pending.op.clone());
    }
    pending.op.status = OpStatus::Cancelled;
    let op = pending.op.clone();
    inner.deletes.remove(op_id);
    if let Some(o) = inner.ops.get_mut(op_id) {
        o.status = OpStatus::Cancelled;
        o.finished_at = Some(now());
    }
    let _ = db::finish_operation(host.db().conn(), op_id, "cancelled", &serde_json::json!({}), &now());
    host.publish(&mut inner);
    Ok(Operation { status: OpStatus::Cancelled, ..op })
}

fn run_flush(host: &Arc<Host>, op_id: &str, game_id: &str) -> Result<OpResult, Failure> {
    set_status(host, op_id, OpStatus::Running);
    // Pending deletes of this game are dropped quietly.
    {
        let mut inner = host.lock();
        let dropped: Vec<String> = inner
            .deletes
            .iter()
            .filter(|(_, d)| d.op.game.as_deref() == Some(game_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in dropped {
            inner.deletes.remove(&id);
            if let Some(o) = inner.ops.get_mut(&id) {
                o.status = OpStatus::Cancelled;
            }
        }
    }
    let store = host.lock().store.clone();
    let folder = host.game_store_dir(&host.lock(), game_id).expect("known game");
    // Found again from scratch: the preview is not a contract.
    let records: Vec<CheckpointRow> = db::all_live_checkpoints(host.db().conn())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.game_id == game_id)
        .collect();
    let mut deleted = Vec::new();
    let mut failures = Vec::new();
    for record in &records {
        let path = store.join(&record.folder);
        let result = if record.state == "deleting" || snap::presence(&path) == Presence::Missing {
            snap::remove_disposal(&path)
        } else {
            snap::dispose(&path, &new_id("f")).map_err(|e| match e {
                snap::DisposeError::Rename(f) | snap::DisposeError::Remove(_, f) => f,
            })
        };
        match result {
            Ok(()) => deleted.push(record.id.clone()),
            Err(f) => failures.push(f),
        }
    }
    // Leftover temporary folders.
    if let Ok(entries) = fs::read_dir(&folder) {
        for entry in entries.flatten() {
            if snap::is_reserved_folder(&entry.file_name().to_string_lossy())
                && let Err(f) = snap::remove_disposal(&entry.path())
            {
                failures.push(f);
            }
        }
    }
    let _ = fs::remove_dir(&folder);
    let everything = failures.is_empty();
    host.db()
        .write(|c| {
            db::forget_checkpoints(c, &deleted)?;
            if everything {
                db::forget_history(c, game_id)?;
                db::clear_notices(c, game_id)?;
            }
            Ok(())
        })
        .map_err(|e| Failure::new(ErrorKind::NotRecorded, e.to_string()))?;
    let mut inner = host.lock();
    if !everything {
        recompute_visibility(host, &mut inner, game_id);
    }
    inner.notices.remove(game_id);
    inner.caches.entry(game_id.to_string()).or_default().leftover_size = 0;
    host.bump_history(&mut inner, game_id);
    host.bump_labels(&mut inner, game_id);
    drop(inner);
    Ok(OpResult { count: Some(deleted.len() as u32), failures, ..Default::default() })
}

/// Ends an operation: records its outcome, releases the game, publishes,
/// and gives a hotkey's completion cue.
pub fn finish(host: &Arc<Host>, op_id: &str, game_id: &str, outcome: Result<OpResult, Failure>) {
    let at = now();
    let (status, error, result) = match outcome {
        Ok(result) => (OpStatus::Succeeded, None, Some(result)),
        Err(f) if f.detail == "cancelled" && f.kind == ErrorKind::InvalidRequest => (OpStatus::Cancelled, None, None),
        Err(f) => (OpStatus::Failed, Some(f.with_game_if_missing(game_id)), None),
    };
    let blocked = error.as_ref().is_some_and(|e| matches!(e.kind, ErrorKind::RollbackFailed | ErrorKind::NotRecorded));
    let db_status = match status {
        OpStatus::Succeeded => "succeeded",
        OpStatus::Cancelled => "cancelled",
        _ if blocked => "blocked",
        _ => "failed",
    };
    let value = serde_json::json!({ "result": result, "error": error });
    // Always record the outcome: Save, Load and Revert also mark success in
    // their own transaction, but Delete and Flush rely on this write.
    let _ = db::finish_operation(host.db().conn(), op_id, db_status, &value, &at);
    let (hotkey, kind, hidden) = {
        let mut inner = host.lock();
        let mut operation =
            inner.ops.get(op_id).cloned().unwrap_or_else(|| op(op_id, None, Some(game_id), "unknown", status));
        operation.status = status;
        operation.error = error.clone();
        operation.result = result;
        operation.finished_at = Some(at);
        operation.remaining_ms = None;
        inner.ops.insert(op_id.to_string(), operation.clone());
        if inner.busy.get(game_id).is_some_and(|b| b.id == op_id) {
            inner.busy.remove(game_id);
        }
        inner.deletes.remove(op_id);
        if blocked && let Some(e) = &error {
            inner.blocked.insert(game_id.to_string(), e.clone());
        }
        if operation.kind == "save"
            && let Some(e) = &error
            && e.kind == ErrorKind::ShuttingDown
        {
            inner.notices.insert(game_id.to_string(), "save_interrupted".into());
        }
        inner.last_results.insert(game_id.to_string(), operation.clone());
        crate::library::refresh_presence(host, &mut inner, &[game_id.to_string()]);
        host.refresh_cache(&mut inner, game_id);
        host.publish(&mut inner);
        let hidden = !inner.ui.visible;
        (inner.hotkey_ops.remove(op_id), operation.kind.clone(), hidden)
    };
    if hotkey {
        match (status, kind.as_str()) {
            (OpStatus::Succeeded, "save") => cue(host, Cue::SaveDone),
            (OpStatus::Succeeded, "load") => cue(host, Cue::LoadDone),
            (OpStatus::Succeeded, _) => {}
            _ => cue(host, Cue::Failed),
        }
    }
    if let Some(e) = &error
        && hidden
        && hotkey
    {
        notify_failure(host, e);
    }
    if let Some(e) = &error {
        crate::privacy::after_failure(host, e);
    }
}

pub fn cue(host: &Host, cue: Cue) {
    if host.lock().play_sounds
        && let Some(player) = &host.sounds
    {
        player.play(cue);
    }
}

fn notify_failure(host: &Host, failure: &Failure) {
    if host.lock().ui.visible {
        return;
    }
    if let Some(integration) = host.integration.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let game = failure.game.clone().unwrap_or_default();
        integration.notify("SaveScummer", &format!("{game}: {}", failure.kind.as_str().replace('_', " ")));
    }
}

/// Moves the checkpoint store: copy everything, verify it, switch, then
/// delete the old copies. Until the switch the old store stays in use, so an
/// interrupted move changes nothing.
pub fn move_store(host: &Arc<Host>, request_id: &str, target: &str) -> Result<Operation, Failure> {
    if let Some(existing) = by_request(host, request_id) {
        return Ok(existing);
    }
    let new_store = PathBuf::from(target.trim());
    if !new_store.is_absolute() {
        return Err(Failure::new(ErrorKind::InvalidConfig, "use a full path").path(&new_store));
    }
    // A user action: a location macOS guards is asked for now.
    crate::privacy::ask_for(host, &new_store)?;
    let op_id = new_id("op");
    let old_store;
    {
        let mut inner = host.lock();
        if inner.phase != Phase::Ready {
            return Err(Failure::new(ErrorKind::Starting, "the host isn't ready"));
        }
        if !inner.busy.is_empty() || inner.store_moving || !inner.deletes.is_empty() {
            return Err(Failure::new(ErrorKind::Busy, "operations are running"));
        }
        if !inner.store_available {
            return Err(
                Failure::new(ErrorKind::StoreUnavailable, "the checkpoint store can't be reached").path(&inner.store)
            );
        }
        old_store = inner.store.clone();
        let ci = host.env.case_insensitive();
        if savescummer_core::common::is_within(&new_store, &old_store, ci)
            || savescummer_core::common::is_within(&old_store, &new_store, ci)
        {
            return Err(Failure::new(
                ErrorKind::InvalidConfig,
                "the new location can't be inside the old one, or contain it",
            )
            .path(&new_store));
        }
        if fs::read_dir(&new_store).map(|mut d| d.next().is_some()).unwrap_or(false) {
            return Err(Failure::new(ErrorKind::InvalidConfig, "the new location must be empty").path(&new_store));
        }
        inner.store_moving = true;
        let operation = op(&op_id, Some(request_id), None, "move_store", OpStatus::Running);
        inner.ops.insert(op_id.clone(), operation);
        host.publish(&mut inner);
    }
    let row = OperationRow {
        id: op_id.clone(),
        request_id: Some(request_id.to_string()),
        game_id: None,
        kind: "move_store".into(),
        status: "running".into(),
        journal: Some(
            serde_json::to_value(Journal {
                phase: "move".into(),
                old_store: Some(old_store.clone()),
                new_store: Some(new_store.clone()),
                ..Default::default()
            })
            .expect("journal serializes"),
        ),
        result: None,
        created_at: now(),
        finished_at: None,
    };
    let inserted = host.db().write(|c| db::insert_operation(c, &row));
    if let Err(e) = inserted {
        host.lock().store_moving = false;
        return Err(Failure::new(ErrorKind::NotRecorded, e.to_string()));
    }
    let operation = host.lock().ops[&op_id].clone();
    let worker = host.clone();
    std::thread::spawn(move || {
        let outcome = run_move(&worker, &op_id, &old_store, &new_store);
        let at = now();
        let (status, error, result) = match outcome {
            Ok(r) => ("succeeded", None, Some(r)),
            Err(f) => ("failed", Some(f), None),
        };
        let _ = db::finish_operation(
            worker.db().conn(),
            &op_id,
            status,
            &serde_json::json!({ "result": result, "error": error }),
            &at,
        );
        let mut inner = worker.lock();
        inner.store_moving = false;
        if let Some(o) = inner.ops.get_mut(&op_id) {
            o.status = if error.is_none() { OpStatus::Succeeded } else { OpStatus::Failed };
            o.error = error;
            o.result = result;
            o.finished_at = Some(at);
        }
        worker.publish(&mut inner);
    });
    Ok(operation)
}

fn run_move(host: &Arc<Host>, op_id: &str, old: &Path, new: &Path) -> Result<OpResult, Failure> {
    host.crash_point("move.start", 1);
    fs::create_dir_all(new).map_err(|e| Failure::new(ErrorKind::AccessDenied, e.to_string()).path(new))?;
    let cleanup_new = || {
        let _ = fs::remove_dir_all(new);
    };
    if let Err(e) = copy_tree(old, new) {
        cleanup_new();
        return Err(e);
    }
    host.crash_point("move.copied", 1);
    // Verify every checkpoint's copy before switching.
    let records = db::all_live_checkpoints(host.db().conn()).unwrap_or_default();
    let mut identities = Vec::new();
    for record in records.iter().filter(|r| r.state != "deleting") {
        let (original, copy) = (old.join(&record.folder), new.join(&record.folder));
        // The original must still be the recorded generation, and the copy
        // a faithful copy of it. The copy's own signature is recorded: a
        // drive with coarser timestamps (exFAT, FAT) gives it another one.
        let faithful = snap::signature(&original).is_ok_and(|sig| sig.hash == record.signature)
            && snap::copy_matches(&original, &copy).unwrap_or(false);
        match snap::signature(&copy) {
            Ok(sig) if faithful => identities.push((record.id.clone(), sig.hash, sig.identity)),
            _ if snap::presence(&original) != Presence::Present => {}
            _ => {
                cleanup_new();
                return Err(Failure::new(ErrorKind::Io, "a copied checkpoint doesn't match its original").path(&copy));
            }
        }
    }
    host.db()
        .write(|c| {
            db::set_setting(c, crate::model::SETTING_STORE, &new.to_string_lossy())?;
            for (id, signature, identity) in &identities {
                c.execute(
                    "UPDATE checkpoints SET signature = ?2, identity = ?3 WHERE id = ?1",
                    [id.as_str(), signature.as_str(), identity.as_deref().unwrap_or("")],
                )?;
            }
            let j = Journal {
                phase: "move.switched".into(),
                old_store: Some(old.to_path_buf()),
                new_store: Some(new.to_path_buf()),
                ..Default::default()
            };
            db::set_journal(c, op_id, "running", &serde_json::to_value(j).expect("journal serializes"))
        })
        .map_err(|e| {
            cleanup_new();
            Failure::new(ErrorKind::NotRecorded, e.to_string())
        })?;
    {
        let mut inner = host.lock();
        inner.store = new.to_path_buf();
        inner.store_available = true;
    }
    // The new store's drive is expected from now on.
    crate::scan::remember_drives(host);
    host.crash_point("move.switched", 1);
    remove_old_store(old);
    Ok(OpResult { count: Some(identities.len() as u32), ..Default::default() })
}

pub fn remove_old_store(old: &Path) {
    if let Ok(entries) = fs::read_dir(old) {
        for entry in entries.flatten() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    let _ = fs::remove_dir(old);
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), Failure> {
    let entries = fs::read_dir(from).map_err(|e| Failure::new(ErrorKind::ReadFailed, e.to_string()).path(from))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if snap::is_reserved_folder(&name.to_string_lossy()) {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        let meta =
            fs::symlink_metadata(&src).map_err(|e| Failure::new(ErrorKind::ReadFailed, e.to_string()).path(&src))?;
        if meta.is_dir() {
            fs::create_dir_all(&dst).map_err(|e| Failure::new(snap::fsx::write_kind(&e), e.to_string()).path(&dst))?;
            copy_tree(&src, &dst)?;
        } else if meta.is_file() {
            snap::fsx::copy_file(&src, &dst).map_err(|e| match e {
                snap::fsx::CopyError::Read(e) => Failure::new(ErrorKind::ReadFailed, e.to_string()).path(&src),
                snap::fsx::CopyError::Write(e) => Failure::new(snap::fsx::write_kind(&e), e.to_string()).path(&dst),
            })?;
        }
    }
    Ok(())
}

/// A checkpoint folder for the Open command and history rows.
pub fn reserved_names_ok(name: &str) -> bool {
    !name.ends_with(SUFFIX_NEW) && !name.ends_with(SUFFIX_OLD)
}

pub fn mark_ready(inner: &mut Inner) {
    inner.phase = Phase::Ready;
}

/// One Load stage, run over the plan.
type Stage<'a> = &'a dyn Fn(&mut LoadPlan) -> Result<(), load::StageError>;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use std::process::{Child, Command};

    /// A process that opens `file`, keeps it open for `hold` seconds, then
    /// closes it and keeps running, like a game that let go of its save.
    fn holder(file: &Path, hold: f32) -> Child {
        let script = format!("exec 3<\"$0\"; sleep {hold}; exec 3<&-; sleep 10");
        let child = Command::new("sh").arg("-c").arg(script).arg(file).spawn().unwrap();
        let id = FileId::of(file).unwrap();
        let started = Instant::now();
        while open_files::inspect(&[child.id()], &[id]).open.is_empty() {
            assert!(started.elapsed() < Duration::from_secs(5), "the holder never opened the file");
            std::thread::sleep(Duration::from_millis(5));
        }
        child
    }

    struct Held {
        _dir: tempfile::TempDir,
        paths: Vec<PathBuf>,
        children: Vec<Child>,
    }

    impl Held {
        fn files(&self) -> Vec<(&Path, FileId)> {
            self.paths.iter().map(|p| (p.as_path(), FileId::of(p).unwrap())).collect()
        }

        fn pids(&self) -> Vec<u32> {
            self.children.iter().map(Child::id).collect()
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            for child in &mut self.children {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// Save files, each held by its own process for the given seconds.
    fn held(holds: &[f32]) -> Held {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        let mut children = Vec::new();
        for (i, hold) in holds.iter().enumerate() {
            let path = dir.path().join(format!("slot{i}.sav"));
            fs::write(&path, "save").unwrap();
            children.push(holder(&path, *hold));
            paths.push(path);
        }
        Held { _dir: dir, paths, children }
    }

    #[test]
    fn a_hold_that_clears_within_the_budget_is_waited_out() {
        let h = held(&[0.3]);
        let budget = Budget::new(snap::retry::FORWARD);
        let started = Instant::now();
        wait_until_closed("g", &h.files(), || h.pids(), &budget).unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!budget.left().is_zero(), "part of the budget is left");
    }

    #[test]
    fn a_hold_that_outlasts_the_budget_is_refused_naming_the_file() {
        let h = held(&[30.0]);
        let budget = Budget::new(snap::retry::FORWARD);
        let started = Instant::now();
        let err = wait_until_closed("g", &h.files(), || h.pids(), &budget).unwrap_err();
        assert_eq!(err.kind, ErrorKind::HeldOpen);
        assert_eq!(err.paths, vec![h.paths[0].to_string_lossy().into_owned()]);
        assert!(err.detail.contains(&format!("pid {}", h.children[0].id())), "{}", err.detail);
        assert!(budget.left().is_zero());
        let waited = started.elapsed();
        assert!(waited >= Duration::from_secs(1) && waited < Duration::from_secs(3), "{waited:?}");
    }

    #[test]
    fn several_held_files_share_the_operations_one_budget() {
        // Earlier retries in the same operation already used most of the
        // budget, so a hold that alone would fit in it doesn't any more.
        let budget = Budget::new(snap::retry::FORWARD);
        while budget.left() > Duration::from_millis(300) {
            budget.wait();
        }
        let h = held(&[0.6, 30.0]);
        let started = Instant::now();
        let err = wait_until_closed("g", &h.files(), || h.pids(), &budget).unwrap_err();
        assert_eq!(err.kind, ErrorKind::HeldOpen);
        assert!(started.elapsed() < Duration::from_millis(800), "only what was left is waited");
        assert_eq!(err.paths.len(), 2, "both files still open are named: {:?}", err.paths);
    }

    #[test]
    fn a_process_that_cant_be_inspected_lets_the_load_go_on() {
        // pid 1 belongs to root: nothing found, but not "nothing open".
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slot.sav");
        fs::write(&path, "save").unwrap();
        let files = [(path.as_path(), FileId::of(&path).unwrap())];
        let budget = Budget::new(snap::retry::FORWARD);
        wait_until_closed("g", &files, || vec![1], &budget).unwrap();
        assert_eq!(budget.left(), snap::retry::FORWARD, "no waiting without a find");
    }

    #[test]
    fn a_file_found_open_is_refused_even_when_another_process_cant_be_inspected() {
        let h = held(&[30.0]);
        let budget = Budget::new(Duration::from_millis(100));
        let pids = || [vec![1], h.pids()].concat();
        let err = wait_until_closed("g", &h.files(), pids, &budget).unwrap_err();
        assert_eq!(err.kind, ErrorKind::HeldOpen);
    }

    #[test]
    fn a_game_that_isnt_running_has_nothing_to_wait_for() {
        let h = held(&[30.0]);
        let budget = Budget::new(snap::retry::FORWARD);
        wait_until_closed("g", &h.files(), Vec::new, &budget).unwrap();
    }
}
