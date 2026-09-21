use rusqlite::{Connection, params};
use savescummer_core::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

mod migration;
mod queries;
use queries::{many, one};

fn error(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::Storage, e.to_string())
}
pub struct SqliteRepository {
    connection: Mutex<Connection>,
}
impl SqliteRepository {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path).map_err(error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
            )
            .map_err(error)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(error)?;
        match version {
            0 => {
                let tx = connection.unchecked_transaction().map_err(error)?;
                tx.execute_batch(include_str!("../migrations/001_initial.sql"))
                    .map_err(error)?;
                tx.commit().map_err(error)?;
            }
            1..=3 => (),
            _ => {
                return Err(error(format!(
                    "unsupported database schema version {version}"
                )));
            }
        }
        if version <= 1 {
            migration::checkpoint_ownership(&connection)?;
        }
        if version < 3 {
            queries::migrate(&connection)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}
fn rows<T: DeserializeOwned>(connection: &Connection, query: &str) -> Result<Vec<T>> {
    let mut statement = connection.prepare(query).map_err(error)?;
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(error)?
        .map(|row| serde_json::from_str(&row.map_err(error)?).map_err(error))
        .collect()
}
fn json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(error)
}
impl Repository for SqliteRepository {
    fn load(&self) -> Result<State> {
        let connection = self.connection.lock().map_err(error)?;
        let mut state = State::default();
        let settings: Option<String> = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'settings'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        state.settings = settings
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(error)?
            .unwrap_or_default();
        let revision: Option<String> = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'revision'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        state.revision = revision
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(error)?
            .unwrap_or(0);
        state.games = rows::<Game>(&connection, "SELECT body FROM games")?
            .into_iter()
            .map(|g| (g.id.clone(), g))
            .collect();
        state.snapshots = rows::<Snapshot>(&connection, "SELECT body FROM snapshots")?
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        state.history = rows(&connection, "SELECT body FROM history ORDER BY sequence")?;
        state.operations = rows::<Operation>(&connection, "SELECT body FROM operations")?
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();
        Ok(state)
    }
    fn commit_changes(&self, changes: &MetadataChanges) -> Result<()> {
        let mut connection = self.connection.lock().map_err(error)?;
        let tx = connection.transaction().map_err(error)?;
        let mut affected = std::collections::BTreeSet::new();
        for (table, ids) in [
            ("games", &changes.deleted_games),
            ("snapshots", &changes.deleted_snapshots),
            ("history", &changes.deleted_history),
            ("operations", &changes.deleted_operations),
        ] {
            for id in ids {
                if table != "games" {
                    let game: Option<String> = tx
                        .query_row(
                            &format!("SELECT game_id FROM {table} WHERE id=?1"),
                            [id],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(error)?;
                    affected.extend(game);
                }
                if table == "snapshots" {
                    tx.execute(
                        "UPDATE history_index SET visible=0 WHERE anchor_id=?1",
                        [id],
                    )
                    .map_err(error)?;
                }
                tx.execute(&format!("DELETE FROM {table} WHERE id=?1"), [id])
                    .map_err(error)?;
            }
        }
        for game in &changes.clear_history {
            tx.execute("DELETE FROM retained_games WHERE game_id=?1", [game])
                .map_err(error)?;
            tx.execute("DELETE FROM history WHERE game_id=?1", [game])
                .map_err(error)?;
            tx.execute("DELETE FROM snapshots WHERE game_id=?1", [game])
                .map_err(error)?;
            tx.execute("UPDATE operations SET body=json_set(body,'$.snapshot_path',NULL) WHERE game_id=?1 AND json_extract(body,'$.snapshot_path') IS NOT NULL",[game]).map_err(error)?;
            affected.insert(game.clone());
        }
        for g in &changes.games {
            affected.insert(g.id.clone());
            tx.execute("INSERT INTO games VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![g.id, json(g)?])
                .map_err(error)?;
        }
        for s in &changes.snapshots {
            affected.insert(s.game_id.clone());
            tx.execute(
                "INSERT INTO snapshots VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET game_id=excluded.game_id,body=excluded.body",
                params![s.id, s.game_id, json(s)?],
            )
            .map_err(error)?;
            tx.execute(
                "UPDATE history_index SET visible=?2 WHERE anchor_id=?1 AND visible<>?2",
                params![s.id, s.removed_at.is_none()],
            )
            .map_err(error)?;
        }
        for h in &changes.history {
            affected.insert(h.game_id.clone());
            tx.execute(
                "INSERT INTO history VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET sequence=excluded.sequence,game_id=excluded.game_id,body=excluded.body",
                params![h.id, h.sequence, h.game_id, json(h)?],
            )
            .map_err(error)?;
            queries::index_history(&tx, h)?;
        }
        for o in &changes.operations {
            if !changes.clear_history.contains(&o.game_id)
                && matches!(
                    o.status,
                    OperationStatus::Failed
                        | OperationStatus::Resolved
                        | OperationStatus::RecoveryNeeded
                )
            {
                tx.execute(
                    "INSERT OR IGNORE INTO retained_games VALUES(?1)",
                    [&o.game_id],
                )
                .map_err(error)?;
            }
            tx.execute(
                "INSERT INTO operations VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET request_id=excluded.request_id,game_id=excluded.game_id,body=excluded.body",
                params![o.id, o.request_id, o.game_id, json(o)?],
            )
            .map_err(error)?;
        }
        for game in affected {
            queries::markers(&tx, &game)?;
            tx.execute("INSERT INTO history_revisions VALUES(?1,1) ON CONFLICT(game_id) DO UPDATE SET revision=revision+1",[game]).map_err(error)?;
        }
        for (key, value) in [
            ("revision", changes.revision),
            (
                "history_sequence",
                changes.history_sequence.max(
                    changes
                        .history
                        .iter()
                        .map(|h| h.sequence)
                        .max()
                        .unwrap_or(0),
                ),
            ),
            (
                "snapshot_order",
                changes.snapshot_order.max(
                    changes
                        .snapshots
                        .iter()
                        .map(|s| s.registration_order)
                        .max()
                        .unwrap_or(0),
                ),
            ),
        ] {
            tx.execute("INSERT INTO metadata VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=MAX(CAST(metadata.value AS INTEGER),CAST(excluded.value AS INTEGER)) WHERE CAST(metadata.value AS INTEGER)<CAST(excluded.value AS INTEGER)",params![key,value.to_string()]).map_err(error)?;
        }
        if let Some(settings) = &changes.settings {
            tx.execute("INSERT INTO metadata VALUES('settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[json(settings)?]).map_err(error)?;
        }
        tx.commit().map_err(error)
    }
    fn boot(&self) -> Result<State> {
        let db = self.connection.lock().map_err(error)?;
        let mut state = State {
            settings: one(&db, "SELECT value FROM metadata WHERE key=?1", "settings")?
                .unwrap_or_default(),
            ..Default::default()
        };
        for (key, value) in [
            ("revision", &mut state.revision),
            ("history_sequence", &mut state.history_sequence),
            ("snapshot_order", &mut state.snapshot_order),
        ] {
            *value = db
                .query_row(
                    "SELECT CAST(value AS INTEGER) FROM metadata WHERE key=?1",
                    [key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(error)?
                .unwrap_or(0);
        }
        state.games = rows::<Game>(&db, "SELECT body FROM games")?
            .into_iter()
            .map(|g| (g.id.clone(), g))
            .collect();
        state.operations = rows::<Operation>(&db,"SELECT body FROM operations WHERE json_extract(body,'$.status') IN ('pending','recovery_needed')")?.into_iter().map(|o|(o.id.clone(),o)).collect();
        for game in state.games.keys() {
            let op: Option<Operation> = one(
                &db,
                "SELECT body FROM operations WHERE game_id=?1 ORDER BY json_extract(body,'$.started_at') DESC,id DESC LIMIT 1",
                game,
            )?;
            if let Some(op) = op {
                state.operations.insert(op.id.clone(), op);
            }
        }
        Ok(state)
    }
    fn game_snapshots(&self, game: &str) -> Result<Vec<Snapshot>> {
        many(
            &*self.connection.lock().map_err(error)?,
            "SELECT body FROM snapshots WHERE game_id=?1 AND json_extract(body,'$.removed_at') IS NULL",
            game,
        )
    }
    fn game_operations(&self, game: &str) -> Result<Vec<Operation>> {
        many(
            &*self.connection.lock().map_err(error)?,
            "SELECT body FROM operations WHERE game_id=?1",
            game,
        )
    }
    fn operation_page(&self, game: &str, after: &str, limit: usize) -> Result<Vec<Operation>> {
        let db = self.connection.lock().map_err(error)?;
        db.prepare("SELECT body FROM operations WHERE game_id=?1 AND id>?2 ORDER BY id LIMIT ?3")
            .map_err(error)?
            .query_map(params![game, after, limit], |r| r.get::<_, String>(0))
            .map_err(error)?
            .map(|r| serde_json::from_str(&r.map_err(error)?).map_err(error))
            .collect()
    }
    fn unpublished_snapshot(&self, game: &str, path: &Path, identity: &str) -> Result<bool> {
        self.connection.lock().map_err(error)?.query_row("SELECT EXISTS(SELECT 1 FROM operations WHERE game_id=?1 AND json_extract(body,'$.snapshot_path')=?2 AND json_extract(body,'$.staging_identity')=?3 AND json_extract(body,'$.status')<>'completed')",
            params![game,path.to_string_lossy(),identity],|r|r.get(0)).map_err(error)
    }
    fn reserved_snapshot_paths(&self, game: &str) -> Result<Vec<PathBuf>> {
        let db = self.connection.lock().map_err(error)?;
        db.prepare("SELECT json_extract(body,'$.snapshot_path') FROM operations WHERE game_id=?1 AND json_extract(body,'$.status')<>'completed' AND json_extract(body,'$.snapshot_path') IS NOT NULL").map_err(error)?
            .query_map([game],|r|r.get::<_,String>(0)).map_err(error)?.map(|r|r.map(PathBuf::from).map_err(error)).collect()
    }
    fn visit_reserved_paths(&self, visit: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
        let db = self.connection.lock().map_err(error)?;
        let mut statement = db.prepare("SELECT json_extract(body,'$.path') FROM snapshots WHERE json_extract(body,'$.removed_at') IS NULL UNION ALL SELECT j.value FROM operations,json_each(json_array(json_extract(body,'$.recovery'),json_extract(body,'$.recovery_staging'),json_extract(body,'$.staging'),json_extract(body,'$.original'))) j WHERE j.value IS NOT NULL").map_err(error)?;
        for row in statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(error)?
        {
            visit(Path::new(&row.map_err(error)?))?;
        }
        Ok(())
    }
    fn snapshot(&self, id: &str) -> Result<Option<Snapshot>> {
        one(
            &*self.connection.lock().map_err(error)?,
            "SELECT body FROM snapshots WHERE id=?1",
            id,
        )
    }
    fn snapshot_at_path(&self, path: &Path) -> Result<Option<Snapshot>> {
        one(
            &*self.connection.lock().map_err(error)?,
            "SELECT body FROM snapshots WHERE json_extract(body,'$.path')=?1 AND json_extract(body,'$.removed_at') IS NULL LIMIT 1",
            &path.to_string_lossy(),
        )
    }
    fn snapshot_path_reserved(&self, path: &Path) -> Result<bool> {
        self.connection.lock().map_err(error)?.query_row("SELECT EXISTS(SELECT 1 FROM snapshots WHERE json_extract(body,'$.path')=?1 AND json_extract(body,'$.removed_at') IS NULL) OR EXISTS(SELECT 1 FROM operations WHERE json_extract(body,'$.snapshot_path')=?1 AND json_extract(body,'$.status')<>'completed')",[path.to_string_lossy()],|r|r.get(0)).map_err(error)
    }
    fn operation(&self, id: &str) -> Result<Option<Operation>> {
        one(
            &*self.connection.lock().map_err(error)?,
            "SELECT body FROM operations WHERE id=?1",
            id,
        )
    }
    fn request_operation(&self, id: &str) -> Result<Option<Operation>> {
        one(
            &*self.connection.lock().map_err(error)?,
            "SELECT body FROM operations WHERE request_id=?1",
            id,
        )
    }
    fn history_entry(&self, id: &str) -> Result<Option<History>> {
        one(
            &*self.connection.lock().map_err(error)?,
            "SELECT body FROM history WHERE id=?1",
            id,
        )
    }
    fn checkpoint_history(&self, id: &str) -> Result<Option<History>> {
        one(
            &*self.connection.lock().map_err(error)?,
            "SELECT h.body FROM history_index i JOIN history h ON h.id=i.id WHERE i.anchor_id=?1 ORDER BY i.sequence LIMIT 1",
            id,
        )
    }
    fn reserved_paths(&self) -> Result<Vec<PathBuf>> {
        let db = self.connection.lock().map_err(error)?;
        let mut result = Vec::new();
        let mut statement = db.prepare("SELECT json_extract(body,'$.path') FROM snapshots WHERE json_extract(body,'$.removed_at') IS NULL UNION SELECT j.value FROM operations,json_each(json_array(json_extract(body,'$.recovery'),json_extract(body,'$.recovery_staging'),json_extract(body,'$.staging'),json_extract(body,'$.original'))) j WHERE j.value IS NOT NULL").map_err(error)?;
        for row in statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(error)?
        {
            result.push(PathBuf::from(row.map_err(error)?));
        }
        Ok(result)
    }
    fn history_status(&self, game: &str) -> Result<HistoryStatus> {
        let db = self.connection.lock().map_err(error)?;
        db.query_row("SELECT COALESCE((SELECT revision FROM history_revisions WHERE game_id=?1),0),EXISTS(SELECT 1 FROM history_index WHERE game_id=?1 AND visible=1),EXISTS(SELECT 1 FROM history WHERE game_id=?1) OR EXISTS(SELECT 1 FROM snapshots WHERE game_id=?1) OR EXISTS(SELECT 1 FROM retained_games WHERE game_id=?1)", [game], |r|Ok(HistoryStatus{revision:r.get(0)?,has_visible_history:r.get(1)?,can_flush:r.get(2)?})).map_err(error)
    }
    fn history_rows(&self, game: &str, before: u64, limit: usize) -> Result<Vec<History>> {
        let db = self.connection.lock().map_err(error)?;
        db.prepare("SELECT h.body FROM history_index i JOIN history h ON h.id=i.id WHERE i.game_id=?1 AND i.visible=1 AND i.sequence<?2 ORDER BY i.sequence DESC LIMIT ?3").map_err(error)?
            .query_map(params![game,before.min(i64::MAX as u64),limit],|r|r.get::<_,String>(0)).map_err(error)?
            .map(|r|serde_json::from_str(&r.map_err(error)?).map_err(error)).collect()
    }
}
use rusqlite::OptionalExtension;
