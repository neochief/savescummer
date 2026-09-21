use super::*;

pub(crate) fn one<T: DeserializeOwned>(db: &Connection, sql: &str, id: &str) -> Result<Option<T>> {
    db.query_row(sql, [id], |r| r.get::<_, String>(0))
        .optional()
        .map_err(error)?
        .map(|s| serde_json::from_str(&s).map_err(error))
        .transpose()
}
pub(crate) fn many<T: DeserializeOwned>(db: &Connection, sql: &str, id: &str) -> Result<Vec<T>> {
    db.prepare(sql)
        .map_err(error)?
        .query_map([id], |r| r.get::<_, String>(0))
        .map_err(error)?
        .map(|r| serde_json::from_str(&r.map_err(error)?).map_err(error))
        .collect()
}

pub(crate) fn index_history(db: &Connection, h: &History) -> Result<()> {
    let anchor = action_checkpoint(h);
    let snapshot: Option<Snapshot> = match anchor {
        Some(id) => one(db, "SELECT body FROM snapshots WHERE id=?1", id)?,
        None => None,
    };
    let time = if h.kind == HistoryKind::ExistingBackup {
        snapshot
            .as_ref()
            .map_or(h.recorded_at, |s| s.selection_time)
    } else {
        h.recorded_at
    };
    let marker = matches!(h.kind, HistoryKind::GameStarted | HistoryKind::GameClosed);
    let visible = snapshot.as_ref().is_some_and(|s| s.removed_at.is_none());
    db.execute("INSERT INTO history_index(id,game_id,sequence,anchor_id,anchor_time,observation_run,marker,visible) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET anchor_id=excluded.anchor_id,anchor_time=excluded.anchor_time,observation_run=excluded.observation_run,marker=excluded.marker,visible=excluded.visible",
        params![h.id,h.game_id,h.sequence,anchor,time,h.observation_run,marker,visible]).map_err(error)?;
    Ok(())
}

pub(crate) fn markers(db: &Connection, game: &str) -> Result<()> {
    let mut after = 0_u64;
    loop {
        let batch = db.prepare("SELECT h.body FROM history_index i JOIN history h ON h.id=i.id WHERE i.game_id=?1 AND i.marker=1 AND i.sequence>?2 ORDER BY i.sequence LIMIT 200").map_err(error)?
            .query_map(params![game,after], |r| r.get::<_, String>(0)).map_err(error)?
            .map(|r| serde_json::from_str::<History>(&r.map_err(error)?).map_err(error)).collect::<Result<Vec<_>>>()?;
        if batch.is_empty() {
            break;
        }
        for h in batch {
            after = h.sequence;
            let neighbour = |sign: &str, order: &str| -> Result<Option<History>> {
                let sql = format!(
                    "SELECT h.body FROM history_index i JOIN history h ON h.id=i.id WHERE i.game_id=?1 AND i.marker=1 AND i.sequence{sign}?2 ORDER BY i.sequence {order} LIMIT 1"
                );
                db.query_row(&sql, params![game, h.sequence], |r| r.get::<_, String>(0))
                    .optional()
                    .map_err(error)?
                    .map(|s| serde_json::from_str(&s).map_err(error))
                    .transpose()
            };
            let previous = neighbour("<", "DESC")?;
            let next = neighbour(">", "ASC")?;
            let visible = if let Some(window) = marker_window(&h, previous.as_ref(), next.as_ref())
            {
                let end = window.end.unwrap_or((i64::MAX as u64, i64::MAX as u64));
                let comparison = if window.inclusive { "<=" } else { "<" };
                let sql = format!(
                    "SELECT EXISTS(SELECT 1 FROM history_index WHERE game_id=?1 AND marker=0 AND visible=1 AND (anchor_time,sequence)>=(?2,?3) AND (anchor_time,sequence){comparison}(?4,?5) AND (?6 IS NULL OR observation_run=?6))"
                );
                db.query_row(
                    &sql,
                    params![
                        game,
                        window.start.0,
                        window.start.1,
                        end.0,
                        end.1,
                        window.observation_run
                    ],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(error)?
            } else {
                false
            };
            db.execute(
                "UPDATE history_index SET visible=?2 WHERE id=?1 AND visible<>?2",
                params![h.id, visible],
            )
            .map_err(error)?;
        }
    }
    Ok(())
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let tx = db.unchecked_transaction().map_err(error)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS history_index(id TEXT PRIMARY KEY REFERENCES history(id) ON DELETE CASCADE, game_id TEXT NOT NULL,sequence INTEGER NOT NULL,anchor_id TEXT,anchor_time INTEGER NOT NULL,observation_run TEXT NOT NULL,marker INTEGER NOT NULL,visible INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS visible_history_page ON history_index(game_id,visible,sequence);
        CREATE INDEX IF NOT EXISTS history_markers ON history_index(game_id,marker,sequence);
        CREATE INDEX IF NOT EXISTS history_anchor ON history_index(anchor_id);
        CREATE INDEX IF NOT EXISTS history_intervals ON history_index(game_id,marker,visible,anchor_time,sequence);
        CREATE TABLE IF NOT EXISTS history_revisions(game_id TEXT PRIMARY KEY,revision INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS retained_games(game_id TEXT PRIMARY KEY);
        INSERT OR IGNORE INTO retained_games SELECT DISTINCT game_id FROM operations WHERE json_extract(body,'$.status') IN ('failed','resolved','recovery_needed');
        CREATE INDEX IF NOT EXISTS snapshots_active ON snapshots(game_id,json_extract(body,'$.removed_at'));
        CREATE INDEX IF NOT EXISTS snapshot_paths ON snapshots(json_extract(body,'$.path'),json_extract(body,'$.removed_at'));
        CREATE INDEX IF NOT EXISTS operations_status ON operations(json_extract(body,'$.status'));
        CREATE INDEX IF NOT EXISTS operations_recent ON operations(game_id,json_extract(body,'$.started_at') DESC,id DESC);
        CREATE INDEX IF NOT EXISTS operations_game_page ON operations(game_id,id);
        CREATE INDEX IF NOT EXISTS operations_unpublished ON operations(game_id,json_extract(body,'$.snapshot_path'),json_extract(body,'$.staging_identity'));
        CREATE INDEX IF NOT EXISTS operations_reserved ON operations(game_id) WHERE json_extract(body,'$.status')<>'completed' AND json_extract(body,'$.snapshot_path') IS NOT NULL;
        CREATE INDEX IF NOT EXISTS operation_path_reservations ON operations(json_extract(body,'$.snapshot_path')) WHERE json_extract(body,'$.status')<>'completed';
        CREATE INDEX IF NOT EXISTS snapshot_registration ON snapshots(json_extract(body,'$.registration_order'));
        INSERT OR IGNORE INTO metadata VALUES('history_sequence',(SELECT COALESCE(MAX(sequence),0) FROM history));
        INSERT OR IGNORE INTO metadata VALUES('snapshot_order',(SELECT COALESCE(MAX(json_extract(body,'$.registration_order')),0) FROM snapshots));
        PRAGMA user_version=3;").map_err(error)?;
    let mut after = 0_u64;
    loop {
        let batch = tx
            .prepare("SELECT body FROM history WHERE sequence>?1 ORDER BY sequence LIMIT 200")
            .map_err(error)?
            .query_map([after], |r| r.get::<_, String>(0))
            .map_err(error)?
            .map(|r| serde_json::from_str::<History>(&r.map_err(error)?).map_err(error))
            .collect::<Result<Vec<_>>>()?;
        if batch.is_empty() {
            break;
        }
        for h in batch {
            after = h.sequence;
            index_history(&tx, &h)?;
        }
    }
    for g in rows::<Game>(&tx, "SELECT body FROM games")? {
        markers(&tx, &g.id)?;
    }
    tx.commit().map_err(error)
}
