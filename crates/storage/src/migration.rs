use super::{error, json, rows};
use rusqlite::{Connection, params};
use savescummer_core::*;
use serde_json::Value;
use std::collections::BTreeMap;

fn text<'a>(record: &'a Value, field: &str) -> Result<&'a str> {
    record[field]
        .as_str()
        .ok_or_else(|| error(format!("v1 migration: missing {field}")))
}

/// Convert only metadata, in one transaction. Never inspect or modify save files:
/// offline volumes and interrupted operations must remain migratable.
pub(super) fn checkpoint_ownership(connection: &Connection) -> Result<()> {
    let tx = connection.unchecked_transaction().map_err(error)?;
    let mut games = rows::<Value>(&tx, "SELECT body FROM games")?;
    let mut snapshots = rows::<Value>(&tx, "SELECT body FROM snapshots")?;
    let mut history = rows::<Value>(&tx, "SELECT body FROM history ORDER BY sequence")?;
    let mut operations = rows::<Value>(&tx, "SELECT body FROM operations")?;
    let mut origins = BTreeMap::new();
    for (records, game_field, path_field) in
        [(&games, "id", "data_dir"), (&operations, "game_id", "live")]
    {
        for record in records {
            let key = (
                text(record, game_field)?.to_owned(),
                text(record, "location_id")?.to_owned(),
            );
            let path = record[path_field].clone();
            if !path.is_string() {
                return Err(error("v1 migration: missing original data path"));
            }
            if let Some(previous) = origins.insert(key, path.clone())
                && previous != path
            {
                return Err(error("v1 migration: conflicting original data paths"));
            }
        }
    }
    let mut order = BTreeMap::new();
    let mut last_order = 0;
    for entry in &history {
        let sequence = entry["sequence"]
            .as_u64()
            .ok_or_else(|| error("v1 migration: missing history sequence"))?;
        last_order = last_order.max(sequence);
        let field = match entry["kind"].as_str() {
            Some("saved" | "existing_backup") => "snapshot_id",
            Some("loaded" | "reverted") => "recovery_id",
            _ => continue,
        };
        if let Some(id) = entry[field].as_str() {
            order.entry(id.to_owned()).or_insert(sequence);
        }
    }
    snapshots.sort_by_key(|s| {
        (
            s["discovered_at"].as_u64().unwrap_or(0),
            s["id"].as_str().unwrap_or("").to_owned(),
        )
    });
    for snapshot in &mut snapshots {
        let key = (
            text(snapshot, "game_id")?.to_owned(),
            text(snapshot, "location_id")?.to_owned(),
        );
        // A manual-only former location may have no surviving path mapping in v1.
        // Preserve that generation and let normal discovery associate it safely.
        snapshot["original_data_dir"] = origins.get(&key).cloned().unwrap_or(Value::Null);
        let registration_order = if let Some(sequence) = order.get(text(snapshot, "id")?) {
            *sequence
        } else {
            last_order = last_order
                .checked_add(1)
                .ok_or_else(|| error("checkpoint order overflow"))?;
            last_order
        };
        snapshot["registration_order"] = registration_order.into();
    }
    for operation in &mut operations {
        if matches!(
            operation["action"]["type"].as_str(),
            Some("load" | "revert")
        ) && !operation["action"]["target"].is_null()
        {
            let source = operation["source_id"]
                .as_str()
                .ok_or_else(|| error("v1 migration: missing accepted source checkpoint"))?
                .to_owned();
            operation["action"]["target"] = source.into();
        }
    }
    for (table, records) in [
        ("games", &mut games),
        ("snapshots", &mut snapshots),
        ("history", &mut history),
        ("operations", &mut operations),
    ] {
        for record in records {
            record
                .as_object_mut()
                .ok_or_else(|| error("invalid v1 record"))?
                .remove("location_id");
            // Validate the new shape before any migration can commit.
            match table {
                "games" => {
                    serde_json::from_value::<Game>(record.clone()).map_err(error)?;
                }
                "snapshots" => {
                    serde_json::from_value::<Snapshot>(record.clone()).map_err(error)?;
                }
                "history" => {
                    serde_json::from_value::<History>(record.clone()).map_err(error)?;
                }
                "operations" => {
                    serde_json::from_value::<Operation>(record.clone()).map_err(error)?;
                }
                _ => unreachable!(),
            }
            tx.execute(
                &format!("UPDATE {table} SET body = ?1 WHERE id = ?2"),
                params![json(record)?, text(record, "id")?],
            )
            .map_err(error)?;
        }
    }
    tx.pragma_update(None, "user_version", 2).map_err(error)?;
    tx.commit().map_err(error)
}
