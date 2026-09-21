use anyhow::Context;
use savescummer_core::*;
use savescummer_ipc::*;
use savescummer_monitor::{Monitor, ObservationSource};
use savescummer_platform::desktop::{
    Desktop, DesktopEvent, NativeStartup, Notifications, StartupRegistration,
};
use savescummer_platform::sounds::{NativeSoundPlayer, Sounds};
use savescummer_platform::{NativeDiscovery, NativeObserver, Paths, SystemClock};
use savescummer_snapshots::FileSnapshots;
use savescummer_storage::SqliteRepository;
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::Notify,
};

mod artwork;
#[cfg(test)]
mod feedback_tests;
include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

pub fn default_data_dir() -> anyhow::Result<PathBuf> {
    let folders = savescummer_platform::known_folders();
    let root = folders
        .get("LOCALAPPDATA")
        .or_else(|| folders.get("XDG_DATA_HOME"))
        .context("cannot resolve per-user application data directory")?;
    Ok(root.join("SaveScummer"))
}
#[derive(clap::Parser, Debug, Clone)]
pub struct HostOptions {
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Override the disposable artwork cache root.
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
    /// Disable artwork downloads and cache checks for this run.
    #[arg(long)]
    pub no_artwork: bool,
    #[arg(long)]
    pub steam_root: Vec<PathBuf>,
    /// Replace the embedded catalog with YAML files from this directory.
    #[arg(long)]
    pub catalog_dir: Option<PathBuf>,
    #[arg(long, default_value = "Copy")]
    pub copy_label: String,
    /// Run only explicitly configured games (useful for isolated tests).
    #[arg(long)]
    pub no_scan: bool,
    #[arg(long)]
    pub no_monitor: bool,
    /// Suppress audio for this host run without changing the saved preference.
    #[arg(long)]
    pub no_audio: bool,
    /// Disable tray, shortcuts, notifications and changes to OS startup settings.
    #[arg(long)]
    pub no_integrations: bool,
    /// Start the background host without opening a desktop client.
    #[arg(long)]
    pub minimized: bool,
    /// Optional desktop executable to launch from the tray.
    #[arg(long)]
    pub desktop: Option<PathBuf>,
}
struct Service {
    artwork: Option<artwork::Artwork>,
    runtime: Arc<Runtime>,
    sounds: Arc<Sounds>,
    notifications: Notifications,
    startup: Option<Arc<dyn StartupRegistration>>,
    integration_errors: Vec<String>,
    paths: Arc<Paths>,
    host_id: Id,
    stopping: AtomicBool,
    shutdown: Notify,
    options: HostOptions,
    /// Serializes request admission with shutdown; filesystem work runs outside it.
    admission: std::sync::Mutex<()>,
}
impl Service {
    fn flush_page(
        &self,
        game_id: &str,
        cursor: Option<&str>,
    ) -> savescummer_core::Result<FlushPreview> {
        let mut preview = self.runtime.flush_preview(game_id)?;
        let offset = if let Some(cursor) = cursor {
            let (host, game, revision, offset): (String, String, u64, usize) =
                serde_json::from_str(cursor)
                    .map_err(|_| Error::new(ErrorCode::InvalidRequest, "invalid Flush cursor"))?;
            if host != self.host_id || game != game_id || revision != preview.revision {
                return Err(Error::new(
                    ErrorCode::CursorExpired,
                    "backups changed; obtain a new Flush preview",
                ));
            }
            offset
        } else {
            0
        };
        if offset > preview.paths.len() {
            return Err(Error::new(
                ErrorCode::InvalidRequest,
                "invalid Flush offset",
            ));
        }
        let total = preview.paths.len();
        preview.paths = preview.paths.into_iter().skip(offset).take(50).collect();
        let mut bytes = 0;
        let mut count = 0;
        for path in &preview.paths {
            let size = serde_json::to_vec(path)
                .map_err(|e| Error::new(ErrorCode::Storage, e.to_string()))?
                .len();
            if size > 512 * 1024 {
                return Err(Error::new(
                    ErrorCode::ResponseTooLarge,
                    "one path exceeds the page byte budget",
                ));
            }
            if bytes + size > 512 * 1024 {
                break;
            }
            bytes += size;
            count += 1;
        }
        preview.paths.truncate(count);
        preview.next_cursor = (offset + count < total).then(|| {
            serde_json::to_string(&(&self.host_id, game_id, preview.revision, offset + count))
                .unwrap()
        });
        Ok(preview)
    }
    fn state(&self) -> savescummer_core::Result<LibraryState> {
        let mut state = self.runtime.summary()?;
        if let Some(artwork) = &self.artwork {
            artwork.project(&mut state);
        }
        Ok(state.into())
    }
    fn execute(
        self: &Arc<Self>,
        game_id: &str,
        action: &Action,
        request_id: &str,
    ) -> savescummer_core::Result<Reply> {
        let replay = self.runtime.operation_for_request(request_id)?.is_some();
        let id = self
            .runtime
            .accept(game_id, action.clone(), request_id.into())?;
        if !replay {
            self.sounds.started(action);
            let service = self.clone();
            let operation_id = id.clone();
            let action = action.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(error) = service.runtime.execute(&operation_id) {
                    eprintln!("operation {operation_id}: {error}");
                    service.sounds.finished(&action, OperationStatus::Failed);
                    service.notifications.failure(error.message);
                } else if let Ok(operation) = service.runtime.operation(&operation_id) {
                    service.sounds.finished(&action, operation.status);
                    if let Some(error) = operation.error {
                        service.notifications.failure(error.message);
                    }
                }
            });
        }
        Ok(Reply::Accepted { operation_id: id })
    }
    fn scan(&self) -> anyhow::Result<()> {
        let mut catalog_errors = vec![];
        let catalog = if let Some(directory) = &self.options.catalog_dir {
            let mut texts = vec![];
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                if matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("yaml" | "yml")
                ) {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => texts.push(text),
                        Err(error) => catalog_errors.push(format!("{}: {error}", path.display())),
                    }
                }
            }
            texts
        } else {
            BUILTIN_CATALOG
                .iter()
                .map(|text| text.to_string())
                .collect()
        };
        let mut definitions = BTreeMap::new();
        for text in catalog {
            match savescummer_scanner::parse_catalog(&text) {
                Ok(definition) => {
                    if definitions.contains_key(&definition.id) {
                        anyhow::bail!("duplicate catalog ID {}", definition.id);
                    }
                    definitions.insert(definition.id.clone(), definition);
                }
                Err(error) => catalog_errors.push(error.to_string()),
            }
        }
        let definitions: Vec<_> = definitions.into_values().collect();
        let scan = savescummer_scanner::scan(
            &definitions,
            &NativeDiscovery {
                additional_steam_roots: self.options.steam_root.clone(),
            },
        );
        self.paths
            .protect(scan.library_roots.iter().flat_map(|root| {
                [
                    root.clone(),
                    root.join("steamapps"),
                    root.join("steamapps/common"),
                ]
            }))?;
        for error in &scan.errors {
            eprintln!("scan: {error}");
        }
        let mut errors = self.integration_errors.clone();
        errors.extend(catalog_errors);
        errors.extend(scan.errors);
        let mut candidates = BTreeMap::<String, Vec<_>>::new();
        for candidate in scan.candidates {
            candidates
                .entry(candidate.game_id.clone())
                .or_default()
                .push(candidate);
        }
        for (id, found) in candidates {
            let candidate = &found[0];
            if let Err(error) = self.runtime.record_discovery(
                id.clone(),
                candidate.name.clone(),
                candidate.info.clone(),
                found
                    .iter()
                    .map(|c| GameLocation {
                        data_dir: c.data_dir.clone(),
                        executables: c.executables.clone(),
                    })
                    .collect(),
            ) && !matches!(error.code, ErrorCode::Busy | ErrorCode::RecoveryNeeded)
            {
                errors.push(error.to_string());
            }
        }
        for game in self.runtime.games()?.values() {
            if game.executables.iter().any(|exe| exe.is_file()) {
                self.runtime.set_installed(&game.id, true)?;
            } else if !game.executables.is_empty()
                && game
                    .executables
                    .iter()
                    .all(|exe| savescummer_platform::confirmed_missing(exe))
            {
                self.runtime.set_installed(&game.id, false)?;
            }
        }
        self.runtime.set_discovery_errors(errors)?;
        if let Some(artwork) = &self.artwork {
            let games = self.runtime.games()?;
            artwork.set_games(
                definitions
                    .iter()
                    .filter_map(|definition| {
                        let app = definition.stores.steam?;
                        games
                            .get(&definition.id)
                            .filter(|game| game.installed)
                            .map(|_| (definition.id.clone(), app))
                    })
                    .collect(),
            );
        }
        for id in self.runtime.games()?.keys() {
            if let Err(error) = self.runtime.refresh(id)
                && !matches!(error.code, ErrorCode::Busy | ErrorCode::RecoveryNeeded)
            {
                eprintln!("discovery {id}: {error}");
            }
        }
        Ok(())
    }
    fn handle(self: &Arc<Self>, request: &Request) -> Reply {
        let result = (|| -> savescummer_core::Result<Reply> {
            let _admission = self
                .admission
                .lock()
                .map_err(|_| Error::new(ErrorCode::Storage, "admission lock poisoned"))?;
            if request.version != VERSION {
                return Err(Error::new(
                    ErrorCode::InvalidRequest,
                    "unsupported protocol version",
                ));
            }
            if self.stopping.load(Ordering::SeqCst) {
                return Err(Error::new(ErrorCode::ShuttingDown, "host is shutting down"));
            }
            Ok(match &request.command {
                Command::State | Command::Watch => Reply::State {
                    state: self.state()?,
                },
                Command::CheckArtwork => {
                    if let Some(artwork) = &self.artwork {
                        artwork.check();
                    }
                    Reply::Ok
                }
                Command::SetPlaySounds { enabled } => {
                    self.runtime.set_play_sounds(*enabled)?;
                    self.sounds.set_enabled(*enabled && !self.options.no_audio);
                    Reply::Ok
                }
                Command::SetLaunchOnStartup { enabled } => {
                    let startup = self.startup.as_ref().ok_or_else(|| {
                        Error::new(
                            ErrorCode::Unavailable,
                            "OS integrations are disabled for this host",
                        )
                    })?;
                    let previous = self.runtime.settings()?.launch_on_startup;
                    startup
                        .set_enabled(*enabled)
                        .map_err(|e| Error::new(ErrorCode::Io, e.to_string()))?;
                    if let Err(error) = self.runtime.set_launch_on_startup(*enabled) {
                        if let Err(rollback) = startup.set_enabled(previous) {
                            return Err(Error::new(
                                ErrorCode::Storage,
                                format!("{error}; startup rollback failed: {rollback}"),
                            ));
                        }
                        return Err(error);
                    }
                    Reply::Ok
                }
                Command::ExplorerTargets { path } => Reply::ExplorerTargets {
                    targets: self.runtime.explorer_targets(path)?,
                },
                Command::Explore { game_id } => {
                    savescummer_platform::desktop::explore(&self.runtime.explore_parent(game_id)?)
                        .map_err(|e| Error::new(ErrorCode::Io, e.to_string()))?;
                    Reply::Ok
                }
                Command::ExecuteActive { action } => {
                    // Reconnect/retry targets the originally accepted game even if focus moved.
                    let game_id = if let Some(id) =
                        self.runtime.operation_for_request(&request.request_id)?
                    {
                        self.runtime.operation(&id)?.game_id
                    } else {
                        self.runtime.active_game()?
                    };
                    self.execute(&game_id, &action.action(), &request.request_id)?
                }
                Command::History {
                    game_id,
                    anchor_id,
                    cursor,
                    limit,
                } => {
                    match if cursor.is_none() {
                        self.runtime.refresh(game_id)
                    } else {
                        Ok(())
                    } {
                        Ok(()) => (),
                        Err(error)
                            if matches!(
                                error.code,
                                ErrorCode::Busy | ErrorCode::RecoveryNeeded
                            ) => {}
                        Err(error) => return Err(error),
                    }
                    Reply::HistoryPage {
                        page: self.runtime.history_page_at(
                            game_id,
                            cursor.as_deref(),
                            *limit,
                            anchor_id.as_deref(),
                        )?,
                    }
                }
                Command::Configure {
                    id,
                    name,
                    data_dir,
                    executables,
                } => Reply::Configured {
                    game: self.runtime.configure(
                        id.clone(),
                        name.clone(),
                        data_dir.clone(),
                        executables.clone(),
                    )?,
                },
                Command::SelectDetectedLocation { game_id, location } => Reply::Configured {
                    game: self
                        .runtime
                        .select_detected_location(game_id, location.as_ref())?,
                },
                Command::Execute { game_id, action } => {
                    self.execute(game_id, action, &request.request_id)?
                }
                Command::Operation { operation_id } => Reply::Operation {
                    operation: Box::new(self.runtime.operation(operation_id)?),
                },
                Command::FlushPreview { game_id } => Reply::FlushPreview {
                    preview: self.flush_page(game_id, None)?,
                },
                Command::FlushDetails { game_id, cursor } => Reply::FlushPreview {
                    preview: self.flush_page(game_id, Some(cursor))?,
                },
                Command::Rescan => {
                    self.scan()
                        .map_err(|e| Error::new(ErrorCode::Io, e.to_string()))?;
                    Reply::Ok
                }
                Command::Shutdown => {
                    self.stopping.store(true, Ordering::SeqCst);
                    self.shutdown.notify_one();
                    Reply::Ok
                }
            })
        })();
        result.unwrap_or_else(|error| {
            if matches!(
                request.command,
                Command::Execute { .. } | Command::ExecuteActive { .. }
            ) {
                self.sounds.rejected(error.code);
                self.notifications.failure(error.message.clone());
            }
            Reply::Error { error }
        })
    }
}
async fn write_response<T: AsyncWrite + Unpin>(
    stream: &mut T,
    response: &Response,
) -> std::io::Result<()> {
    if serde_json::to_vec(response)
        .map_err(std::io::Error::other)?
        .len()
        > MAX_FRAME
    {
        let error = Response {
            version: VERSION,
            request_id: response.request_id.clone(),
            host_id: response.host_id.clone(),
            result: Reply::Error {
                error: Error::new(
                    ErrorCode::ResponseTooLarge,
                    "response exceeds the frame limit; request a smaller page or reduce the oversized item",
                ),
            },
        };
        return write_frame(stream, &error).await;
    }
    write_frame(stream, response).await
}
async fn connection<T: AsyncRead + AsyncWrite + Unpin>(
    mut stream: T,
    service: Arc<Service>,
) -> anyhow::Result<()> {
    let request: Request =
        tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream)).await??;
    let handler = service.clone();
    let command = request.clone();
    let result = tokio::task::spawn_blocking(move || handler.handle(&command)).await?;
    let mut revision = if let Reply::State { state } = &result {
        Some((state.revision, state.artwork_revision))
    } else {
        None
    };
    let response = Response {
        version: VERSION,
        request_id: request.request_id.clone(),
        host_id: service.host_id.clone(),
        result,
    };
    tokio::time::timeout(
        Duration::from_secs(10),
        write_response(&mut stream, &response),
    )
    .await??;
    if matches!(request.command, Command::Watch) && revision.is_some() {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if service.stopping.load(Ordering::SeqCst) {
                let response = Response {
                    version: VERSION,
                    request_id: request.request_id.clone(),
                    host_id: service.host_id.clone(),
                    result: Reply::ShuttingDown,
                };
                tokio::time::timeout(
                    Duration::from_secs(10),
                    write_response(&mut stream, &response),
                )
                .await??;
                break;
            }
            let versions = (
                service.runtime.revision()?,
                service.artwork.as_ref().map_or(0, |a| a.revision()),
            );
            if revision == Some(versions) {
                continue;
            }
            let state = service.state()?;
            if revision != Some((state.revision, state.artwork_revision)) {
                revision = Some((state.revision, state.artwork_revision));
                let response = Response {
                    version: VERSION,
                    request_id: request.request_id.clone(),
                    host_id: service.host_id.clone(),
                    result: Reply::State { state },
                };
                tokio::time::timeout(
                    Duration::from_secs(10),
                    write_response(&mut stream, &response),
                )
                .await??;
            }
        }
    }
    Ok(())
}
pub async fn serve(options: HostOptions) -> anyhow::Result<()> {
    let data_dir = options
        .data_dir
        .clone()
        .map(Ok)
        .unwrap_or_else(default_data_dir)?;
    let data_dir = if data_dir.is_absolute() {
        data_dir
    } else {
        std::env::current_dir()?.join(data_dir)
    };
    std::fs::create_dir_all(&data_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let instance = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data_dir.join("host.lock"))?;
    fs2::FileExt::try_lock_exclusive(&instance).context("another host owns this data directory")?;
    let discovery = NativeDiscovery {
        additional_steam_roots: options.steam_root.clone(),
    };
    use savescummer_scanner::Discovery;
    let mut protected = vec![data_dir.clone()];
    for root in discovery.steam_roots() {
        protected.extend([
            root.clone(),
            root.join("steamapps"),
            root.join("steamapps/common"),
        ]);
    }
    let snapshots = FileSnapshots {
        copy_label: options.copy_label.clone(),
        recognized_labels: vec!["Copy".into(), "Copie".into(), options.copy_label.clone()],
    };
    let paths = Arc::new(Paths::system(protected));
    let runtime = Arc::new(Runtime::open(
        Arc::new(SqliteRepository::open(&data_dir.join("runtime.db"))?),
        Arc::new(snapshots),
        paths.clone(),
        Arc::new(SystemClock),
    )?);
    let sounds = Arc::new(Sounds::new(
        runtime.settings()?.play_sounds && !options.no_audio,
        Arc::new(NativeSoundPlayer),
    ));
    let (desktop_tx, desktop_rx) = std::sync::mpsc::channel();
    let mut integration_errors = vec![];
    let desktop = if cfg!(windows) && !options.no_integrations {
        match Desktop::start(desktop_tx) {
            Ok(desktop) => {
                integration_errors.extend(desktop.warnings.clone());
                Some(desktop)
            }
            Err(error) => {
                integration_errors.push(format!("desktop integrations: {error}"));
                None
            }
        }
    } else {
        None
    };
    let startup: Option<Arc<dyn StartupRegistration>> = if cfg!(windows) && !options.no_integrations
    {
        Some(Arc::new(NativeStartup {
            host: std::env::current_exe()?,
            data_dir: data_dir.clone(),
            desktop: options.desktop.clone(),
        }))
    } else {
        None
    };
    // Only explicit SetLaunchOnStartup requests may change registration. Starting
    // another build with the same saved preference must not take over autostart.
    runtime.set_discovery_errors(integration_errors.clone())?;
    let artwork = if options.no_artwork {
        None
    } else {
        let root = options
            .cache_dir
            .clone()
            .map(Ok)
            .unwrap_or_else(savescummer_platform::artwork_cache_dir);
        match root.and_then(|root| {
            let root = if root.is_absolute() {
                root
            } else {
                std::env::current_dir()?.join(root)
            };
            paths
                .protect([root.clone()])
                .map_err(std::io::Error::other)?;
            artwork::Artwork::start(root, artwork::SteamDownloader::default())
        }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                eprintln!("artwork unavailable: {error}");
                None
            }
        }
    };
    let service = Arc::new(Service {
        artwork,
        runtime,
        sounds,
        notifications: desktop
            .as_ref()
            .map(|d| d.notifications.clone())
            .unwrap_or_default(),
        startup,
        integration_errors,
        paths,
        host_id: new_id(),
        stopping: AtomicBool::new(false),
        shutdown: Notify::new(),
        options,
        admission: std::sync::Mutex::new(()),
    });
    if !service.options.no_scan {
        service.scan()?;
    } else {
        for id in service.runtime.games()?.keys() {
            let _ = service.runtime.refresh(id);
        }
    }
    let address = endpoint(&data_dir)?;
    let mut listener = Listener::bind(&address)?;
    println!(
        "{}",
        serde_json::json!({"ready": true, "endpoint": address, "host_id": service.host_id})
    );
    let monitor_service = service.clone();
    let background = tokio::spawn(async move {
        let mut monitor = Monitor::default();
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        let mut last_scan = std::time::Instant::now();
        while !monitor_service.stopping.load(Ordering::SeqCst) {
            interval.tick().await;
            let service = monitor_service.clone();
            let scan_due = last_scan.elapsed() >= Duration::from_secs(15 * 60);
            if scan_due {
                last_scan = std::time::Instant::now();
            }
            let result = tokio::task::spawn_blocking(move || {
                if !service.options.no_monitor {
                    match NativeObserver.observe() {
                        Ok(observation) => {
                            let activity = monitor.observe(&service.runtime.games()?, observation);
                            service.runtime.record_activity(
                                activity.stack,
                                activity.started,
                                activity.closed,
                            )?;
                        }
                        Err(error) => eprintln!("monitor: {error}"),
                    }
                }
                if scan_due
                    && !service.options.no_scan
                    && let Err(error) = service.scan()
                {
                    eprintln!("scan: {error}");
                }
                Ok::<_, Error>(monitor)
            })
            .await;
            match result {
                Ok(Ok(next)) => monitor = next,
                error => {
                    eprintln!("background task failed: {error:?}");
                    break;
                }
            }
        }
    });
    let mut connections = tokio::task::JoinSet::new();
    let mut desktop_tick = tokio::time::interval(Duration::from_millis(30));
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("console shutdown handler unavailable: {error}");
            std::future::pending::<()>().await;
        }
    };
    tokio::pin!(interrupt);
    loop {
        tokio::select! {
            _ = desktop_tick.tick() => {
                while let Ok(event) = desktop_rx.try_recv() {
                    match event {
                        DesktopEvent::Open => {
                            let executable = service.options.desktop.clone().unwrap_or(std::env::current_exe()?.with_file_name("savescummer-desktop.exe"));
                            let mut command = std::process::Command::new(executable);
                            command.arg("--data-dir").arg(&data_dir);
                            #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
                            match command.spawn() {
                                Ok(mut child) => { std::thread::spawn(move || { let _ = child.wait(); }); }
                                Err(error) => service.notifications.failure(format!("Cannot open main window: {error}")),
                            }
                        }
                        event => {
                            let command = match event {
                                DesktopEvent::Save => Command::ExecuteActive { action: ShortcutAction::Save },
                                DesktopEvent::Load => Command::ExecuteActive { action: ShortcutAction::Load },
                                DesktopEvent::Exit => Command::Shutdown,
                                DesktopEvent::Open => unreachable!(),
                            };
                            let service = service.clone();
                            tokio::task::spawn_blocking(move || service.handle(&Request { version: VERSION, request_id: new_id(), command }));
                        }
                    }
                }
            }
            accepted = listener.accept() => {
                let stream = accepted?; let service = service.clone();
                connections.spawn(async move { if let Err(error) = connection(stream, service).await { eprintln!("client: {error}"); } });
            }
            _ = service.shutdown.notified() => break,
            _ = &mut interrupt => {
                let _admission = service.admission.lock().map_err(|_| anyhow::anyhow!("admission lock poisoned"))?;
                service.stopping.store(true, Ordering::SeqCst);
                break;
            }
            _ = connections.join_next(), if !connections.is_empty() => (),
        }
    }
    service.stopping.store(true, Ordering::SeqCst);
    while service.runtime.has_pending_operations()? {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    background.await?;
    // Give the shutdown requester its response; malformed/idle clients have a timeout.
    while connections.join_next().await.is_some() {}
    drop(listener);
    drop(desktop);
    drop(instance);
    Ok(())
}
