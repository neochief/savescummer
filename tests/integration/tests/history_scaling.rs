use savescummer_core::*;
use savescummer_platform::{Paths, SystemClock};
use savescummer_snapshots::FileSnapshots;
use savescummer_storage::SqliteRepository;
use std::{fs, sync::Arc, time::Instant};

struct Fixture {
    root: tempfile::TempDir,
    repo: Arc<SqliteRepository>,
    runtime: Runtime,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = Arc::new(SqliteRepository::open(&root.path().join("db")).unwrap());
        let runtime = Self::open(repo.clone());
        for id in ["a", "b"] {
            let live = root.path().join(id);
            fs::create_dir(&live).unwrap();
            fs::write(live.join("save"), b"data").unwrap();
            runtime
                .configure(id.into(), id.into(), live, vec![])
                .unwrap();
        }
        Self {
            root,
            repo,
            runtime,
        }
    }
    fn open(repo: Arc<SqliteRepository>) -> Runtime {
        Runtime::open(
            repo,
            Arc::new(FileSnapshots::default()),
            Arc::new(Paths::new(vec![])),
            Arc::new(SystemClock),
        )
        .unwrap()
    }
    fn save(&self, game: &str) -> Id {
        let op = self.runtime.accept(game, Action::Save, new_id()).unwrap();
        self.runtime.execute(&op).unwrap();
        op
    }
    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("db")).unwrap()
    }
    fn populate(&self, count: usize, spread: bool) {
        self.save("a");
        self.save("b");
        let state = self.runtime.state().unwrap();
        let mut sequence = state.history.iter().map(|h| h.sequence).max().unwrap();
        let mut order = state
            .snapshots
            .values()
            .map(|s| s.registration_order)
            .max()
            .unwrap();
        for start in (0..count).step_by(200) {
            let mut changes = MetadataChanges::default();
            for i in start..(start + 200).min(count) {
                let game = if spread && i % 2 == 1 { "b" } else { "a" };
                let mut row = state
                    .history
                    .iter()
                    .find(|h| h.game_id == game)
                    .unwrap()
                    .clone();
                sequence += 1;
                row.id = format!("history-{i:08}");
                row.sequence = sequence;
                changes.history.push(row);
                let mut op = state
                    .operations
                    .values()
                    .find(|o| o.game_id == game)
                    .unwrap()
                    .clone();
                op.id = format!("operation-{i:08}");
                op.request_id = format!("request-{i:08}");
                op.started_at = 0;
                changes.operations.push(op);
                let mut retired = state
                    .snapshots
                    .values()
                    .find(|s| s.game_id == game)
                    .unwrap()
                    .clone();
                order += 1;
                retired.id = format!("retired-{i:08}");
                retired.registration_order = order;
                retired.removed_at = Some(1);
                retired.available = false;
                changes.snapshots.push(retired);
            }
            changes.history_sequence = sequence;
            changes.snapshot_order = order;
            self.repo.commit_changes(&changes).unwrap();
        }
    }
}

#[test]
fn writes_touch_only_changed_rows_and_failed_batches_are_atomic() {
    let f = Fixture::new();
    let a = f.save("a");
    let b = f.save("b");
    let db = f.db();
    db.execute_batch("CREATE TABLE writes(entity TEXT,id TEXT,game_id TEXT);
        CREATE TRIGGER games_write AFTER UPDATE ON games BEGIN INSERT INTO writes VALUES('games',new.id,new.id); END;
        CREATE TRIGGER snapshots_write AFTER UPDATE ON snapshots BEGIN INSERT INTO writes VALUES('snapshots',new.id,new.game_id); END;
        CREATE TRIGGER history_write AFTER UPDATE ON history BEGIN INSERT INTO writes VALUES('history',new.id,new.game_id); END;
        CREATE TRIGGER operations_write AFTER UPDATE ON operations BEGIN INSERT INTO writes VALUES('operations',new.id,new.game_id); END;
        CREATE TRIGGER operations_insert AFTER INSERT ON operations BEGIN INSERT INTO writes VALUES('operations',new.id,new.game_id); END;
        CREATE TRIGGER snapshots_insert AFTER INSERT ON snapshots BEGIN INSERT INTO writes VALUES('snapshots',new.id,new.game_id); END;
        CREATE TRIGGER history_insert AFTER INSERT ON history BEGIN INSERT INTO writes VALUES('history',new.id,new.game_id); END;").unwrap();
    f.runtime.set_play_sounds(false).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM writes", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        0
    );
    f.save("a");
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM writes WHERE game_id='b' OR id IN (?1,?2)",
            [&a, &b],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    let before = f.repo.load().unwrap();
    db.execute_batch("CREATE TRIGGER fail_operation BEFORE INSERT ON operations WHEN new.id='reject' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let mut game = before.games["a"].clone();
    game.name = "must roll back".into();
    let mut op = before.operations[&a].clone();
    op.id = "reject".into();
    op.request_id = "reject".into();
    assert!(
        f.repo
            .commit_changes(&MetadataChanges {
                games: vec![game],
                operations: vec![op],
                revision: before.revision + 1,
                ..Default::default()
            })
            .is_err()
    );
    let after = f.repo.load().unwrap();
    assert_eq!(before.games, after.games);
    assert_eq!(before.operations, after.operations);
    assert_eq!(before.revision, after.revision);
}

#[test]
fn cursors_are_game_and_host_bound_and_ignore_unrelated_activity() {
    let f = Fixture::new();
    for _ in 0..3 {
        f.save("a");
    }
    let page = f.runtime.history_page("a", None, 1).unwrap();
    let cursor = page.next_cursor.unwrap();
    f.save("b");
    f.runtime.set_play_sounds(false).unwrap();
    let next = f.runtime.history_page("a", Some(&cursor), 1).unwrap();
    assert_ne!(next.rows[0].entry.id, page.rows[0].entry.id);
    assert_eq!(
        f.runtime
            .history_page("b", Some(&cursor), 1)
            .unwrap_err()
            .code,
        ErrorCode::CursorExpired
    );
    let reopened = Fixture::open(f.repo.clone());
    assert_eq!(
        reopened
            .history_page("a", Some(&cursor), 1)
            .unwrap_err()
            .code,
        ErrorCode::CursorExpired
    );
    f.save("a");
    assert_eq!(
        f.runtime
            .history_page("a", Some(&cursor), 1)
            .unwrap_err()
            .code,
        ErrorCode::CursorExpired
    );
    let anchored = f
        .runtime
        .history_page_at("a", None, 1, Some(&next.rows[0].entry.id))
        .unwrap();
    assert_eq!(anchored.rows[0].entry.id, next.rows[0].entry.id);
    assert_eq!(
        f.runtime.history_page("a", None, 201).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn lazy_game_reads_preserve_other_games_checkpoint_ownership_and_path_reservations() {
    let f = Fixture::new();
    f.save("a");
    let checkpoint = f
        .runtime
        .state()
        .unwrap()
        .snapshots
        .values()
        .find(|s| s.game_id == "a")
        .unwrap()
        .clone();
    let new_a = f.root.path().join("moved-a");
    fs::create_dir(&new_a).unwrap();
    f.runtime
        .configure("a".into(), "A".into(), new_a, vec![])
        .unwrap();
    f.runtime
        .configure("b".into(), "B".into(), f.root.path().join("a"), vec![])
        .unwrap();
    f.runtime.refresh("b").unwrap();
    assert!(
        f.runtime
            .history_page("b", None, 50)
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        f.runtime
            .accept("b", Action::Load { target: None }, new_id())
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    fs::remove_dir_all(&checkpoint.path).unwrap();
    let op = f.save("b");
    assert_ne!(
        f.runtime.operation(&op).unwrap().snapshot_path.as_ref(),
        Some(&checkpoint.path)
    );
    assert_eq!(
        f.repo.snapshot(&checkpoint.id).unwrap().unwrap().game_id,
        "a"
    );
}

#[test]
fn audit_larger_than_frame_limit_keeps_boot_summary_and_pages_bounded() {
    let f = Fixture::new();
    f.populate(10_000, true);
    let db = f.db();
    let bytes:u64=db.query_row("SELECT (SELECT sum(length(body)) FROM history)+(SELECT sum(length(body)) FROM operations)+(SELECT sum(length(body)) FROM snapshots)",[],|r|r.get(0)).unwrap();
    assert!(bytes > 8 * 1024 * 1024);
    let boot = f.repo.boot().unwrap();
    assert!(boot.history.is_empty() && boot.snapshots.is_empty());
    assert!(boot.operations.len() <= 2);
    let rt = Fixture::open(f.repo.clone());
    let summary: LibraryState = rt.summary().unwrap().into();
    let wire = serde_json::to_value(&summary).unwrap();
    assert!(wire.get("history").is_none() && wire.get("visible_history").is_none());
    assert!(serde_json::to_vec(&summary).unwrap().len() < 32 * 1024);
    assert_eq!(
        rt.operation_for_request("request-00000000").unwrap(),
        Some("operation-00000000".into())
    );
    assert_eq!(
        rt.operation("operation-00000000").unwrap().status,
        OperationStatus::Completed
    );
    let mut cursor = None;
    let mut ids = std::collections::BTreeSet::new();
    loop {
        let page = rt.history_page("a", cursor.as_deref(), 200).unwrap();
        assert!(page.rows.len() <= 200);
        assert!(serde_json::to_vec(&page).unwrap().len() < 512 * 1024 + 1024);
        for row in page.rows {
            assert!(ids.insert(row.entry.id));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 5001);
    let op = rt.accept("a", Action::Save, new_id()).unwrap();
    rt.execute(&op).unwrap();
    assert!(
        rt.history_page("a", None, 1).unwrap().rows[0]
            .entry
            .sequence
            > 10_000
    );
}

#[test]
#[ignore = "recorded capacity benchmark; run explicitly outside correctness checks"]
fn capacity_measurements() {
    for count in [100, 10_000, 100_000] {
        for spread in [false, true] {
            let f = Fixture::new();
            f.populate(count, spread);
            let start = Instant::now();
            let rt = Fixture::open(f.repo.clone());
            let startup = start.elapsed();
            let start = Instant::now();
            let summary: LibraryState = rt.summary().unwrap().into();
            let summary_time = start.elapsed();
            let start = Instant::now();
            let page = rt.history_page("a", None, 50).unwrap();
            let first = start.elapsed();
            let start = Instant::now();
            rt.history_page("a", page.next_cursor.as_deref(), 50)
                .unwrap();
            let next = start.elapsed();
            let start = Instant::now();
            let id = rt.accept("b", Action::Save, new_id()).unwrap();
            rt.execute(&id).unwrap();
            let save = start.elapsed();
            eprintln!(
                "rows={count} spread={spread} startup={startup:?} summary={summary_time:?} first_page={first:?} next_page={next:?} save={save:?} summary_bytes={}",
                serde_json::to_vec(&summary).unwrap().len()
            );
        }
    }
}
