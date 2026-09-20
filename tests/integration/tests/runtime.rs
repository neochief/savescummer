use savescummer_core::*;
use savescummer_platform::{Paths, SystemClock};
use savescummer_snapshots::FileSnapshots;
use savescummer_storage::SqliteRepository;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Fixture {
    temp: tempfile::TempDir,
    runtime: Runtime,
    live: PathBuf,
}
fn runtime(root: &Path) -> Runtime {
    Runtime::open(
        Arc::new(SqliteRepository::open(&root.join("runtime.db")).unwrap()),
        Arc::new(FileSnapshots::default()),
        Arc::new(Paths::new(vec![])),
        Arc::new(SystemClock),
    )
    .unwrap()
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let live = temp.path().join("Game données");
        fs::create_dir(&live).unwrap();
        fs::create_dir_all(live.join("nested/empty")).unwrap();
        fs::write(live.join("nested/雪.txt"), b"snow").unwrap();
        fs::write(live.join("save"), b"A").unwrap();
        let runtime = runtime(temp.path());
        runtime
            .configure("game".into(), "Game".into(), live.clone(), vec![])
            .unwrap();
        Self {
            temp,
            runtime,
            live,
        }
    }
    fn run(&self, action: Action) -> Operation {
        let id = self.runtime.accept("game", action, new_id()).unwrap();
        self.runtime.execute(&id).unwrap();
        self.runtime.operation(&id).unwrap()
    }
    fn data(&self) -> Vec<u8> {
        fs::read(self.live.join("save")).unwrap()
    }
    fn write(&self, data: &[u8]) {
        fs::write(self.live.join("save"), data).unwrap();
    }
    fn last(&self) -> History {
        self.runtime
            .state()
            .unwrap()
            .history
            .last()
            .unwrap()
            .clone()
    }
}
#[test]
fn save_load_revert_revert_persist_exact_relationships_and_empty_directories() {
    let f = Fixture::new();
    f.run(Action::Save);
    let saved = f.last();
    let snapshot =
        f.runtime.state().unwrap().snapshots[&saved.snapshot_id.clone().unwrap()].clone();
    assert!(snapshot.path.join("nested/empty").is_dir());
    assert_eq!(
        fs::read(snapshot.path.join("nested/雪.txt")).unwrap(),
        b"snow"
    );
    f.write(b"B");
    f.run(Action::Load { target: None });
    let load = f.last();
    assert_eq!(f.data(), b"A");
    f.write(b"C");
    f.run(Action::Revert {
        target: load.recovery_id.clone().unwrap(),
    });
    let revert = f.last();
    assert_eq!(f.data(), b"B");
    f.run(Action::Revert {
        target: revert.recovery_id.clone().unwrap(),
    });
    assert_eq!(f.data(), b"C");
    let state = f.runtime.state().unwrap();
    assert_eq!(load.target_id, Some(saved.id));
    assert_eq!(revert.target_id, Some(load.id));
    assert_eq!(fs::read(snapshot.path.join("save")).unwrap(), b"A");
    assert_eq!(state.snapshots.len(), 4);
    assert_eq!(state.history.len(), 4);
    let reopened = runtime(f.temp.path()).state().unwrap();
    assert_eq!(
        reopened.history.last().unwrap().id,
        state.history.last().unwrap().id
    );
    assert_eq!(reopened.snapshots.len(), 4);
}
#[test]
fn unknown_manual_save_time_uses_folder_modification_estimate_for_default_load() {
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"manual");
    let manual = f.live.with_file_name("Game données - Copy (2)");
    FileSnapshots::default()
        .copy(&f.live, &manual, &mut |_| {})
        .unwrap();
    fs::File::open(&manual).ok();
    f.runtime.refresh("game").unwrap();
    f.runtime.refresh("game").unwrap();
    let before = f.runtime.state().unwrap();
    assert_eq!(before.history.len(), 2);
    let imported = before
        .snapshots
        .values()
        .find(|s| s.path == manual)
        .unwrap();
    assert!(imported.saved_at.is_none());
    assert_eq!(
        imported.selection_time,
        FileSnapshots::default().modified_ms(&manual).unwrap()
    );
    f.write(b"current");
    f.run(Action::Load { target: None });
    assert_eq!(f.data(), b"manual");
    assert_eq!(
        f.last().target_id,
        Some(before.history.last().unwrap().id.clone())
    );
}
#[test]
fn unavailable_explicit_target_never_substitutes_or_captures_recovery() {
    let f = Fixture::new();
    f.run(Action::Save);
    let saved = f.last();
    let path = f.runtime.state().unwrap().snapshots[saved.snapshot_id.as_ref().unwrap()]
        .path
        .clone();
    fs::remove_dir_all(path).unwrap();
    f.write(b"B");
    f.run(Action::Save);
    let before = f.runtime.state().unwrap();
    let error = f
        .runtime
        .accept(
            "game",
            Action::Load {
                target: saved.snapshot_id,
            },
            new_id(),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(
        f.runtime.state().unwrap().operations.len(),
        before.operations.len()
    );
    assert_eq!(f.data(), b"B");
}
#[test]
fn per_game_busy_request_id_and_location_rules_are_enforced_in_core() {
    let f = Fixture::new();
    let request = new_id();
    let id = f
        .runtime
        .accept("game", Action::Save, request.clone())
        .unwrap();
    assert_eq!(
        f.runtime
            .accept("game", Action::Save, request.clone())
            .unwrap(),
        id
    );
    assert_eq!(
        f.runtime
            .accept("game", Action::Load { target: None }, request)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        f.runtime
            .accept("game", Action::Save, new_id())
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(
        f.runtime
            .configure("game".into(), "changed".into(), f.live.clone(), vec![])
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    f.runtime.execute(&id).unwrap();
    f.runtime.execute(&id).unwrap();
    assert_eq!(f.runtime.state().unwrap().history.len(), 1);
    let saved = f.last();
    let other = f.temp.path().join("Other");
    fs::create_dir(&other).unwrap();
    f.runtime
        .configure("game".into(), "Game".into(), other, vec![])
        .unwrap();
    assert_eq!(
        f.runtime
            .accept(
                "game",
                Action::Load {
                    target: saved.snapshot_id
                },
                new_id()
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidTarget
    );
}
#[test]
fn overlaps_include_missing_games_and_snapshot_destinations() {
    let f = Fixture::new();
    assert_eq!(
        f.runtime
            .configure(
                "other".into(),
                "Other".into(),
                f.live.join("missing"),
                vec![]
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidPath
    );
    f.runtime
        .configure(
            "other".into(),
            "Other".into(),
            f.live.with_file_name("Game données - Copy"),
            vec![],
        )
        .unwrap();
    let id = f.runtime.accept("game", Action::Save, new_id()).unwrap();
    assert_eq!(
        f.runtime.execute(&id).unwrap_err().code,
        ErrorCode::InvalidPath
    );
    assert!(!f.live.with_file_name("Game données - Copy").exists());
}
#[test]
fn flush_requires_current_confirmation_and_preserves_live_data() {
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"B");
    f.run(Action::Load { target: None });
    let preview = f.runtime.flush_preview("game").unwrap();
    assert_eq!(preview.saved, 1);
    assert_eq!(preview.recovery, 1);
    assert_eq!(preview.retained, 1);
    assert_eq!(
        f.runtime
            .accept(
                "game",
                Action::Flush {
                    confirmed_revision: preview.revision - 1
                },
                new_id()
            )
            .unwrap_err()
            .code,
        ErrorCode::ConfirmationRequired
    );
    f.run(Action::Flush {
        confirmed_revision: preview.revision,
    });
    assert_eq!(f.data(), b"A");
    assert!(f.runtime.state().unwrap().history.is_empty());
    for path in preview.paths {
        assert!(!path.exists(), "{} survived flush", path.display());
    }
}
#[cfg(windows)]
#[test]
fn actual_windows_file_lock_fails_copy_without_publishing_a_checkpoint() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let _locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.live.join("save"))
        .unwrap();
    let id = f.runtime.accept("game", Action::Save, new_id()).unwrap();
    assert!(f.runtime.execute(&id).is_err());
    let state = f.runtime.state().unwrap();
    assert!(state.history.is_empty());
    assert!(state.snapshots.is_empty());
    assert_eq!(state.operations[&id].status, OperationStatus::Failed);
    assert!(state.operations[&id].staging.exists());
}

struct FailCompletion {
    repository: SqliteRepository,
    armed: AtomicBool,
}
impl Repository for FailCompletion {
    fn load(&self) -> Result<State> {
        self.repository.load()
    }
    fn commit(&self, state: &State) -> Result<()> {
        if state.operations.values().any(|op| {
            op.status == OperationStatus::Completed && matches!(op.action, Action::Load { .. })
        }) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(Error::new(
                ErrorCode::Storage,
                "injected completion transaction failure",
            ));
        }
        self.repository.commit(state)
    }
}
#[test]
fn filesystem_success_with_failed_database_commit_requires_explicit_recovery() {
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"B");
    let repository = FailCompletion {
        repository: SqliteRepository::open(&f.temp.path().join("runtime.db")).unwrap(),
        armed: AtomicBool::new(true),
    };
    let rt = Runtime::open(
        Arc::new(repository),
        Arc::new(FileSnapshots::default()),
        Arc::new(Paths::new(vec![])),
        Arc::new(SystemClock),
    )
    .unwrap();
    let id = rt
        .accept("game", Action::Load { target: None }, new_id())
        .unwrap();
    assert!(rt.execute(&id).is_err());
    assert_eq!(f.data(), b"A");
    assert_eq!(
        rt.operation(&id).unwrap().status,
        OperationStatus::RecoveryNeeded
    );
    assert_eq!(rt.state().unwrap().history.len(), 1);
    assert_eq!(
        rt.accept("game", Action::Save, new_id()).unwrap_err().code,
        ErrorCode::RecoveryNeeded
    );
    f.write(b"new progress");
    let recover = rt
        .accept(
            "game",
            Action::Recover {
                operation: id.clone(),
                choice: RecoveryChoice::RestoreBefore,
            },
            new_id(),
        )
        .unwrap();
    rt.execute(&recover).unwrap();
    assert_eq!(f.data(), b"B");
    let recovery = rt.operation(&recover).unwrap();
    assert_eq!(
        fs::read(recovery.recovery.join("save")).unwrap(),
        b"new progress"
    );
    assert_eq!(rt.operation(&id).unwrap().status, OperationStatus::Resolved);
    assert_eq!(rt.state().unwrap().history.len(), 1);
}

// A child test process executes the real workflow against the real SQLite store.
// Only this repository wrapper adds a deterministic pause after a durable phase.
struct PauseAfterPhase {
    repository: SqliteRepository,
    phase: Phase,
    ready: PathBuf,
}
impl Repository for PauseAfterPhase {
    fn load(&self) -> Result<State> {
        self.repository.load()
    }
    fn commit(&self, state: &State) -> Result<()> {
        self.repository.commit(state)?;
        if state
            .operations
            .values()
            .any(|o| o.status == OperationStatus::Pending && o.phase == self.phase)
        {
            fs::write(&self.ready, b"ready").unwrap();
            loop {
                std::thread::park();
            }
        }
        Ok(())
    }
}
#[test]
#[ignore = "subprocess fixture, invoked by restart tests"]
fn crash_worker() {
    let root = PathBuf::from(std::env::var_os("SAVESCUMMER_TEST_ROOT").unwrap());
    let phase = match std::env::var("SAVESCUMMER_TEST_PHASE").unwrap().as_str() {
        "original_moved" => Phase::OriginalMoved,
        "replacement_installed" => Phase::ReplacementInstalled,
        _ => Phase::RecoveryReady,
    };
    let repository = PauseAfterPhase {
        repository: SqliteRepository::open(&root.join("runtime.db")).unwrap(),
        phase,
        ready: root.join("barrier"),
    };
    let rt = Runtime::open(
        Arc::new(repository),
        Arc::new(FileSnapshots::default()),
        Arc::new(Paths::new(vec![])),
        Arc::new(SystemClock),
    )
    .unwrap();
    let id = rt
        .accept("game", Action::Load { target: None }, new_id())
        .unwrap();
    rt.execute(&id).unwrap();
}
fn interrupt(f: &Fixture, phase: &str) {
    use std::process::{Command, Stdio};
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "crash_worker", "--ignored", "--nocapture"])
        .env("SAVESCUMMER_TEST_ROOT", f.temp.path())
        .env("SAVESCUMMER_TEST_PHASE", phase)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !f.temp.path().join("barrier").exists() {
        if child.try_wait().unwrap().is_some() || std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child did not reach {phase}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
}
#[test]
fn real_process_interrupted_before_live_change_does_not_block_on_restart() {
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"B");
    interrupt(&f, "recovery_ready");
    let rt = runtime(f.temp.path());
    assert_eq!(f.data(), b"B");
    assert!(
        !rt.state()
            .unwrap()
            .operations
            .values()
            .any(Operation::blocks)
    );
    assert_eq!(rt.state().unwrap().history.len(), 1);
}
#[test]
fn real_process_interrupted_after_original_move_rolls_back_without_ui() {
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"B");
    interrupt(&f, "original_moved");
    assert!(!f.live.exists());
    let rt = runtime(f.temp.path());
    assert_eq!(f.data(), b"B");
    assert!(
        !rt.state()
            .unwrap()
            .operations
            .values()
            .any(Operation::blocks)
    );
    assert_eq!(rt.state().unwrap().history.len(), 1);
}
#[test]
fn real_process_interrupted_after_replacement_preserves_newer_progress_until_choice() {
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"B");
    interrupt(&f, "replacement_installed");
    f.write(b"new progress");
    let rt = runtime(f.temp.path());
    assert_eq!(f.data(), b"new progress");
    let old = rt
        .state()
        .unwrap()
        .operations
        .values()
        .find(|o| o.status == OperationStatus::RecoveryNeeded)
        .unwrap()
        .id
        .clone();
    assert_eq!(
        rt.accept("game", Action::Save, new_id()).unwrap_err().code,
        ErrorCode::RecoveryNeeded
    );
    let id = rt
        .accept(
            "game",
            Action::Recover {
                operation: old,
                choice: RecoveryChoice::KeepCurrent,
            },
            new_id(),
        )
        .unwrap();
    rt.execute(&id).unwrap();
    assert_eq!(f.data(), b"new progress");
    let restarted = runtime(f.temp.path());
    assert!(
        !restarted
            .state()
            .unwrap()
            .operations
            .values()
            .any(Operation::blocks)
    );
    assert_eq!(restarted.state().unwrap().history.len(), 1);
}
#[test]
fn dedicated_recovery_can_restore_missing_live_directory() {
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"B");
    interrupt(&f, "replacement_installed");
    let rt = runtime(f.temp.path());
    let old = rt
        .state()
        .unwrap()
        .operations
        .values()
        .find(|o| o.status == OperationStatus::RecoveryNeeded)
        .unwrap()
        .id
        .clone();
    fs::remove_dir_all(&f.live).unwrap();
    let keep = rt
        .accept(
            "game",
            Action::Recover {
                operation: old.clone(),
                choice: RecoveryChoice::KeepCurrent,
            },
            new_id(),
        )
        .unwrap();
    assert!(rt.execute(&keep).is_err());
    assert_eq!(
        rt.operation(&old).unwrap().status,
        OperationStatus::RecoveryNeeded
    );
    let restore = rt
        .accept(
            "game",
            Action::Recover {
                operation: old,
                choice: RecoveryChoice::RestoreBefore,
            },
            new_id(),
        )
        .unwrap();
    rt.execute(&restore).unwrap();
    assert_eq!(f.data(), b"B");
}

#[cfg(windows)]
#[test]
fn failure_preserving_locked_current_data_leaves_live_untouched_and_no_loaded_entry() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    f.run(Action::Save);
    f.write(b"B");
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.live.join("save"))
        .unwrap();
    let id = f
        .runtime
        .accept("game", Action::Load { target: None }, new_id())
        .unwrap();
    assert!(f.runtime.execute(&id).is_err());
    drop(locked);
    assert_eq!(f.data(), b"B");
    assert_eq!(f.runtime.state().unwrap().history.len(), 1);
    assert_eq!(
        f.runtime.operation(&id).unwrap().status,
        OperationStatus::Failed
    );
}

#[cfg(windows)]
#[test]
fn partial_flush_reports_failure_preserves_remaining_records_and_can_be_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    f.run(Action::Save);
    let first = f.last();
    f.write(b"B");
    f.run(Action::Save);
    let state = f.runtime.state().unwrap();
    let path = state.snapshots[first.snapshot_id.as_ref().unwrap()]
        .path
        .clone();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path.join("save"))
        .unwrap();
    let preview = f.runtime.flush_preview("game").unwrap();
    let id = f
        .runtime
        .accept(
            "game",
            Action::Flush {
                confirmed_revision: preview.revision,
            },
            new_id(),
        )
        .unwrap();
    assert!(f.runtime.execute(&id).is_err());
    assert_eq!(
        f.runtime.operation(&id).unwrap().status,
        OperationStatus::Failed
    );
    assert_eq!(f.data(), b"B");
    assert!(path.exists());
    assert!(!f.runtime.state().unwrap().history.is_empty());
    drop(locked);
    let preview = f.runtime.flush_preview("game").unwrap();
    f.run(Action::Flush {
        confirmed_revision: preview.revision,
    });
    assert!(f.runtime.state().unwrap().history.is_empty());
    assert_eq!(f.data(), b"B");
}

struct RenameFailures {
    files: FileSnapshots,
    live: PathBuf,
    rollback_fails: AtomicBool,
}
impl SnapshotIo for RenameFailures {
    fn accessible_dir(&self, path: &Path) -> Result<bool> {
        self.files.accessible_dir(path)
    }
    fn exists(&self, path: &Path) -> Result<bool> {
        self.files.exists(path)
    }
    fn identity(&self, path: &Path) -> Result<String> {
        self.files.identity(path)
    }
    fn modified_ms(&self, path: &Path) -> Result<u64> {
        self.files.modified_ms(path)
    }
    fn fingerprint(&self, path: &Path) -> Result<String> {
        self.files.fingerprint(path)
    }
    fn copy(&self, source: &Path, target: &Path, progress: &mut dyn FnMut(u64)) -> Result<()> {
        self.files.copy(source, target, progress)
    }
    fn saved_candidates(&self, live: &Path) -> Result<Vec<PathBuf>> {
        self.files.saved_candidates(live)
    }
    fn next_saved_path(&self, live: &Path, reserved: &[PathBuf]) -> Result<PathBuf> {
        self.files.next_saved_path(live, reserved)
    }
    fn remove(&self, path: &Path) -> Result<()> {
        self.files.remove(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        if to == self.live
            && (from.to_string_lossy().contains(".staging-")
                || self.rollback_fails.load(Ordering::SeqCst))
        {
            return Err(Error::new(ErrorCode::Io, "injected rename failure"));
        }
        self.files.rename(from, to)
    }
}
#[test]
fn replacement_failure_rolls_back_and_rollback_failure_can_be_retried() {
    for fail_rollback in [false, true] {
        let f = Fixture::new();
        f.run(Action::Save);
        f.write(b"B");
        let files = Arc::new(RenameFailures {
            files: FileSnapshots::default(),
            live: f.live.clone(),
            rollback_fails: AtomicBool::new(fail_rollback),
        });
        let rt = Runtime::open(
            Arc::new(SqliteRepository::open(&f.temp.path().join("runtime.db")).unwrap()),
            files.clone(),
            Arc::new(Paths::new(vec![])),
            Arc::new(SystemClock),
        )
        .unwrap();
        let id = rt
            .accept("game", Action::Load { target: None }, new_id())
            .unwrap();
        assert!(rt.execute(&id).is_err());
        if fail_rollback {
            assert_eq!(
                rt.operation(&id).unwrap().status,
                OperationStatus::RecoveryNeeded
            );
            assert!(!f.live.exists());
            files.rollback_fails.store(false, Ordering::SeqCst);
            let retry = rt
                .accept(
                    "game",
                    Action::Recover {
                        operation: id.clone(),
                        choice: RecoveryChoice::Retry,
                    },
                    new_id(),
                )
                .unwrap();
            rt.execute(&retry).unwrap();
            assert_eq!(rt.operation(&id).unwrap().status, OperationStatus::Resolved);
        } else {
            assert_eq!(rt.operation(&id).unwrap().status, OperationStatus::Failed);
        }
        assert_eq!(f.data(), b"B");
        assert_eq!(rt.state().unwrap().history.len(), 1);
    }
}
