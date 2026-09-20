use savescummer_core::*;
use savescummer_platform::Paths;
use savescummer_snapshots::FileSnapshots;
use savescummer_storage::SqliteRepository;
use std::{fs, path::PathBuf, sync::Arc};

struct FixedClock;
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        1_000
    }
}

struct Fixture {
    temp: tempfile::TempDir,
    live: PathBuf,
    other: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let live = temp.path().join("Game");
        let other = temp.path().join("Other");
        for path in [&live, &other] {
            fs::create_dir(path).unwrap();
            fs::write(path.join("save"), b"A").unwrap();
        }
        Self { temp, live, other }
    }
    fn db(&self) -> PathBuf {
        self.temp.path().join("runtime.db")
    }
    fn open(&self) -> Runtime {
        Runtime::open(
            Arc::new(SqliteRepository::open(&self.db()).unwrap()),
            Arc::new(FileSnapshots::default()),
            Arc::new(Paths::new(vec![])),
            Arc::new(FixedClock),
        )
        .unwrap()
    }
    fn configure(&self, rt: &Runtime, other: bool) {
        rt.configure(
            "game".into(),
            "Game".into(),
            if other {
                self.other.clone()
            } else {
                self.live.clone()
            },
            vec![],
        )
        .unwrap();
    }
    fn start(&self) -> Runtime {
        let rt = self.open();
        self.configure(&rt, false);
        rt
    }
    fn write(&self, bytes: &[u8]) {
        fs::write(self.live.join("save"), bytes).unwrap();
    }
    fn data(&self) -> Vec<u8> {
        fs::read(self.live.join("save")).unwrap()
    }
}
fn run(rt: &Runtime, action: Action) -> Operation {
    let id = rt.accept("game", action, new_id()).unwrap();
    rt.execute(&id).unwrap();
    rt.operation(&id).unwrap()
}
fn last(rt: &Runtime) -> History {
    rt.state().unwrap().history.last().unwrap().clone()
}

#[test]
fn returning_to_a_directory_restores_saved_and_recovery_checkpoint_eligibility() {
    let f = Fixture::new();
    let rt = f.start();
    run(&rt, Action::Save);
    let saved = last(&rt).snapshot_id.unwrap();
    f.write(b"B");
    run(
        &rt,
        Action::Load {
            target: Some(saved.clone()),
        },
    );
    let recovery = last(&rt).recovery_id.unwrap();
    f.configure(&rt, true);
    let state = rt.state().unwrap();
    assert!(!state.snapshots[&saved].available);
    assert!(!state.snapshots[&recovery].available);
    for action in [
        Action::Load {
            target: Some(saved.clone()),
        },
        Action::Revert {
            target: recovery.clone(),
        },
    ] {
        assert_eq!(
            rt.accept("game", action, new_id()).unwrap_err().code,
            ErrorCode::InvalidTarget
        );
    }
    assert_eq!(
        rt.accept("game", Action::Load { target: None }, new_id())
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    assert_eq!(rt.state().unwrap().operations.len(), state.operations.len());
    drop(rt);
    let rt = f.open();
    f.configure(&rt, false);
    assert!(rt.state().unwrap().snapshots[&saved].available);
    assert!(rt.state().unwrap().snapshots[&recovery].available);
    run(&rt, Action::Revert { target: recovery });
    assert_eq!(f.data(), b"B");
    run(&rt, Action::Load { target: None });
    assert_eq!(f.data(), b"A");
    assert_eq!(last(&rt).snapshot_id, Some(saved));
}

#[test]
fn checkpoint_selection_and_ties_survive_missing_history_and_restart() {
    let f = Fixture::new();
    let rt = f.start();
    run(&rt, Action::Save);
    let a = last(&rt).snapshot_id.unwrap();
    f.write(b"B");
    run(&rt, Action::Save);
    let b = last(&rt).snapshot_id.unwrap();
    let mut state = rt.state().unwrap();
    assert_eq!(
        state.snapshots[&a].selection_time,
        state.snapshots[&b].selection_time
    );
    assert!(state.snapshots[&b].registration_order > state.snapshots[&a].registration_order);
    // Make timeline ordering disagree with checkpoint registration ordering.
    state.history[0].sequence = 20;
    state.history[1].sequence = 10;
    drop(rt);
    SqliteRepository::open(&f.db())
        .unwrap()
        .commit(&state)
        .unwrap();
    let rt = f.open();
    run(&rt, Action::Load { target: None });
    assert_eq!(f.data(), b"B");
    let recovery = last(&rt).recovery_id.unwrap();
    let mut state = rt.state().unwrap();
    state.history.clear();
    drop(rt);
    SqliteRepository::open(&f.db())
        .unwrap()
        .commit(&state)
        .unwrap();
    let rt = f.open();
    f.write(b"C");
    let load = run(&rt, Action::Load { target: None });
    assert_eq!(load.source_id, Some(b));
    assert_eq!(
        load.target_id, None,
        "missing audit row does not prevent restore"
    );
    assert_eq!(f.data(), b"B");
    run(&rt, Action::Load { target: Some(a) });
    assert_eq!(f.data(), b"A");
    run(&rt, Action::Revert { target: recovery });
    assert_eq!(f.data(), b"B");
    run(&rt, Action::Save);
    let newest = last(&rt).snapshot_id.unwrap();
    assert!(
        rt.state().unwrap().snapshots[&newest].registration_order
            > state
                .snapshots
                .values()
                .map(|s| s.registration_order)
                .max()
                .unwrap()
    );
}

#[test]
fn invalid_checkpoint_targets_are_rejected_before_capturing_recovery() {
    let f = Fixture::new();
    let rt = f.start();
    run(&rt, Action::Save);
    let saved = last(&rt);
    run(&rt, Action::Load { target: None });
    let recovery = last(&rt).recovery_id.unwrap();
    rt.configure("other".into(), "Other".into(), f.other.clone(), vec![])
        .unwrap();
    let id = rt.accept("other", Action::Save, new_id()).unwrap();
    rt.execute(&id).unwrap();
    let foreign = last(&rt).snapshot_id.unwrap();
    let before = rt.state().unwrap();
    for action in [
        Action::Load {
            target: Some(new_id()),
        },
        Action::Load {
            target: Some(saved.id),
        },
        Action::Load {
            target: Some(foreign),
        },
        Action::Load {
            target: Some(recovery),
        },
        Action::Revert {
            target: saved.snapshot_id.unwrap(),
        },
    ] {
        assert_eq!(
            rt.accept("game", action, new_id()).unwrap_err().code,
            ErrorCode::InvalidTarget
        );
    }
    let after = rt.state().unwrap();
    assert_eq!(before.operations.len(), after.operations.len());
    assert_eq!(before.snapshots.len(), after.snapshots.len());
    assert_eq!(f.data(), b"A");
}

#[test]
fn directory_comparison_follows_filesystem_case_rules_and_aliases() {
    let f = Fixture::new();
    let paths = Paths::new(vec![]);
    let lower = f.live.with_file_name("game");
    // Observe the volume's behavior, including case-sensitive Windows volumes.
    if lower.exists() {
        assert!(paths.same_location(&f.live, &lower));
    } else {
        fs::create_dir(&lower).unwrap();
        assert!(!paths.same_location(&f.live, &lower));
    }
    assert!(paths.same_location(&f.live, &f.live.join(".")));
    assert!(!paths.same_location(&f.live, &f.other));
    #[cfg(unix)]
    {
        let alias = f.temp.path().join("alias");
        std::os::unix::fs::symlink(&f.live, &alias).unwrap();
        assert!(paths.same_location(&f.live, &alias));
    }
}

// Write the previous schema exactly as it existed on disk, including history-ID
// command targets. Tests below exercise the real migration, not a mock repository.
fn write_v1(f: &Fixture, state: &State) {
    SqliteRepository::open(&f.db())
        .unwrap()
        .commit(state)
        .unwrap();
    let db = rusqlite::Connection::open(f.db()).unwrap();
    let tx = db.unchecked_transaction().unwrap();
    for table in ["games", "snapshots", "history", "operations"] {
        let records: Vec<String> = tx
            .prepare(&format!("SELECT body FROM {table}"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for body in records {
            let mut record: serde_json::Value = serde_json::from_str(&body).unwrap();
            let path = match table {
                "games" => record["data_dir"].clone(),
                "snapshots" => record["original_data_dir"].clone(),
                "operations" => record["live"].clone(),
                "history" => serde_json::json!(f.live),
                _ => unreachable!(),
            };
            record["location_id"] = format!("old-location:{}", path.as_str().unwrap()).into();
            record.as_object_mut().unwrap().remove("original_data_dir");
            record.as_object_mut().unwrap().remove("registration_order");
            if table == "operations"
                && matches!(record["action"]["type"].as_str(), Some("load" | "revert"))
                && !record["action"]["target"].is_null()
            {
                record["action"]["target"] = record["target_id"].clone();
            }
            tx.execute(
                &format!("UPDATE {table} SET body = ?1 WHERE id = ?2"),
                rusqlite::params![record.to_string(), record["id"].as_str().unwrap()],
            )
            .unwrap();
        }
    }
    tx.pragma_update(None, "user_version", 1).unwrap();
    tx.commit().unwrap();
}

#[test]
fn migration_preserves_checkpoint_ids_old_directories_audit_and_idempotency() {
    let f = Fixture::new();
    let rt = f.start();
    run(&rt, Action::Save);
    let saved = last(&rt).snapshot_id.unwrap();
    f.write(b"B");
    let load = run(
        &rt,
        Action::Load {
            target: Some(saved.clone()),
        },
    );
    let recovery = last(&rt).recovery_id.unwrap();
    let revert = run(
        &rt,
        Action::Revert {
            target: recovery.clone(),
        },
    );
    f.configure(&rt, true);
    let before = rt.state().unwrap();
    drop(rt);
    write_v1(&f, &before);
    let repository = SqliteRepository::open(&f.db()).unwrap();
    let migrated = repository.load().unwrap();
    assert_eq!(migrated.revision, before.revision);
    assert_eq!(
        serde_json::to_value(&migrated.history).unwrap(),
        serde_json::to_value(&before.history).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&migrated.operations).unwrap(),
        serde_json::to_value(&before.operations).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&migrated.snapshots).unwrap(),
        serde_json::to_value(&before.snapshots).unwrap()
    );
    drop(repository);
    let rt = f.open();
    assert!(!rt.state().unwrap().snapshots[&saved].available);
    f.configure(&rt, false);
    assert_eq!(
        rt.accept("game", load.action.clone(), load.request_id)
            .unwrap(),
        load.id
    );
    assert_eq!(
        rt.accept("game", revert.action.clone(), revert.request_id)
            .unwrap(),
        revert.id
    );
    run(
        &rt,
        Action::Load {
            target: Some(saved),
        },
    );
    assert_eq!(f.data(), b"A");
    run(&rt, Action::Revert { target: recovery });
    assert_eq!(f.data(), b"B");
}

#[test]
fn migration_preserves_unmapped_manual_checkpoint_until_verified_rediscovery() {
    let f = Fixture::new();
    let rt = f.start();
    let manual = f.live.with_file_name("Game - Copy");
    FileSnapshots::default()
        .copy(&f.live, &manual, &mut |_| {})
        .unwrap();
    rt.refresh("game").unwrap();
    let original = last(&rt);
    let checkpoint = original.snapshot_id.clone().unwrap();
    f.configure(&rt, true);
    let before = rt.state().unwrap();
    assert!(before.operations.is_empty());
    drop(rt);
    write_v1(&f, &before);
    let rt = f.open();
    let state = rt.state().unwrap();
    assert_eq!(state.snapshots[&checkpoint].original_data_dir, None);
    assert!(!state.snapshots[&checkpoint].available);
    assert_eq!(
        rt.accept(
            "game",
            Action::Load {
                target: Some(checkpoint.clone())
            },
            new_id()
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidTarget
    );
    f.configure(&rt, false);
    rt.refresh("game").unwrap();
    let state = rt.state().unwrap();
    assert_eq!(state.snapshots.len(), 1);
    assert_eq!(state.history.len(), 1);
    assert_eq!(state.history[0].id, original.id);
    assert_eq!(
        state.snapshots[&checkpoint].original_data_dir.as_ref(),
        Some(&state.games["game"].data_dir)
    );
    assert!(state.snapshots[&checkpoint].available);
    f.write(b"changed");
    run(
        &rt,
        Action::Load {
            target: Some(checkpoint),
        },
    );
    assert_eq!(f.data(), b"A");
}

#[test]
fn interrupted_v1_restore_retains_its_journal_and_recovery_choice() {
    let f = Fixture::new();
    let rt = f.start();
    run(&rt, Action::Save);
    let saved = last(&rt).snapshot_id.unwrap();
    f.write(b"before load");
    let load = run(
        &rt,
        Action::Load {
            target: Some(saved),
        },
    );
    let mut state = rt.state().unwrap();
    let loaded = state.history.pop().unwrap();
    state.snapshots.remove(&loaded.recovery_id.unwrap());
    let pending = state.operations.get_mut(&load.id).unwrap();
    pending.status = OperationStatus::Pending;
    pending.phase = Phase::ReplacementInstalled;
    // Model the durable state before the completion transaction was committed.
    drop(rt);
    write_v1(&f, &state);
    f.write(b"progress after crash");
    let rt = f.open();
    let interrupted = rt.operation(&load.id).unwrap();
    assert_eq!(interrupted.status, OperationStatus::RecoveryNeeded);
    assert_eq!(interrupted.live, load.live);
    assert_eq!(interrupted.recovery, load.recovery);
    assert_eq!(interrupted.source_id, load.source_id);
    assert_eq!(interrupted.source_fingerprint, load.source_fingerprint);
    assert_eq!(interrupted.target_id, load.target_id);
    assert_eq!(f.data(), b"progress after crash");
    run(
        &rt,
        Action::Recover {
            operation: load.id.clone(),
            choice: RecoveryChoice::RestoreBefore,
        },
    );
    assert_eq!(f.data(), b"before load");
    assert_eq!(
        rt.operation(&load.id).unwrap().status,
        OperationStatus::Resolved
    );
    assert_eq!(
        rt.state().unwrap().history.len(),
        1,
        "recovery must not invent a successful Load"
    );
}

#[test]
fn invalid_v1_migration_rolls_back_metadata_atomically() {
    let f = Fixture::new();
    let rt = f.start();
    run(&rt, Action::Save);
    let state = rt.state().unwrap();
    drop(rt);
    write_v1(&f, &state);
    let db = rusqlite::Connection::open(f.db()).unwrap();
    // Force failure late in the migration, after it has updated game/snapshot rows.
    db.execute(
        "UPDATE operations SET body = json_remove(body, '$.phase')",
        [],
    )
    .unwrap();
    let before: String = db
        .query_row("SELECT body FROM games", [], |r| r.get(0))
        .unwrap();
    assert!(SqliteRepository::open(&f.db()).is_err());
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let after: String = db
        .query_row("SELECT body FROM games", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(before, after);
}
