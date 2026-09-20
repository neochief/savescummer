use rusqlite::{Connection, params};
use savescummer_core::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{path::Path, sync::Mutex};

mod migration;

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
            1 | 2 => (),
            _ => {
                return Err(error(format!(
                    "unsupported database schema version {version}"
                )));
            }
        }
        if version <= 1 {
            migration::checkpoint_ownership(&connection)?;
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
    fn commit(&self, state: &State) -> Result<()> {
        let mut connection = self.connection.lock().map_err(error)?;
        let tx = connection.transaction().map_err(error)?;
        // Small per-user libraries: one simple atomic transaction is preferable to
        // distributing transaction policy across operation-specific SQL methods.
        tx.execute_batch("DELETE FROM games; DELETE FROM snapshots; DELETE FROM history; DELETE FROM operations;").map_err(error)?;
        for g in state.games.values() {
            tx.execute("INSERT INTO games VALUES (?1, ?2)", params![g.id, json(g)?])
                .map_err(error)?;
        }
        for s in state.snapshots.values() {
            tx.execute(
                "INSERT INTO snapshots VALUES (?1, ?2, ?3)",
                params![s.id, s.game_id, json(s)?],
            )
            .map_err(error)?;
        }
        for h in &state.history {
            tx.execute(
                "INSERT INTO history VALUES (?1, ?2, ?3, ?4)",
                params![h.id, h.sequence, h.game_id, json(h)?],
            )
            .map_err(error)?;
        }
        for o in state.operations.values() {
            tx.execute(
                "INSERT INTO operations VALUES (?1, ?2, ?3, ?4)",
                params![o.id, o.request_id, o.game_id, json(o)?],
            )
            .map_err(error)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO metadata VALUES ('revision', ?1)",
            [state.revision.to_string()],
        )
        .map_err(error)?;
        tx.execute(
            "INSERT OR REPLACE INTO metadata VALUES ('settings', ?1)",
            [json(&state.settings)?],
        )
        .map_err(error)?;
        tx.commit().map_err(error)
    }
}
use rusqlite::OptionalExtension;
