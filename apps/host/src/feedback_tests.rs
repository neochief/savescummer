use super::*;
use savescummer_platform::sounds::{Cue, SoundPlayer};
use std::{
    io,
    sync::{Mutex, mpsc},
    time::Instant,
};

struct RecordingPlayer(mpsc::Sender<Cue>);
impl SoundPlayer for RecordingPlayer {
    fn play(&self, cue: Cue) -> io::Result<()> {
        self.0.send(cue).unwrap();
        Ok(())
    }
}
struct TestRepository {
    state: Mutex<State>,
    fail_completion: AtomicBool,
    fail_startup: AtomicBool,
}
impl Repository for TestRepository {
    fn load(&self) -> Result<State> {
        Ok(self.state.lock().unwrap().clone())
    }
    fn commit(&self, state: &State) -> Result<()> {
        if self.fail_startup.load(Ordering::SeqCst) && state.settings.launch_on_startup {
            return Err(Error::new(
                ErrorCode::Storage,
                "injected startup preference failure",
            ));
        }
        if self.fail_completion.load(Ordering::SeqCst)
            && state
                .operations
                .values()
                .any(|op| op.status == OperationStatus::Completed)
        {
            return Err(Error::new(
                ErrorCode::Storage,
                "injected completion commit failure",
            ));
        }
        *self.state.lock().unwrap() = state.clone();
        Ok(())
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Service>,
    Arc<TestRepository>,
    mpsc::Receiver<Cue>,
) {
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("Game");
    std::fs::create_dir(&live).unwrap();
    std::fs::write(live.join("save"), b"test save").unwrap();
    let repository = Arc::new(TestRepository {
        state: Mutex::new(State::default()),
        fail_completion: AtomicBool::new(false),
        fail_startup: AtomicBool::new(false),
    });
    let paths = Arc::new(Paths::new(vec![]));
    let runtime = Arc::new(
        Runtime::open(
            repository.clone(),
            Arc::new(FileSnapshots::default()),
            paths.clone(),
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    runtime
        .configure("game".into(), "Game".into(), live, vec![])
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let service = Arc::new(Service {
        artwork: None,
        runtime,
        notifications: Notifications::default(),
        startup: None,
        integration_errors: vec![],
        paths,
        sounds: Arc::new(Sounds::new(true, Arc::new(RecordingPlayer(tx)))),
        host_id: new_id(),
        stopping: AtomicBool::new(false),
        shutdown: Notify::new(),
        options: HostOptions {
            cache_dir: None,
            no_artwork: true,
            data_dir: Some(temp.path().into()),
            steam_root: vec![],
            catalog_dir: None,
            copy_label: "Copy".into(),
            no_scan: true,
            no_monitor: true,
            no_audio: false,
            no_integrations: true,
            minimized: true,
            desktop: None,
        },
        admission: Mutex::new(()),
    });
    (temp, service, repository, rx)
}
fn request(command: Command) -> Request {
    Request {
        version: VERSION,
        request_id: new_id(),
        command,
    }
}

#[tokio::test]
async fn artwork_does_not_block_operations_and_watch_publishes_completion() {
    struct PausedDownload {
        started: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl artwork::Downloader for PausedDownload {
        fn icon(&mut self, _: u32) -> anyhow::Result<Vec<u8>> {
            self.started.send(())?;
            self.release.recv_timeout(Duration::from_secs(5))?;
            let mut bytes = std::io::Cursor::new(vec![]);
            image::DynamicImage::new_rgb8(32, 32).write_to(&mut bytes, image::ImageFormat::Png)?;
            Ok(bytes.into_inner())
        }
    }
    let (temp, mut service, repository, _sounds) = fixture();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = artwork::Artwork::start(
        temp.path().join("cache"),
        PausedDownload {
            started: started_tx,
            release: release_rx,
        },
    )
    .unwrap();
    worker.set_games(BTreeMap::from([("game".into(), 42)]));
    Arc::get_mut(&mut service).unwrap().artwork = Some(worker);
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(
        service.handle(&request(Command::CheckArtwork)),
        Reply::Ok
    ));
    let Reply::Accepted { operation_id } = service.handle(&request(Command::Execute {
        game_id: "game".into(),
        action: Action::Save,
    })) else {
        panic!("save was blocked by artwork");
    };
    assert_eq!(
        terminal(&service, &operation_id).await.status,
        OperationStatus::Completed
    );
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(connection(server, service.clone()));
    write_frame(&mut client, &request(Command::Watch))
        .await
        .unwrap();
    let initial: Response = read_frame(&mut client).await.unwrap();
    let Reply::State { state: initial } = initial.result else {
        panic!();
    };
    assert!(initial.artwork["game"].icon_path.is_none());
    release_tx.send(()).unwrap();
    let changed: Response = tokio::time::timeout(Duration::from_secs(3), read_frame(&mut client))
        .await
        .unwrap()
        .unwrap();
    let Reply::State { state } = changed.result else {
        panic!();
    };
    assert!(state.artwork["game"].icon_path.as_ref().unwrap().is_file());
    assert!(state.artwork_revision > initial.artwork_revision);
    assert_eq!(state.revision, initial.revision);
    assert!(repository.state.lock().unwrap().artwork.is_empty());
    service.stopping.store(true, Ordering::SeqCst);
    task.await.unwrap().unwrap();
}
async fn terminal(service: &Service, id: &str) -> Operation {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let op = service.runtime.operation(id).unwrap();
        if op.status != OperationStatus::Pending {
            return op;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn committed_save_plays_once_even_when_client_retries() {
    let (_temp, service, repository, audio) = fixture();
    let save = request(Command::Execute {
        game_id: "game".into(),
        action: Action::Save,
    });
    let Reply::Accepted { operation_id } = service.handle(&save) else {
        panic!("not accepted")
    };
    // Retry while either pending or already completed, then again after completion.
    assert!(matches!(service.handle(&save), Reply::Accepted { .. }));
    assert_eq!(
        terminal(&service, &operation_id).await.status,
        OperationStatus::Completed
    );
    assert!(matches!(service.handle(&save), Reply::Accepted { .. }));
    assert_eq!(
        audio.recv_timeout(Duration::from_secs(5)).unwrap(),
        Cue::SaveStart
    );
    assert_eq!(
        audio.recv_timeout(Duration::from_secs(5)).unwrap(),
        Cue::SaveComplete
    );
    let durable = repository.load().unwrap();
    assert_eq!(durable.operations.len(), 1);
    assert!(durable.history.iter().any(|h| h.kind == HistoryKind::Saved));
    assert!(audio.recv_timeout(Duration::from_millis(150)).is_err());
}

#[tokio::test]
async fn failed_commit_gets_failure_instead_of_completion() {
    let (_temp, service, repository, audio) = fixture();
    repository.fail_completion.store(true, Ordering::SeqCst);
    let Reply::Accepted { operation_id } = service.handle(&request(Command::Execute {
        game_id: "game".into(),
        action: Action::Save,
    })) else {
        panic!("not accepted")
    };
    assert_ne!(
        terminal(&service, &operation_id).await.status,
        OperationStatus::Completed
    );
    assert_eq!(
        audio.recv_timeout(Duration::from_secs(5)).unwrap(),
        Cue::SaveStart
    );
    assert_eq!(
        audio.recv_timeout(Duration::from_secs(5)).unwrap(),
        Cue::Failed
    );
    assert!(
        !repository
            .load()
            .unwrap()
            .history
            .iter()
            .any(|h| h.kind == HistoryKind::Saved)
    );
    assert!(audio.recv_timeout(Duration::from_millis(150)).is_err());
}

#[tokio::test]
async fn rejected_load_has_only_failure_and_muted_save_still_commits() {
    let (_temp, service, repository, audio) = fixture();
    assert!(matches!(
        service.handle(&request(Command::Execute {
            game_id: "game".into(),
            action: Action::Load { target: None },
        })),
        Reply::Error { .. }
    ));
    assert_eq!(
        audio.recv_timeout(Duration::from_secs(5)).unwrap(),
        Cue::Failed
    );
    assert!(matches!(
        service.handle(&request(Command::SetPlaySounds { enabled: false })),
        Reply::Ok
    ));
    assert!(!repository.load().unwrap().settings.play_sounds);
    let Reply::Accepted { operation_id } = service.handle(&request(Command::Execute {
        game_id: "game".into(),
        action: Action::Save,
    })) else {
        panic!("not accepted")
    };
    assert_eq!(
        terminal(&service, &operation_id).await.status,
        OperationStatus::Completed
    );
    assert!(audio.recv_timeout(Duration::from_millis(150)).is_err());
}

#[tokio::test]
async fn active_shortcuts_share_admission_and_retries_keep_the_original_game() {
    let (temp, service, _repository, _audio) = fixture();
    assert!(matches!(
        service.handle(&request(Command::ExecuteActive {
            action: ShortcutAction::Save
        })),
        Reply::Error {
            error: Error {
                code: ErrorCode::Unavailable,
                ..
            }
        }
    ));
    let other = temp.path().join("Other");
    std::fs::create_dir(&other).unwrap();
    service
        .runtime
        .configure("other".into(), "Other".into(), other, vec![])
        .unwrap();
    service
        .runtime
        .record_activity(vec!["game".into()], vec!["game".into()], vec![])
        .unwrap();
    let shortcut = request(Command::ExecuteActive {
        action: ShortcutAction::Save,
    });
    let Reply::Accepted { operation_id } = service.handle(&shortcut) else {
        panic!("shortcut rejected")
    };
    terminal(&service, &operation_id).await;
    service
        .runtime
        .record_activity(
            vec!["other".into(), "game".into()],
            vec!["other".into()],
            vec![],
        )
        .unwrap();
    let Reply::Accepted {
        operation_id: replay,
    } = service.handle(&shortcut)
    else {
        panic!("retry rejected")
    };
    assert_eq!(operation_id, replay);
    assert_eq!(service.runtime.operation(&replay).unwrap().game_id, "game");
    let pending = service
        .runtime
        .accept("other", Action::Save, new_id())
        .unwrap();
    assert!(matches!(
        service.handle(&request(Command::ExecuteActive {
            action: ShortcutAction::Save
        })),
        Reply::Error {
            error: Error {
                code: ErrorCode::Busy,
                ..
            }
        }
    ));
    service.runtime.execute(&pending).unwrap();
}

struct RecordingStartup {
    enabled: AtomicBool,
    fail: AtomicBool,
}
impl StartupRegistration for RecordingStartup {
    fn set_enabled(&self, enabled: bool) -> io::Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(io::Error::other("injected registration failure"));
        }
        self.enabled.store(enabled, Ordering::SeqCst);
        Ok(())
    }
}
#[tokio::test]
async fn startup_registration_and_persistence_failures_preserve_previous_setting() {
    let (_temp, mut service, repository, _audio) = fixture();
    let startup = Arc::new(RecordingStartup {
        enabled: AtomicBool::new(false),
        fail: AtomicBool::new(true),
    });
    Arc::get_mut(&mut service).unwrap().startup = Some(startup.clone());
    let enable = request(Command::SetLaunchOnStartup { enabled: true });
    assert!(matches!(service.handle(&enable), Reply::Error { .. }));
    assert!(!service.runtime.settings().unwrap().launch_on_startup);
    startup.fail.store(false, Ordering::SeqCst);
    repository.fail_startup.store(true, Ordering::SeqCst);
    assert!(matches!(service.handle(&enable), Reply::Error { .. }));
    assert!(!startup.enabled.load(Ordering::SeqCst));
    assert!(!service.runtime.settings().unwrap().launch_on_startup);
    repository.fail_startup.store(false, Ordering::SeqCst);
    assert!(matches!(service.handle(&enable), Reply::Ok));
    assert!(startup.enabled.load(Ordering::SeqCst));
    assert!(repository.load().unwrap().settings.launch_on_startup);
}
