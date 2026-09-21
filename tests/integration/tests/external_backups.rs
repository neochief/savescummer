use savescummer_core::*;
use savescummer_platform::Paths;
use savescummer_snapshots::FileSnapshots;
use savescummer_storage::SqliteRepository;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

#[derive(Clone)]
struct TestClock(Arc<AtomicU64>);
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
struct Fixture {
    root: tempfile::TempDir,
    live: PathBuf,
    clock: TestClock,
    rt: Runtime,
}
impl Fixture {
    fn state(&self) -> State {
        let state = self.rt.state().unwrap();
        let mut actual = vec![];
        for game in state.games.keys() {
            let mut cursor = None;
            loop {
                let page = self.rt.history_page(game, cursor.as_deref(), 3).unwrap();
                actual.extend(page.rows.into_iter().map(|row| row.entry));
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
        }
        actual.sort_by_key(|h| std::cmp::Reverse(h.sequence));
        let mut expected = state.visible_history.clone();
        expected.sort_by_key(|h| std::cmp::Reverse(h.sequence));
        assert_eq!(
            actual, expected,
            "indexed pages must preserve the full core visibility policy"
        );
        state
    }
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("Game");
        fs::create_dir_all(live.join("nested")).unwrap();
        fs::write(live.join("nested/save"), b"original").unwrap();
        let clock = TestClock(Arc::new(AtomicU64::new(1_000)));
        let rt = Self::open(root.path(), clock.clone());
        rt.configure("game".into(), "Game".into(), live.clone(), vec![])
            .unwrap();
        Self {
            root,
            live,
            clock,
            rt,
        }
    }
    fn open(root: &Path, clock: TestClock) -> Runtime {
        Runtime::open(
            Arc::new(SqliteRepository::open(&root.join("db")).unwrap()),
            Arc::new(FileSnapshots::default()),
            Arc::new(Paths::new(vec![])),
            Arc::new(clock),
        )
        .unwrap()
    }
    fn time(&self, time: u64) {
        self.clock.0.store(time, Ordering::SeqCst);
    }
    fn run(&self, action: Action) {
        let id = self.rt.accept("game", action, new_id()).unwrap();
        self.rt.execute(&id).unwrap();
    }
    fn last(&self) -> History {
        self.state().history.last().unwrap().clone()
    }
    fn snapshot(&self, entry: &History) -> Snapshot {
        self.state().snapshots[entry.snapshot_id.as_ref().unwrap()].clone()
    }
    fn start(&self, time: u64) -> Id {
        self.time(time);
        self.rt
            .record_activity(vec!["game".into()], vec!["game".into()], vec![])
            .unwrap();
        self.last().id
    }
    fn close(&self, time: u64) -> Id {
        self.time(time);
        self.rt
            .record_activity(vec![], vec![], vec!["game".into()])
            .unwrap();
        self.last().id
    }
    fn manual(&self, path: &Path, data: &[u8], time: u64) {
        FileSnapshots::default()
            .copy(&self.live, path, &mut |_| {})
            .unwrap();
        fs::write(path.join("nested/save"), data).unwrap();
        set_modified(path, time);
    }
}
fn set_modified(path: &Path, millis: u64) {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(0x100 | 0x80).custom_flags(0x02000000);
    }
    options
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_millis(millis)))
        .unwrap();
}

#[test]
fn flush_refreshes_external_changes_before_accepting_confirmation() {
    for change in ["replace", "modify", "delete", "add"] {
        let f = Fixture::new();
        f.run(Action::Save);
        let old = f.snapshot(&f.last());
        let preview = f.rt.flush_preview("game").unwrap();
        match change {
            "replace" => {
                fs::remove_dir_all(&old.path).unwrap();
                f.manual(&old.path, b"fresh checkpoint", 20_000);
            }
            "modify" => fs::write(old.path.join("nested/save"), b"modified checkpoint").unwrap(),
            "delete" => fs::remove_dir_all(&old.path).unwrap(),
            "add" => f.manual(
                &f.live.with_file_name("Game - Copy (2)"),
                b"additional",
                20_000,
            ),
            _ => unreachable!(),
        }
        let files = FileSnapshots::default();
        let before = files
            .saved_candidates(&f.live)
            .unwrap()
            .into_iter()
            .map(|path| {
                let fingerprint = files.fingerprint(&path).unwrap();
                (path, fingerprint)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            f.rt.accept(
                "game",
                Action::Flush {
                    confirmed_revision: preview.revision
                },
                new_id()
            )
            .unwrap_err()
            .code,
            ErrorCode::ConfirmationRequired,
            "external {change} must invalidate the preview",
        );
        for (path, fingerprint) in before {
            assert_eq!(files.fingerprint(&path).unwrap(), fingerprint);
        }
        assert_eq!(fs::read(f.live.join("nested/save")).unwrap(), b"original");
        if change != "delete" {
            assert!(old.path.is_dir());
        }
        if change == "replace" || change == "modify" {
            let state = f.state();
            assert!(state.snapshots[&old.id].removed_at.is_some());
            assert!(
                state
                    .snapshots
                    .values()
                    .any(|s| s.path == old.path && s.id != old.id && s.available)
            );
        }
        let fresh = f.rt.flush_preview("game").unwrap();
        assert_ne!(fresh.revision, preview.revision);
        f.run(Action::Flush {
            confirmed_revision: fresh.revision,
        });
        for path in fresh.paths {
            assert!(!path.exists());
        }
        assert_eq!(fs::read(f.live.join("nested/save")).unwrap(), b"original");
    }
}
#[test]
fn deleting_all_backups_then_recreating_same_names_gives_new_ids_and_hides_old_rows() {
    let f = Fixture::new();
    let start = f.start(1000);
    f.time(2000);
    f.run(Action::Save);
    let old_a = f.last();
    let a = f.snapshot(&old_a);
    f.time(3000);
    f.run(Action::Save);
    let old_b = f.last();
    let b = f.snapshot(&old_b);
    let close = f.close(4000);
    assert_eq!(f.state().visible_history.len(), 4);
    fs::remove_dir_all(&a.path).unwrap();
    fs::remove_dir_all(&b.path).unwrap();
    f.rt.refresh("game").unwrap();
    let removed = f.state();
    assert!(removed.visible_history.is_empty());
    assert!(
        removed
            .snapshots
            .values()
            .all(|s| s.removal_reason == Some(RemovalReason::Deleted))
    );
    f.time(9000);
    f.manual(&a.path, b"fresh A", 8000);
    f.manual(&b.path, b"fresh B", 8500);
    f.rt.refresh("game").unwrap();
    f.rt.refresh("game").unwrap();
    let state = f.state();
    assert_eq!(state.visible_history.len(), 2);
    assert_eq!(state.snapshots.len(), 4);
    assert!(
        state
            .visible_history
            .iter()
            .all(|h| h.kind == HistoryKind::ExistingBackup
                && h.id != old_a.id
                && h.id != old_b.id
                && h.id != start
                && h.id != close)
    );
    for old in [&old_a, &old_b] {
        assert_eq!(
            f.rt.accept(
                "game",
                Action::Load {
                    target: old.snapshot_id.clone()
                },
                new_id()
            )
            .unwrap_err()
            .code,
            ErrorCode::Unavailable
        );
    }
    f.run(Action::Load { target: None });
    assert_eq!(fs::read(f.live.join("nested/save")).unwrap(), b"fresh B");
    let preview = f.rt.flush_preview("game").unwrap();
    assert_eq!(
        preview.saved, 2,
        "retired generations must not inflate confirmation counts"
    );
}
#[test]
fn replacement_between_scans_and_across_restart_is_imported_once_even_with_same_timestamp() {
    let f = Fixture::new();
    f.run(Action::Save);
    let old = f.last();
    let snapshot = f.snapshot(&old);
    let old_modified = FileSnapshots::default()
        .modified_ms(&snapshot.path)
        .unwrap();
    // Keep the old directory allocated under an unrelated name to establish a
    // distinct filesystem identity even on filesystems that reuse identifiers.
    fs::rename(
        &snapshot.path,
        f.root.path().join("old unrelated directory"),
    )
    .unwrap();
    f.manual(&snapshot.path, b"replacement", old_modified);
    let restarted = Fixture::open(f.root.path(), f.clock.clone());
    restarted.refresh("game").unwrap();
    let state = restarted.state().unwrap();
    assert_eq!(
        state.snapshots[&snapshot.id].removal_reason,
        Some(RemovalReason::Changed)
    );
    assert_eq!(state.visible_history.len(), 1);
    let new = &state.visible_history[0];
    assert_ne!(new.id, old.id);
    assert_ne!(new.snapshot_id, old.snapshot_id);
    assert_eq!(
        restarted
            .accept(
                "game",
                Action::Load {
                    target: old.snapshot_id
                },
                new_id()
            )
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    let id = restarted
        .accept(
            "game",
            Action::Load {
                target: new.snapshot_id.clone(),
            },
            new_id(),
        )
        .unwrap();
    restarted.execute(&id).unwrap();
    assert_eq!(
        fs::read(f.live.join("nested/save")).unwrap(),
        b"replacement"
    );
}
#[test]
fn root_timestamp_nested_edit_added_and_removed_files_each_create_a_new_generation() {
    let f = Fixture::new();
    f.run(Action::Save);
    let path = f.snapshot(&f.last()).path;
    for edit in 0..4 {
        let previous =
            f.rt.state()
                .unwrap()
                .visible_history
                .last()
                .unwrap()
                .clone();
        match edit {
            0 => set_modified(&path, 10_000),
            1 => {
                fs::write(path.join("nested/save"), b"modified nested payload").unwrap();
                set_modified(&path, 10_000);
            }
            2 => {
                fs::write(path.join("new"), b"extra").unwrap();
            }
            _ => fs::remove_file(path.join("new")).unwrap(),
        }
        f.rt.refresh("game").unwrap();
        f.rt.refresh("game").unwrap();
        let state = f.state();
        assert_eq!(state.visible_history.len(), 1);
        assert_ne!(state.visible_history[0].id, previous.id);
        assert_eq!(state.snapshots.len(), (edit + 2) as usize);
        assert_eq!(
            state.snapshots[previous.snapshot_id.as_ref().unwrap()].removal_reason,
            Some(RemovalReason::Changed)
        );
    }
}
#[test]
fn source_changed_after_acceptance_fails_before_preserving_current_data() {
    let f = Fixture::new();
    f.run(Action::Save);
    let snapshot = f.snapshot(&f.last());
    let id =
        f.rt.accept("game", Action::Load { target: None }, new_id())
            .unwrap();
    fs::write(snapshot.path.join("nested/save"), b"different").unwrap();
    assert_eq!(f.rt.execute(&id).unwrap_err().code, ErrorCode::Unavailable);
    let op = f.rt.operation(&id).unwrap();
    assert!(!op.recovery.exists());
    assert_eq!(fs::read(f.live.join("nested/save")).unwrap(), b"original");
}
#[test]
fn retained_recovery_keeps_load_visible_but_changed_recovery_invalidates_revert() {
    let f = Fixture::new();
    f.run(Action::Save);
    let saved = f.last();
    let snapshot = f.snapshot(&saved);
    fs::write(f.live.join("nested/save"), b"before load").unwrap();
    f.run(Action::Load { target: None });
    let load = f.last();
    let recovery = f.state().snapshots[load.recovery_id.as_ref().unwrap()].clone();
    fs::remove_dir_all(snapshot.path).unwrap();
    f.rt.refresh("game").unwrap();
    let state = f.state();
    assert_eq!(state.visible_history.len(), 1);
    assert_eq!(state.visible_history[0].id, load.id);
    assert_eq!(state.visible_history[0].target_id, Some(saved.id));
    f.run(Action::Revert {
        target: load.recovery_id.clone().unwrap(),
    });
    assert_eq!(
        fs::read(f.live.join("nested/save")).unwrap(),
        b"before load"
    );
    fs::write(recovery.path.join("nested/save"), b"tampered recovery").unwrap();
    assert_eq!(
        f.rt.accept(
            "game",
            Action::Revert {
                target: load.recovery_id.clone().unwrap()
            },
            new_id()
        )
        .unwrap_err()
        .code,
        ErrorCode::Unavailable
    );
    let state = f.state();
    assert!(!state.visible_history.iter().any(|h| h.id == load.id));
    assert!(
        !state
            .history
            .iter()
            .any(|h| h.kind == HistoryKind::ExistingBackup)
    );
}
#[test]
fn session_markers_disappear_for_empty_middle_sessions_and_when_no_points_remain() {
    let f = Fixture::new();
    let mut sessions = vec![];
    let mut saved = vec![];
    for i in 0..3 {
        let base = 1000 + i * 10_000;
        let start = f.start(base);
        f.time(base + 1000);
        f.run(Action::Save);
        saved.push(f.snapshot(&f.last()));
        let close = f.close(base + 2000);
        sessions.push((start, close));
    }
    assert_eq!(f.state().visible_history.len(), 9);
    fs::remove_dir_all(&saved[1].path).unwrap();
    f.rt.refresh("game").unwrap();
    let state = f.state();
    assert_eq!(state.visible_history.len(), 6);
    assert!(
        !state
            .visible_history
            .iter()
            .any(|h| h.id == sessions[1].0 || h.id == sessions[1].1)
    );
    for index in [0, 2] {
        fs::remove_dir_all(&saved[index].path).unwrap();
    }
    f.rt.refresh("game").unwrap();
    assert!(f.state().visible_history.is_empty());
    let restarted = Fixture::open(f.root.path(), f.clock.clone());
    assert!(restarted.state().unwrap().visible_history.is_empty());
    assert_eq!(restarted.state().unwrap().history.len(), 9);
}
#[cfg(windows)]
#[test]
fn unreadable_backup_is_unavailable_not_retired_and_does_not_import_partial_generation() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    f.run(Action::Save);
    let old = f.last();
    let snapshot = f.snapshot(&old);
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(snapshot.path.join("nested/save"))
        .unwrap();
    f.rt.refresh("game").unwrap();
    let state = f.state();
    assert!(!state.snapshots[&snapshot.id].available);
    assert!(state.snapshots[&snapshot.id].removed_at.is_none());
    assert_eq!(state.visible_history[0].id, old.id);
    assert_eq!(state.snapshots.len(), 1);
    drop(locked);
    f.rt.refresh("game").unwrap();
    assert!(f.state().snapshots[&snapshot.id].available);
    assert_eq!(f.state().snapshots.len(), 1);
}
#[test]
fn unavailable_parent_does_not_confirm_deletion_and_access_return_preserves_generation() {
    let f = Fixture::new();
    let container = f.root.path().join("drive");
    fs::create_dir(&container).unwrap();
    let live = container.join("Game");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("save"), b"A").unwrap();
    f.rt.configure("drive-game".into(), "Drive game".into(), live, vec![])
        .unwrap();
    let id = f.rt.accept("drive-game", Action::Save, new_id()).unwrap();
    f.rt.execute(&id).unwrap();
    let before = f.state();
    let snapshot = before
        .snapshots
        .values()
        .find(|s| s.game_id == "drive-game")
        .unwrap();
    let offline = f.root.path().join("offline");
    fs::rename(&container, &offline).unwrap();
    f.rt.refresh("drive-game").unwrap();
    let state = f.state();
    assert!(state.snapshots[&snapshot.id].removed_at.is_none());
    assert!(!state.snapshots[&snapshot.id].available);
    fs::rename(&offline, &container).unwrap();
    f.rt.refresh("drive-game").unwrap();
    let state = f.state();
    assert!(state.snapshots[&snapshot.id].available);
    assert_eq!(state.snapshots.len(), 1);
}

#[test]
fn save_can_reuse_a_removed_name_without_retargeting_the_retired_history_entry() {
    let f = Fixture::new();
    f.run(Action::Save);
    let old = f.last();
    let snapshot = f.snapshot(&old);
    fs::remove_dir_all(&snapshot.path).unwrap();
    fs::write(f.live.join("nested/save"), b"new save").unwrap();
    f.run(Action::Save);
    let fresh = f.last();
    assert_ne!(fresh.id, old.id);
    assert_eq!(f.snapshot(&fresh).path, snapshot.path);
    assert_eq!(f.state().visible_history.len(), 1);
    assert_eq!(
        f.rt.accept(
            "game",
            Action::Load {
                target: old.snapshot_id
            },
            new_id()
        )
        .unwrap_err()
        .code,
        ErrorCode::Unavailable
    );
}

#[test]
fn new_manual_checkpoint_after_restart_does_not_reopen_an_old_unclosed_session() {
    let f = Fixture::new();
    let old_start = f.start(1000);
    f.time(2000);
    f.run(Action::Save);
    let snapshot = f.snapshot(&f.last());
    fs::remove_dir_all(&snapshot.path).unwrap();
    f.time(20_000);
    f.manual(&snapshot.path, b"new", 19_000);
    let restarted = Fixture::open(f.root.path(), f.clock.clone());
    let state = restarted.state().unwrap();
    assert_eq!(state.visible_history.len(), 1);
    assert_eq!(state.visible_history[0].kind, HistoryKind::ExistingBackup);
    assert_ne!(state.visible_history[0].id, old_start);
}

#[test]
fn manual_modification_estimate_associates_only_with_its_observed_session() {
    let f = Fixture::new();
    let unrelated_start = f.start(1000);
    let unrelated_close = f.close(2000);
    let relevant_start = f.start(3000);
    let relevant_close = f.close(5000);
    assert!(f.state().visible_history.is_empty());
    f.time(20_000);
    f.manual(&f.live.with_file_name("Game - Copy"), b"manual", 4000);
    f.rt.refresh("game").unwrap();
    let state = f.state();
    assert_eq!(state.visible_history.len(), 3);
    assert!(state.visible_history.iter().any(|h| h.id == relevant_start));
    assert!(state.visible_history.iter().any(|h| h.id == relevant_close));
    assert!(
        !state
            .visible_history
            .iter()
            .any(|h| h.id == unrelated_start || h.id == unrelated_close)
    );
}
