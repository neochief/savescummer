use savescummer_core::*;
use savescummer_ipc::*;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};

struct Host {
    child: Child,
    address: PathBuf,
}
impl Host {
    fn start(root: &Path) -> Self {
        Self::start_options(
            root,
            &[
                "--no-scan",
                "--no-monitor",
                "--no-audio",
                "--no-integrations",
                "--minimized",
            ],
        )
    }
    fn start_options(root: &Path, options: &[&str]) -> Self {
        fs::create_dir_all(root).unwrap();
        let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_savescummer-host"));
        command
            .arg("--data-dir")
            .arg(root)
            .arg("--no-artwork")
            .args(options)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let child = command.spawn().unwrap();
        let mut host = Self {
            child,
            address: endpoint(root).unwrap(),
        };
        let stdout = host.child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send((result, line));
        });
        let (result, line) = rx
            .recv_timeout(Duration::from_secs(15))
            .expect("host readiness timeout");
        result.unwrap();
        assert!(
            line.contains("\"ready\":true"),
            "host failed to start: {line}"
        );
        host
    }
    async fn send(&self, command: savescummer_ipc::Command) -> Reply {
        request(
            &self.address,
            &Request {
                version: VERSION,
                request_id: new_id(),
                command,
            },
        )
        .await
        .unwrap()
        .result
    }
    async fn state(&self) -> LibraryState {
        match self.send(savescummer_ipc::Command::State).await {
            Reply::State { state } => state,
            reply => panic!("{reply:?}"),
        }
    }
    async fn history(&self) -> Vec<History> {
        match self
            .send(savescummer_ipc::Command::History {
                anchor_id: None,
                game_id: "game".into(),
                cursor: None,
                limit: 200,
            })
            .await
        {
            Reply::HistoryPage { page } => {
                page.rows.into_iter().rev().map(|row| row.entry).collect()
            }
            reply => panic!("unexpected history reply: {reply:?}"),
        }
    }
    async fn wait(&self, id: &str) -> Operation {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Reply::Operation { operation } = self
                .send(savescummer_ipc::Command::Operation {
                    operation_id: id.into(),
                })
                .await
                && operation.status != OperationStatus::Pending
            {
                return *operation;
            }
            assert!(Instant::now() < deadline, "operation timeout");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test]
async fn sound_preference_is_default_on_published_and_persisted_across_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let mut host = Host::start(&root);
    let initial = host.state().await;
    assert!(initial.settings.play_sounds);
    let mut watch = connect(&host.address).await.unwrap();
    write_frame(
        &mut watch,
        &Request {
            version: VERSION,
            request_id: new_id(),
            command: savescummer_ipc::Command::Watch,
        },
    )
    .await
    .unwrap();
    let _: Response = read_frame(&mut watch).await.unwrap();
    assert!(matches!(
        host.send(savescummer_ipc::Command::SetPlaySounds { enabled: false })
            .await,
        Reply::Ok
    ));
    let changed: Response = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut watch))
        .await
        .unwrap()
        .unwrap();
    let Reply::State { state } = changed.result else {
        panic!("missing state update")
    };
    assert!(!state.settings.play_sounds);
    assert!(state.revision > initial.revision);
    // The process-only --no-audio option never overwrites the saved preference.
    assert!(matches!(
        host.send(savescummer_ipc::Command::Shutdown).await,
        Reply::Ok
    ));
    assert!(host.child.wait().unwrap().success());
    drop(watch);
    drop(host);
    let mut host = Host::start(&root);
    assert!(!host.state().await.settings.play_sounds);
    assert!(matches!(
        host.send(savescummer_ipc::Command::SetPlaySounds { enabled: true })
            .await,
        Reply::Ok
    ));
    assert!(host.state().await.settings.play_sounds);
    assert!(matches!(
        host.send(savescummer_ipc::Command::Shutdown).await,
        Reply::Ok
    ));
    assert!(host.child.wait().unwrap().success());
}

#[tokio::test]
async fn real_host_named_pipe_reconnect_idempotency_watch_and_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let live = temp.path().join("Game");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("save"), b"A").unwrap();
    let mut host = Host::start(&root);
    assert!(matches!(
        host.send(savescummer_ipc::Command::Configure {
            id: "game".into(),
            name: "Game".into(),
            data_dir: live.clone(),
            executables: vec![]
        })
        .await,
        Reply::Configured { .. }
    ));
    let mut watch = connect(&host.address).await.unwrap();
    write_frame(
        &mut watch,
        &Request {
            version: VERSION,
            request_id: "watch".into(),
            command: savescummer_ipc::Command::Watch,
        },
    )
    .await
    .unwrap();
    let initial: Response = read_frame(&mut watch).await.unwrap();
    let Reply::State {
        state: initial_state,
    } = initial.result
    else {
        panic!()
    };
    // Disconnect after writing the command, before reading its acceptance.
    let command = Request {
        version: VERSION,
        request_id: "reconnect-save".into(),
        command: savescummer_ipc::Command::Execute {
            game_id: "game".into(),
            action: Action::Save,
        },
    };
    let mut disconnected = connect(&host.address).await.unwrap();
    write_frame(&mut disconnected, &command).await.unwrap();
    drop(disconnected);
    let response = request(&host.address, &command).await.unwrap();
    let Reply::Accepted { operation_id } = response.result else {
        panic!("{response:?}")
    };
    let operation = host.wait(&operation_id).await;
    assert_eq!(operation.status, OperationStatus::Completed);
    let duplicate = request(&host.address, &command).await.unwrap();
    assert!(matches!(duplicate.result, Reply::Accepted { operation_id: id } if id == operation_id));
    assert_eq!(host.history().await.len(), 1);
    let changed: Response = tokio::time::timeout(Duration::from_secs(3), read_frame(&mut watch))
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(changed.result, Reply::State { state } if state.revision > initial_state.revision)
    );
    drop(watch);
    let incompatible = request(
        &host.address,
        &Request {
            version: VERSION + 1,
            request_id: new_id(),
            command: savescummer_ipc::Command::State,
        },
    )
    .await
    .unwrap();
    assert!(
        matches!(incompatible.result, Reply::Error { error } if error.code == ErrorCode::InvalidRequest)
    );
    let old_host_id = initial.host_id;
    assert!(matches!(
        host.send(savescummer_ipc::Command::Shutdown).await,
        Reply::Ok
    ));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = host.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(host);
    let restarted = Host::start(&root);
    let response = request(&restarted.address, &command).await.unwrap();
    assert_ne!(response.host_id, old_host_id);
    assert!(matches!(response.result, Reply::Accepted { operation_id: id } if id == operation_id));
    assert_eq!(restarted.history().await.len(), 1);
    assert_eq!(fs::read(live.join("save")).unwrap(), b"A");
}

#[tokio::test]
async fn service_restores_explicit_checkpoint_ids_and_rejects_history_ids() {
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("Game");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("save"), b"A").unwrap();
    let host = Host::start(&temp.path().join("state"));
    assert!(matches!(
        host.send(savescummer_ipc::Command::Configure {
            id: "game".into(),
            name: "Game".into(),
            data_dir: live.clone(),
            executables: vec![],
        })
        .await,
        Reply::Configured { .. }
    ));
    let Reply::Accepted { operation_id } = host
        .send(savescummer_ipc::Command::Execute {
            game_id: "game".into(),
            action: Action::Save,
        })
        .await
    else {
        panic!()
    };
    assert_eq!(
        host.wait(&operation_id).await.status,
        OperationStatus::Completed
    );
    let state = host.state().await;
    let history = host.history().await;
    let saved = history.last().unwrap();
    let checkpoint = saved.snapshot_id.clone().unwrap();
    assert_eq!(
        state.snapshots[&checkpoint].original_data_dir.as_ref(),
        Some(&state.games["game"].data_dir)
    );
    assert!(matches!(host.send(savescummer_ipc::Command::Execute {
        game_id: "game".into(), action: Action::Load { target: Some(saved.id.clone()) },
    }).await, Reply::Error { error } if error.code == ErrorCode::InvalidTarget));
    fs::write(live.join("save"), b"B").unwrap();
    let Reply::Accepted { operation_id } = host
        .send(savescummer_ipc::Command::Execute {
            game_id: "game".into(),
            action: Action::Load {
                target: Some(checkpoint.clone()),
            },
        })
        .await
    else {
        panic!()
    };
    let load = host.wait(&operation_id).await;
    assert_eq!(load.status, OperationStatus::Completed);
    assert_eq!(load.source_id, Some(checkpoint));
    assert_eq!(load.target_id.as_ref(), Some(&saved.id));
    assert_eq!(fs::read(live.join("save")).unwrap(), b"A");
    let recovery = host
        .history()
        .await
        .last()
        .unwrap()
        .recovery_id
        .clone()
        .unwrap();
    let Reply::Accepted { operation_id } = host
        .send(savescummer_ipc::Command::Execute {
            game_id: "game".into(),
            action: Action::Revert {
                target: recovery.clone(),
            },
        })
        .await
    else {
        panic!()
    };
    let revert = host.wait(&operation_id).await;
    assert_eq!(revert.status, OperationStatus::Completed);
    assert_eq!(revert.source_id, Some(recovery));
    assert_eq!(fs::read(live.join("save")).unwrap(), b"B");
}

#[tokio::test]
async fn second_host_cannot_open_same_database() {
    let temp = tempfile::tempdir().unwrap();
    let host = Host::start(temp.path());
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_savescummer-host"));
    command
        .arg("--data-dir")
        .arg(temp.path())
        .args(["--no-scan", "--no-monitor"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("another host"));
    assert!(host.state().await.games.is_empty());
}

#[tokio::test]
async fn history_and_flush_details_are_paged_over_real_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("Game");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("save"), b"live").unwrap();
    for number in 1..=55 {
        let name = if number == 1 {
            "Game - Copy".to_string()
        } else {
            format!("Game - Copy ({number})")
        };
        let path = temp.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("save"), b"backup").unwrap();
    }
    let host = Host::start(&temp.path().join("state"));
    assert!(matches!(
        host.send(savescummer_ipc::Command::Configure {
            id: "game".into(),
            name: "Game".into(),
            data_dir: live,
            executables: vec![]
        })
        .await,
        Reply::Configured { .. }
    ));
    let Reply::HistoryPage { page: first } = host
        .send(savescummer_ipc::Command::History {
            anchor_id: None,
            game_id: "game".into(),
            cursor: None,
            limit: 50,
        })
        .await
    else {
        panic!()
    };
    assert_eq!(first.rows.len(), 50);
    let Reply::HistoryPage { page: last } = host
        .send(savescummer_ipc::Command::History {
            anchor_id: None,
            game_id: "game".into(),
            cursor: first.next_cursor,
            limit: 50,
        })
        .await
    else {
        panic!()
    };
    assert_eq!(last.rows.len(), 5);
    assert!(last.next_cursor.is_none());
    let summary = host.state().await;
    let json = serde_json::to_value(&summary).unwrap();
    assert!(json.get("history").is_none() && json.get("visible_history").is_none());
    assert_eq!(summary.snapshots.len(), 1);
    let Reply::FlushPreview { preview: first } = host
        .send(savescummer_ipc::Command::FlushPreview {
            game_id: "game".into(),
        })
        .await
    else {
        panic!()
    };
    assert_eq!(first.saved, 55);
    assert_eq!(first.paths.len(), 50);
    let cursor = first.next_cursor.unwrap();
    let Reply::FlushPreview { preview: last } = host
        .send(savescummer_ipc::Command::FlushDetails {
            game_id: "game".into(),
            cursor: cursor.clone(),
        })
        .await
    else {
        panic!()
    };
    assert_eq!(last.paths.len(), 5);
    assert_eq!(last.revision, first.revision);
    fs::write(temp.path().join("Game - Copy/save"), b"replacement data").unwrap();
    assert!(
        matches!(host.send(savescummer_ipc::Command::FlushDetails {game_id:"game".into(),cursor}).await,Reply::Error {error} if error.code==ErrorCode::CursorExpired)
    );
}

#[tokio::test]
async fn history_query_exposes_fresh_generation_and_omits_retired_row() {
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("Game");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("save"), b"A").unwrap();
    let host = Host::start(&temp.path().join("state"));
    host.send(savescummer_ipc::Command::Configure {
        id: "game".into(),
        name: "Game".into(),
        data_dir: live,
        executables: vec![],
    })
    .await;
    let Reply::Accepted { operation_id } = host
        .send(savescummer_ipc::Command::Execute {
            game_id: "game".into(),
            action: Action::Save,
        })
        .await
    else {
        panic!()
    };
    assert_eq!(
        host.wait(&operation_id).await.status,
        OperationStatus::Completed
    );
    let state = host.state().await;
    let history = host.history().await;
    let old = &history[0];
    let snapshot = &state.snapshots[old.snapshot_id.as_ref().unwrap()];
    fs::remove_dir_all(&snapshot.path).unwrap();
    let Reply::HistoryPage { page } = host
        .send(savescummer_ipc::Command::History {
            anchor_id: None,
            game_id: "game".into(),
            cursor: None,
            limit: 50,
        })
        .await
    else {
        panic!()
    };
    assert!(page.rows.is_empty());
    fs::create_dir(&snapshot.path).unwrap();
    fs::write(snapshot.path.join("save"), b"fresh").unwrap();
    let Reply::HistoryPage { page } = host
        .send(savescummer_ipc::Command::History {
            anchor_id: None,
            game_id: "game".into(),
            cursor: None,
            limit: 50,
        })
        .await
    else {
        panic!()
    };
    assert_eq!(page.rows.len(), 1);
    assert_ne!(page.rows[0].entry.id, old.id);
    assert_eq!(page.rows[0].entry.kind, HistoryKind::ExistingBackup);
}

#[tokio::test]
async fn external_catalog_discovery_configuration_reset_and_uninstall_are_headless() {
    let local = savescummer_platform::known_folders()["LOCALAPPDATA"].clone();
    let temp = tempfile::tempdir_in(&local).unwrap();
    let install = temp.path().join("Portable");
    let catalogs = temp.path().join("catalog");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir(&catalogs).unwrap();
    fs::write(install.join("game.exe"), b"fixture").unwrap();
    let relative = install
        .strip_prefix(&local)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    fs::write(catalogs.join("game.yaml"), format!("id: portable\nname: Portable\nplatforms:\n  windows:\n    executables: [game.exe]\n    data_dir: '{{INSTALL_DIR}}/saves'\n    known_install_dirs: ['{{LOCALAPPDATA}}/{relative}']\n")).unwrap();
    let root = temp.path().join("state");
    let mut host = Host::start_options(
        &root,
        &[
            "--catalog-dir",
            catalogs.to_str().unwrap(),
            "--no-monitor",
            "--no-audio",
            "--no-integrations",
        ],
    );
    let state = host.state().await;
    assert_eq!(state.games.len(), 1);
    assert!(state.games["portable"].installed);
    assert_eq!(state.games["portable"].detected_locations.len(), 1);
    assert!(!state.availability["portable"].data_available);
    let custom = temp.path().join("custom-saves");
    assert!(matches!(
        host.send(Command::Configure {
            id: "portable".into(),
            name: "Portable".into(),
            data_dir: custom.clone(),
            executables: vec![install.join("game.exe")]
        })
        .await,
        Reply::Configured { .. }
    ));
    host.send(Command::Rescan).await;
    let paths = savescummer_platform::Paths::new(vec![]);
    assert_eq!(
        host.state().await.games["portable"].data_dir,
        paths.resolve(&custom).unwrap()
    );
    assert!(matches!(
        host.send(Command::SelectDetectedLocation {
            game_id: "portable".into(),
            location: None
        })
        .await,
        Reply::Configured { .. }
    ));
    assert_eq!(
        host.state().await.games["portable"].data_dir,
        paths.resolve(&install.join("saves")).unwrap()
    );
    fs::remove_file(install.join("game.exe")).unwrap();
    fs::remove_dir(&install).unwrap();
    host.send(Command::Rescan).await;
    assert!(!host.state().await.games["portable"].installed);
    host.send(Command::Shutdown).await;
    assert!(host.child.wait().unwrap().success());
}

#[tokio::test]
async fn custom_game_addition_and_forget_flush_are_headless_and_persistent() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let live = temp.path().join("Custom saves");
    let executable = temp.path().join("Custom.exe");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("save"), b"current").unwrap();
    fs::write(&executable, b"fixture").unwrap();
    let mut host = Host::start(&root);
    let Reply::Configured { game } = host
        .send(Command::AddCustomGame {
            name: "Custom".into(),
            executable,
            data_dir: live.clone(),
        })
        .await
    else {
        panic!("custom game was not configured")
    };
    assert_eq!(game.origin, GameOrigin::Custom);
    assert!(game.installed);
    let Reply::Accepted { operation_id } = host
        .send(Command::Execute {
            game_id: game.id.clone(),
            action: Action::Save,
        })
        .await
    else {
        panic!("save was not accepted")
    };
    assert_eq!(
        host.wait(&operation_id).await.status,
        OperationStatus::Completed
    );
    let Reply::FlushPreview { preview } = host
        .send(Command::FlushPreview {
            game_id: game.id.clone(),
        })
        .await
    else {
        panic!("forget preview was not returned")
    };
    let paths = preview.paths.clone();
    let Reply::Accepted { operation_id } = host
        .send(Command::Execute {
            game_id: game.id.clone(),
            action: Action::Forget {
                confirmed_revision: preview.revision,
            },
        })
        .await
    else {
        panic!("forget was not accepted")
    };
    assert_eq!(
        host.wait(&operation_id).await.status,
        OperationStatus::Completed
    );
    assert!(!host.state().await.games.contains_key(&game.id));
    assert_eq!(fs::read(live.join("save")).unwrap(), b"current");
    for path in paths {
        assert!(!path.exists(), "{} survived Forget", path.display());
    }
    assert!(matches!(host.send(Command::Shutdown).await, Reply::Ok));
    assert!(host.child.wait().unwrap().success());
    drop(host);
    let mut host = Host::start(&root);
    assert!(!host.state().await.games.contains_key(&game.id));
    assert!(matches!(host.send(Command::Shutdown).await, Reply::Ok));
    assert!(host.child.wait().unwrap().success());
}
