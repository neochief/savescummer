//! SQLite storage, owned only by the host: settings, game records,
//! checkpoint records (with labels), history and the operation journal.
//! Game files never go in here.
//!
//! Every function takes a `&Connection`, so the host can group several
//! writes into one transaction ([`Storage::write`]); in-memory state updates
//! only after it commits. Visible-history rules live in the core; storage
//! only keeps their result as an index for fast pages.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::Value;

use savescummer_core::common::RecordedTarget;
use savescummer_core::history::RowKind;

pub use rusqlite::Error;
pub type Result<T> = rusqlite::Result<T>;

const SCHEMA_VERSION: i64 = 2;

const MIGRATION_1: &str = r#"
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE games (id TEXT PRIMARY KEY, data TEXT NOT NULL);
CREATE TABLE checkpoints (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    game_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    folder TEXT NOT NULL,
    created_at TEXT NOT NULL,
    label TEXT,
    targets TEXT NOT NULL,
    signature TEXT NOT NULL,
    identity TEXT,
    size INTEGER NOT NULL,
    state TEXT NOT NULL
);
CREATE INDEX checkpoints_by_game ON checkpoints (game_id, state, kind, seq);
CREATE TABLE history (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    game_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    at TEXT NOT NULL,
    session TEXT,
    checkpoint_id TEXT,
    recovery_id TEXT,
    reverted_row TEXT,
    removed INTEGER NOT NULL DEFAULT 0,
    cloud_check INTEGER NOT NULL DEFAULT 0,
    cloud_replaced INTEGER NOT NULL DEFAULT 0,
    visible INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX history_pages ON history (game_id, visible, seq);
CREATE INDEX history_by_checkpoint ON history (checkpoint_id);
CREATE INDEX history_by_recovery ON history (recovery_id);
CREATE TABLE operations (
    id TEXT PRIMARY KEY,
    request_id TEXT UNIQUE,
    game_id TEXT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    journal TEXT,
    result TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT
);
CREATE INDEX operations_open ON operations (status);
CREATE TABLE notices (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    at TEXT NOT NULL
);
"#;

/// When the host was running, so time it didn't observe never joins two
/// sessions. `last_seen_at` moves with a heartbeat; a run without `ended_at`
/// ended in a crash or power loss, somewhere after `last_seen_at`.
const MIGRATION_2: &str = r#"
CREATE TABLE host_runs (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    started_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    ended_at TEXT
);
"#;

pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Opens (creating if needed) and migrates the database. Durability is
    /// never traded for fewer writes: WAL with full syncs.
    pub fn open(path: &Path) -> Result<Storage> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        let mut storage = Storage { conn };
        storage.migrate()?;
        Ok(storage)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::InvalidParameterName(format!(
                "the database is from a newer version (schema {version}, this build reads {SCHEMA_VERSION})"
            )));
        }
        if version < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_1)?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_2)?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Runs `f` in one transaction: everything or nothing.
    pub fn write<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Counts pages written so far, for tests proving a Save writes little.
    pub fn total_changes(&self) -> u64 {
        self.conn.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as u64
    }
}

// ---- settings -----------------------------------------------------------------

pub fn setting(c: &Connection, key: &str) -> Result<Option<String>> {
    c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub fn set_setting(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---- games --------------------------------------------------------------------

pub fn games(c: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = c.prepare("SELECT id, data FROM games ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

pub fn put_game(c: &Connection, id: &str, data: &str) -> Result<()> {
    c.execute(
        "INSERT INTO games (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data = excluded.data WHERE data <> excluded.data",
        params![id, data],
    )?;
    Ok(())
}

// ---- checkpoints --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRow {
    pub seq: i64,
    pub id: String,
    pub game_id: String,
    /// `saved` or `recovery`.
    pub kind: String,
    /// Relative to the checkpoint store.
    pub folder: String,
    pub created_at: String,
    pub label: Option<String>,
    pub targets: Vec<RecordedTarget>,
    pub signature: String,
    pub identity: Option<String>,
    pub size: u64,
    /// `ok`, `unavailable`, `retired` (changed or gone outside the app),
    /// `deleting` (renamed for disposal) or `deleted`.
    pub state: String,
}

impl CheckpointRow {
    /// Retired and deleted checkpoints no longer exist; unavailable ones do.
    pub fn exists(&self) -> bool {
        matches!(self.state.as_str(), "ok" | "unavailable")
    }
}

const CHECKPOINT_COLUMNS: &str =
    "seq, id, game_id, kind, folder, created_at, label, targets, signature, identity, size, state";

fn checkpoint_row(r: &Row<'_>) -> Result<CheckpointRow> {
    let targets: String = r.get(7)?;
    Ok(CheckpointRow {
        seq: r.get(0)?,
        id: r.get(1)?,
        game_id: r.get(2)?,
        kind: r.get(3)?,
        folder: r.get(4)?,
        created_at: r.get(5)?,
        label: r.get(6)?,
        targets: serde_json::from_str(&targets).unwrap_or_default(),
        signature: r.get(8)?,
        identity: r.get(9)?,
        size: r.get::<_, i64>(10)? as u64,
        state: r.get(11)?,
    })
}

/// Inserts a checkpoint record and returns its registration order.
pub fn insert_checkpoint(c: &Connection, row: &CheckpointRow) -> Result<i64> {
    c.execute(
        "INSERT INTO checkpoints (id, game_id, kind, folder, created_at, label, targets, signature, identity, size, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            row.id,
            row.game_id,
            row.kind,
            row.folder,
            row.created_at,
            row.label,
            serde_json::to_string(&row.targets).expect("targets serialize"),
            row.signature,
            row.identity,
            row.size as i64,
            row.state,
        ],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn checkpoint(c: &Connection, id: &str) -> Result<Option<CheckpointRow>> {
    c.query_row(&format!("SELECT {CHECKPOINT_COLUMNS} FROM checkpoints WHERE id = ?1"), [id], checkpoint_row).optional()
}

/// A game's checkpoints that still exist (ok or unavailable), oldest first.
pub fn existing_checkpoints(c: &Connection, game: &str) -> Result<Vec<CheckpointRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {CHECKPOINT_COLUMNS} FROM checkpoints WHERE game_id = ?1 AND state IN ('ok', 'unavailable') ORDER BY seq"
    ))?;
    let rows = stmt.query_map([game], checkpoint_row)?;
    rows.collect()
}

/// Every checkpoint record that still exists or is being deleted, for scans.
pub fn all_live_checkpoints(c: &Connection) -> Result<Vec<CheckpointRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {CHECKPOINT_COLUMNS} FROM checkpoints WHERE state IN ('ok', 'unavailable', 'deleting') ORDER BY seq"
    ))?;
    let rows = stmt.query_map([], checkpoint_row)?;
    rows.collect()
}

pub fn set_checkpoint_state(c: &Connection, id: &str, state: &str) -> Result<()> {
    c.execute("UPDATE checkpoints SET state = ?2 WHERE id = ?1", params![id, state])?;
    Ok(())
}

pub fn set_checkpoint_folder(c: &Connection, id: &str, folder: &str) -> Result<()> {
    c.execute("UPDATE checkpoints SET folder = ?2 WHERE id = ?1", params![id, folder])?;
    Ok(())
}

/// Sets a saved checkpoint's label. False when it doesn't exist any more.
pub fn set_label(c: &Connection, id: &str, label: Option<&str>) -> Result<bool> {
    let changed = c.execute(
        "UPDATE checkpoints SET label = ?2 WHERE id = ?1 AND kind = 'saved' AND state IN ('ok', 'unavailable')",
        params![id, label],
    )?;
    Ok(changed == 1)
}

/// Removes every record of a game (Flush), for the checkpoints listed.
pub fn forget_checkpoints(c: &Connection, ids: &[String]) -> Result<()> {
    let mut stmt = c.prepare("DELETE FROM checkpoints WHERE id = ?1")?;
    for id in ids {
        stmt.execute([id])?;
    }
    Ok(())
}

// ---- history ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub seq: i64,
    pub id: String,
    pub game_id: String,
    pub kind: RowKind,
    pub at: String,
    pub session: Option<String>,
    /// Saved: its checkpoint. Loaded/Reverted: the checkpoint restored.
    pub checkpoint_id: Option<String>,
    /// Loaded/Reverted: the recovery checkpoint made before restoring.
    pub recovery_id: Option<String>,
    /// Reverted: the row whose recovery checkpoint was restored.
    pub reverted_row: Option<String>,
    pub removed: u32,
    pub cloud_check: bool,
    pub cloud_replaced: bool,
    pub visible: bool,
}

impl HistoryRow {
    /// The checkpoint whose existence keeps this row visible.
    pub fn owned_checkpoint(&self) -> Option<&str> {
        match self.kind {
            RowKind::Saved => self.checkpoint_id.as_deref(),
            RowKind::Loaded | RowKind::Reverted => self.recovery_id.as_deref(),
            _ => None,
        }
    }
}

const HISTORY_COLUMNS: &str = "seq, id, game_id, kind, at, session, checkpoint_id, recovery_id, reverted_row, removed, cloud_check, cloud_replaced, visible";

fn history_row(r: &Row<'_>) -> Result<HistoryRow> {
    let kind: String = r.get(3)?;
    Ok(HistoryRow {
        seq: r.get(0)?,
        id: r.get(1)?,
        game_id: r.get(2)?,
        kind: RowKind::parse(&kind).unwrap_or(RowKind::Saved),
        at: r.get(4)?,
        session: r.get(5)?,
        checkpoint_id: r.get(6)?,
        recovery_id: r.get(7)?,
        reverted_row: r.get(8)?,
        removed: r.get::<_, i64>(9)? as u32,
        cloud_check: r.get::<_, i64>(10)? != 0,
        cloud_replaced: r.get::<_, i64>(11)? != 0,
        visible: r.get::<_, i64>(12)? != 0,
    })
}

pub fn insert_row(c: &Connection, row: &HistoryRow) -> Result<i64> {
    c.execute(
        "INSERT INTO history (id, game_id, kind, at, session, checkpoint_id, recovery_id, reverted_row, removed, cloud_check, cloud_replaced, visible)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            row.id,
            row.game_id,
            row.kind.as_str(),
            row.at,
            row.session,
            row.checkpoint_id,
            row.recovery_id,
            row.reverted_row,
            row.removed as i64,
            row.cloud_check as i64,
            row.cloud_replaced as i64,
            row.visible as i64,
        ],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn row(c: &Connection, id: &str) -> Result<Option<HistoryRow>> {
    c.query_row(&format!("SELECT {HISTORY_COLUMNS} FROM history WHERE id = ?1"), [id], history_row).optional()
}

/// A page of visible rows, newest first, strictly before `before_seq`.
pub fn history_page(c: &Connection, game: &str, before_seq: Option<i64>, limit: usize) -> Result<Vec<HistoryRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {HISTORY_COLUMNS} FROM history WHERE game_id = ?1 AND visible = 1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![game, before_seq.unwrap_or(i64::MAX), limit as i64], history_row)?;
    rows.collect()
}

pub fn has_visible_history(c: &Connection, game: &str) -> Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM history WHERE game_id = ?1 AND visible = 1)", [game], |r| r.get(0))
}

/// Every row of a game with whether its owned checkpoint exists: what the
/// core's visibility rule needs.
pub fn visibility_inputs(c: &Connection, game: &str) -> Result<Vec<(HistoryRow, bool)>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {HISTORY_COLUMNS},
                (SELECT state IN ('ok', 'unavailable') FROM checkpoints k
                  WHERE k.id = CASE h.kind WHEN 'saved' THEN h.checkpoint_id ELSE h.recovery_id END)
         FROM history h WHERE game_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map([game], |r| {
        let exists: Option<bool> = r.get(13)?;
        Ok((history_row(r)?, exists.unwrap_or(false)))
    })?;
    rows.collect()
}

pub fn set_visible(c: &Connection, id: &str, visible: bool) -> Result<()> {
    c.execute("UPDATE history SET visible = ?2 WHERE id = ?1", params![id, visible as i64])?;
    Ok(())
}

/// Rows waiting for the Steam Cloud check at the game's next start.
pub fn pending_cloud_checks(c: &Connection, game: &str) -> Result<Vec<HistoryRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {HISTORY_COLUMNS} FROM history WHERE game_id = ?1 AND cloud_check = 1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map([game], history_row)?;
    rows.collect()
}

pub fn finish_cloud_check(c: &Connection, id: &str, replaced: bool) -> Result<()> {
    c.execute("UPDATE history SET cloud_check = 0, cloud_replaced = ?2 WHERE id = ?1", params![id, replaced as i64])?;
    Ok(())
}

pub fn forget_history(c: &Connection, game: &str) -> Result<()> {
    c.execute("DELETE FROM history WHERE game_id = ?1", [game])?;
    Ok(())
}

// ---- operations -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct OperationRow {
    pub id: String,
    pub request_id: Option<String>,
    pub game_id: Option<String>,
    pub kind: String,
    /// `accepted`, `running`, `succeeded`, `failed` or `cancelled`.
    pub status: String,
    pub journal: Option<Value>,
    pub result: Option<Value>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

const OPERATION_COLUMNS: &str = "id, request_id, game_id, kind, status, journal, result, created_at, finished_at";

fn operation_row(r: &Row<'_>) -> Result<OperationRow> {
    let journal: Option<String> = r.get(5)?;
    let result: Option<String> = r.get(6)?;
    Ok(OperationRow {
        id: r.get(0)?,
        request_id: r.get(1)?,
        game_id: r.get(2)?,
        kind: r.get(3)?,
        status: r.get(4)?,
        journal: journal.and_then(|j| serde_json::from_str(&j).ok()),
        result: result.and_then(|j| serde_json::from_str(&j).ok()),
        created_at: r.get(7)?,
        finished_at: r.get(8)?,
    })
}

pub fn insert_operation(c: &Connection, op: &OperationRow) -> Result<()> {
    c.execute(
        "INSERT INTO operations (id, request_id, game_id, kind, status, journal, result, created_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            op.id,
            op.request_id,
            op.game_id,
            op.kind,
            op.status,
            op.journal.as_ref().map(|j| j.to_string()),
            op.result.as_ref().map(|j| j.to_string()),
            op.created_at,
            op.finished_at,
        ],
    )?;
    Ok(())
}

pub fn operation(c: &Connection, id: &str) -> Result<Option<OperationRow>> {
    c.query_row(&format!("SELECT {OPERATION_COLUMNS} FROM operations WHERE id = ?1"), [id], operation_row).optional()
}

pub fn operation_by_request(c: &Connection, request_id: &str) -> Result<Option<OperationRow>> {
    c.query_row(
        &format!("SELECT {OPERATION_COLUMNS} FROM operations WHERE request_id = ?1"),
        [request_id],
        operation_row,
    )
    .optional()
}

/// Operations that never finished: what recovery at startup resolves.
pub fn unfinished_operations(c: &Connection) -> Result<Vec<OperationRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {OPERATION_COLUMNS} FROM operations WHERE status IN ('accepted', 'running', 'blocked') ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([], operation_row)?;
    rows.collect()
}

/// Operations resolved by recovery rule 4 whose game was released: their
/// material is kept and must never be cleaned up as leftovers.
pub fn kept_operations(c: &Connection) -> Result<Vec<OperationRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {OPERATION_COLUMNS} FROM operations WHERE status = 'kept' ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([], operation_row)?;
    rows.collect()
}

/// Records the journal before a step that changes files.
pub fn set_journal(c: &Connection, id: &str, status: &str, journal: &Value) -> Result<()> {
    c.execute(
        "UPDATE operations SET status = ?2, journal = ?3 WHERE id = ?1",
        params![id, status, journal.to_string()],
    )?;
    Ok(())
}

pub fn finish_operation(c: &Connection, id: &str, status: &str, result: &Value, at: &str) -> Result<()> {
    c.execute(
        "UPDATE operations SET status = ?2, result = ?3, finished_at = ?4 WHERE id = ?1",
        params![id, status, result.to_string(), at],
    )?;
    Ok(())
}

// ---- notices --------------------------------------------------------------------

pub fn add_notice(c: &Connection, game: &str, kind: &str, at: &str) -> Result<()> {
    c.execute("INSERT INTO notices (game_id, kind, at) VALUES (?1, ?2, ?3)", params![game, kind, at])?;
    Ok(())
}

pub fn notices(c: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = c.prepare("SELECT game_id, kind, at FROM notices ORDER BY seq")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

pub fn clear_notices(c: &Connection, game: &str) -> Result<()> {
    c.execute("DELETE FROM notices WHERE game_id = ?1", [game])?;
    Ok(())
}

// ---- host runs -----------------------------------------------------------------

/// Runs kept; older ones are dropped as new ones start.
const RUNS_KEPT: i64 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRun {
    pub id: String,
    pub started_at: String,
    pub last_seen_at: String,
    /// None: the run didn't end cleanly (or is the current one).
    pub ended_at: Option<String>,
}

pub fn start_run(c: &Connection, id: &str, at: &str) -> Result<()> {
    c.execute("INSERT INTO host_runs (id, started_at, last_seen_at) VALUES (?1, ?2, ?2)", params![id, at])?;
    c.execute("DELETE FROM host_runs WHERE seq <= (SELECT MAX(seq) FROM host_runs) - ?1", [RUNS_KEPT])?;
    Ok(())
}

pub fn touch_run(c: &Connection, id: &str, at: &str) -> Result<()> {
    c.execute("UPDATE host_runs SET last_seen_at = ?2 WHERE id = ?1", params![id, at])?;
    Ok(())
}

pub fn end_run(c: &Connection, id: &str, at: &str) -> Result<()> {
    c.execute("UPDATE host_runs SET last_seen_at = ?2, ended_at = ?2 WHERE id = ?1", params![id, at])?;
    Ok(())
}

/// The most recent runs, newest first.
pub fn runs(c: &Connection, limit: usize) -> Result<Vec<HostRun>> {
    let mut stmt =
        c.prepare("SELECT id, started_at, last_seen_at, ended_at FROM host_runs ORDER BY seq DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok(HostRun { id: r.get(0)?, started_at: r.get(1)?, last_seen_at: r.get(2)?, ended_at: r.get(3)? })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use savescummer_core::Filter;

    fn open() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("host.db")).unwrap();
        (dir, storage)
    }

    fn cp(id: &str, game: &str, kind: &str) -> CheckpointRow {
        CheckpointRow {
            seq: 0,
            id: id.into(),
            game_id: game.into(),
            kind: kind.into(),
            folder: format!("G/{id}"),
            created_at: "2026-09-24T19:25:03Z".into(),
            label: None,
            targets: vec![RecordedTarget {
                root: "C:/G".into(),
                filter: Filter::Exact("saves".into()),
                excludes: vec![],
                absent: false,
                folder: "saves".into(),
            }],
            signature: "sig".into(),
            identity: None,
            size: 10,
            state: "ok".into(),
        }
    }

    fn hrow(id: &str, game: &str, kind: RowKind, checkpoint: Option<&str>) -> HistoryRow {
        HistoryRow {
            seq: 0,
            id: id.into(),
            game_id: game.into(),
            kind,
            at: "2026-09-24T19:25:03Z".into(),
            session: None,
            checkpoint_id: checkpoint.map(str::to_string),
            recovery_id: None,
            reverted_row: None,
            removed: 0,
            cloud_check: false,
            cloud_replaced: false,
            visible: true,
        }
    }

    #[test]
    fn a_save_is_one_transaction_that_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host.db");
        {
            let mut s = Storage::open(&path).unwrap();
            s.write(|c| {
                insert_checkpoint(c, &cp("a", "g", "saved"))?;
                insert_row(c, &hrow("r1", "g", RowKind::Saved, Some("a")))?;
                Ok(())
            })
            .unwrap();
        }
        let s = Storage::open(&path).unwrap();
        assert_eq!(existing_checkpoints(s.conn(), "g").unwrap().len(), 1);
        assert_eq!(history_page(s.conn(), "g", None, 10).unwrap().len(), 1);
    }

    #[test]
    fn a_failed_transaction_leaves_nothing() {
        let (_d, mut s) = open();
        let result: Result<()> = s.write(|c| {
            insert_checkpoint(c, &cp("a", "g", "saved"))?;
            insert_checkpoint(c, &cp("a", "g", "saved"))?; // duplicate id fails
            Ok(())
        });
        assert!(result.is_err());
        assert!(existing_checkpoints(s.conn(), "g").unwrap().is_empty());
    }

    #[test]
    fn a_save_writes_nothing_it_didnt_change() {
        let (_d, mut s) = open();
        for i in 0..50 {
            s.write(|c| {
                insert_checkpoint(c, &cp(&format!("c{i}"), "other", "saved"))?;
                insert_row(c, &hrow(&format!("r{i}"), "other", RowKind::Saved, Some(&format!("c{i}"))))
            })
            .unwrap();
        }
        let before = s.total_changes();
        s.write(|c| {
            insert_checkpoint(c, &cp("new", "g", "saved"))?;
            insert_row(c, &hrow("new-row", "g", RowKind::Saved, Some("new")))
        })
        .unwrap();
        assert_eq!(s.total_changes() - before, 2, "one checkpoint row and one history row");
        // Re-putting an unchanged game record changes nothing.
        s.write(|c| put_game(c, "g", "{}")).unwrap();
        let before = s.total_changes();
        s.write(|c| put_game(c, "g", "{}")).unwrap();
        assert_eq!(s.total_changes(), before);
    }

    #[test]
    fn pages_are_newest_first_and_stable_with_equal_times() {
        let (_d, mut s) = open();
        s.write(|c| {
            for i in 0..5 {
                insert_checkpoint(c, &cp(&format!("c{i}"), "g", "saved"))?;
                insert_row(c, &hrow(&format!("r{i}"), "g", RowKind::Saved, Some(&format!("c{i}"))))?;
            }
            Ok(())
        })
        .unwrap();
        let first = history_page(s.conn(), "g", None, 2).unwrap();
        assert_eq!(first.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["r4", "r3"]);
        let second = history_page(s.conn(), "g", Some(first[1].seq), 2).unwrap();
        assert_eq!(second.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["r2", "r1"]);
    }

    #[test]
    fn labels_only_on_existing_saved_checkpoints() {
        let (_d, mut s) = open();
        s.write(|c| {
            insert_checkpoint(c, &cp("a", "g", "saved"))?;
            insert_checkpoint(c, &cp("b", "g", "recovery"))
        })
        .unwrap();
        assert!(set_label(s.conn(), "a", Some("Before boss")).unwrap());
        assert!(!set_label(s.conn(), "b", Some("x")).unwrap(), "recovery checkpoints have no label");
        set_checkpoint_state(s.conn(), "a", "deleted").unwrap();
        assert!(!set_label(s.conn(), "a", Some("y")).unwrap(), "gone");
        assert_eq!(
            checkpoint(s.conn(), "a").unwrap().unwrap().label.as_deref(),
            Some("Before boss"),
            "kept after deletion"
        );
    }

    #[test]
    fn visibility_inputs_follow_checkpoint_state() {
        let (_d, mut s) = open();
        s.write(|c| {
            insert_checkpoint(c, &cp("a", "g", "saved"))?;
            insert_row(c, &hrow("r", "g", RowKind::Saved, Some("a")))?;
            insert_row(c, &hrow("m", "g", RowKind::GameStarted, None))
        })
        .unwrap();
        let inputs = visibility_inputs(s.conn(), "g").unwrap();
        assert!(inputs[0].1);
        assert!(!inputs[1].1);
        set_checkpoint_state(s.conn(), "a", "retired").unwrap();
        assert!(!visibility_inputs(s.conn(), "g").unwrap()[0].1);
    }

    #[test]
    fn operations_by_request_and_unfinished() {
        let (_d, mut s) = open();
        let op = OperationRow {
            id: "op1".into(),
            request_id: Some("req".into()),
            game_id: Some("g".into()),
            kind: "save".into(),
            status: "accepted".into(),
            journal: None,
            result: None,
            created_at: "t".into(),
            finished_at: None,
        };
        s.write(|c| insert_operation(c, &op)).unwrap();
        assert_eq!(operation_by_request(s.conn(), "req").unwrap().unwrap().id, "op1");
        assert_eq!(unfinished_operations(s.conn()).unwrap().len(), 1);
        finish_operation(s.conn(), "op1", "succeeded", &serde_json::json!({}), "t2").unwrap();
        assert!(unfinished_operations(s.conn()).unwrap().is_empty());
    }

    #[test]
    fn host_runs_record_clean_and_unclean_ends() {
        let (_d, mut s) = open();
        s.write(|c| start_run(c, "r1", "t1")).unwrap();
        touch_run(s.conn(), "r1", "t2").unwrap();
        // r1 crashed: never ended. r2 ends cleanly.
        s.write(|c| start_run(c, "r2", "t3")).unwrap();
        end_run(s.conn(), "r2", "t4").unwrap();
        let runs = runs(s.conn(), 10).unwrap();
        assert_eq!(
            runs,
            vec![
                HostRun {
                    id: "r2".into(),
                    started_at: "t3".into(),
                    last_seen_at: "t4".into(),
                    ended_at: Some("t4".into())
                },
                HostRun { id: "r1".into(), started_at: "t1".into(), last_seen_at: "t2".into(), ended_at: None },
            ]
        );
        for i in 0..RUNS_KEPT + 5 {
            start_run(s.conn(), &format!("x{i}"), "t").unwrap();
        }
        assert_eq!(
            s.conn().query_row("SELECT COUNT(*) FROM host_runs", [], |r| r.get::<_, i64>(0)).unwrap(),
            RUNS_KEPT
        );
    }

    #[test]
    fn a_version_1_database_gains_host_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATION_1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute("INSERT INTO settings (key, value) VALUES ('k', 'v')", []).unwrap();
        }
        let s = Storage::open(&path).unwrap();
        assert_eq!(setting(s.conn(), "k").unwrap().as_deref(), Some("v"));
        start_run(s.conn(), "r", "t").unwrap();
        assert_eq!(runs(s.conn(), 1).unwrap().len(), 1);
    }

    #[test]
    fn a_newer_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host.db");
        Storage::open(&path).unwrap().conn().pragma_update(None, "user_version", 99).unwrap();
        assert!(Storage::open(&path).is_err());
    }
}
