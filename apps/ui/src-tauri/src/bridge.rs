//! The webview sees a small command surface. Only this layer speaks the host
//! protocol or reads artwork; the host still owns every game decision.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use savescummer_ipc::{
    Client, Command, ConnectError, EventBody, GameSummary, OpenTarget, PROTOCOL_VERSION, Response, State,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

#[derive(Clone)]
struct Bridge {
    endpoint: String,
    data_dir: PathBuf,
    explicit_data_dir: bool,
    restart_env: Option<PathBuf>,
    restart_no_integrations: bool,
    restart_no_catalog_update: bool,
    selection: Arc<Mutex<Option<String>>>,
    focused: Arc<AtomicBool>,
    shortcut_capture: Arc<AtomicBool>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiRequest {
    State,
    AckTrayDialog {
        id: String,
    },
    History {
        game: String,
        cursor: Option<String>,
        limit: Option<usize>,
    },
    Save {
        game: String,
    },
    Retry {
        game: String,
    },
    Play {
        game: String,
    },
    CloseGame {
        game: String,
    },
    RequestAccess {
        game: String,
    },
    OpenAccessSettings {
        game: String,
    },
    Load {
        game: String,
        checkpoint: Option<String>,
    },
    Revert {
        game: String,
        checkpoint: String,
    },
    Delete {
        game: String,
        checkpoint: String,
    },
    SetLabel {
        checkpoint: String,
        label: Option<String>,
    },
    Scan,
    Outcome {
        operation: String,
    },
    AddGame {
        name: String,
        executable: String,
        save_location: String,
    },
    Configure {
        game: String,
        name: Option<String>,
        executable: Option<String>,
        save_location: Option<String>,
        reset_executable: bool,
        reset_save_location: bool,
        expert_mode: Option<bool>,
    },
    SaveSet {
        game: String,
    },
    Settings {
        play_sounds: Option<bool>,
        launch_on_startup: Option<bool>,
        save_shortcut: Option<String>,
        load_shortcut: Option<String>,
        flush_old_checkpoints: Option<bool>,
    },
    FlushPreview {
        game: String,
        cursor: Option<String>,
        limit: Option<usize>,
    },
    Flush {
        game: String,
    },
    OpenCheckpoints {
        game: String,
        /// Only return the folder (Configure shows it); open nothing.
        #[serde(default)]
        resolve_only: bool,
    },
    OpenExecutable {
        game: String,
    },
    OpenSaves {
        game: String,
        target: usize,
    },
    PickerStart {
        path: String,
    },
    RequestOnboardingPermission {
        session: String,
        row: String,
    },
    FinishOnboarding {
        session: String,
    },
}

impl UiRequest {
    fn command(self) -> Command {
        match self {
            Self::State => Command::State,
            Self::AckTrayDialog { id } => Command::AckTrayDialog { id },
            Self::History { game, cursor, limit } => {
                Command::History { game, cursor, limit: limit.map(|n| n.min(200)) }
            }
            Self::Retry { game } => Command::Retry { game },
            Self::Play { game } => Command::Play { game },
            Self::CloseGame { game } => Command::CloseGame { game },
            Self::RequestAccess { game } => Command::RequestAccess { game },
            Self::OpenAccessSettings { game } => {
                Command::Open { target: OpenTarget::AccessSettings { game }, resolve_only: false }
            }
            Self::Save { game } => Command::Save { game, label: None },
            Self::Load { game, checkpoint } => Command::Load { game, checkpoint },
            Self::Revert { game, checkpoint } => Command::Revert { game, checkpoint },
            Self::Delete { game, checkpoint } => Command::Delete { game, checkpoint },
            Self::SetLabel { checkpoint, label } => Command::SetLabel { checkpoint, label },
            Self::Scan => Command::Scan { full: false },
            Self::Outcome { operation } => Command::Outcome { operation, wait: true },
            Self::AddGame { name, executable, save_location } => Command::AddGame { name, executable, save_location },
            Self::Configure {
                game,
                name,
                executable,
                save_location,
                reset_executable,
                reset_save_location,
                expert_mode,
            } => Command::Configure {
                game,
                name,
                executable,
                save_location,
                reset_executable,
                reset_save_location,
                expert_mode,
            },
            Self::SaveSet { game } => Command::SaveSet { game },
            Self::Settings { play_sounds, launch_on_startup, save_shortcut, load_shortcut, flush_old_checkpoints } => {
                Command::Settings {
                    play_sounds,
                    launch_on_startup,
                    save_shortcut,
                    load_shortcut,
                    flush_old_checkpoints,
                }
            }
            Self::FlushPreview { game, cursor, limit } => {
                Command::FlushPreview { game, cursor, limit: limit.map(|n| n.min(200)) }
            }
            Self::Flush { game } => Command::Flush { game },
            Self::OpenCheckpoints { game, resolve_only } => {
                Command::Open { target: OpenTarget::Checkpoints { game }, resolve_only }
            }
            Self::OpenExecutable { game } => {
                Command::Open { target: OpenTarget::Executable { game }, resolve_only: false }
            }
            Self::OpenSaves { game, target } => {
                Command::Open { target: OpenTarget::TargetRoot { game, target }, resolve_only: false }
            }
            Self::PickerStart { path } => Command::PickerStart { path },
            Self::RequestOnboardingPermission { session, row } => Command::RequestOnboardingPermission { session, row },
            Self::FinishOnboarding { session } => Command::FinishOnboarding { session },
        }
    }
}

#[derive(Serialize)]
struct ArtworkData {
    mime: &'static str,
    bytes: Vec<u8>,
}

#[tauri::command]
async fn host_request(bridge: tauri::State<'_, Bridge>, request: UiRequest) -> Result<Response, String> {
    let endpoint = bridge.endpoint.clone();
    tauri::async_runtime::spawn_blocking(move || request_host(&endpoint, request.command()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn report_ui(bridge: tauri::State<'_, Bridge>, selected: Option<String>, focused: bool) -> Result<(), String> {
    *bridge.selection.lock().map_err(|e| e.to_string())? = selected.clone();
    bridge.focused.store(focused, Ordering::Relaxed);
    let endpoint = bridge.endpoint.clone();
    tauri::async_runtime::spawn_blocking(move || {
        request_host(&endpoint, Command::UiReport { focused, visible: true, selected, capturing_shortcut: None })
            .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_shortcut_capture(bridge: tauri::State<'_, Bridge>, capturing: bool) -> Result<(), String> {
    bridge.shortcut_capture.store(capturing, Ordering::Relaxed);
    let selected = bridge.selection.lock().map_err(|e| e.to_string())?.clone();
    let focused = bridge.focused.load(Ordering::Relaxed);
    let endpoint = bridge.endpoint.clone();
    tauri::async_runtime::spawn_blocking(move || {
        request_host(
            &endpoint,
            Command::UiReport { focused, visible: true, selected, capturing_shortcut: Some(capturing) },
        )
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens the product website in the default browser. The URL is fixed here, so the webview can't open arbitrary links.
#[tauri::command]
fn open_website() -> Result<(), String> {
    savescummer_platform::open_folder(Path::new("https://savescummer.app/")).map_err(|e| e.to_string())
}

/// Asks Google or ChatGPT where `game` keeps its saves. Only the game name comes from the webview; the sites are fixed here.
#[tauri::command]
fn open_save_search(engine: String, game: String) -> Result<(), String> {
    let url = save_search_url(&engine, &game).ok_or_else(|| format!("unknown search engine {engine}"))?;
    savescummer_platform::open_folder(Path::new(&url)).map_err(|e| e.to_string())
}

fn save_search_url(engine: &str, game: &str) -> Option<String> {
    let platform = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    };
    let question = format!("What is the save game location of {game} on {platform}");
    let encoded: String = question
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect();
    match engine {
        "google" => Some(format!("https://www.google.com/search?q={encoded}")),
        "chatgpt" => Some(format!("https://chatgpt.com/?prompt={encoded}")),
        _ => None,
    }
}

#[tauri::command]
async fn artwork(bridge: tauri::State<'_, Bridge>, game: String, kind: String) -> Result<ArtworkData, String> {
    let bridge = bridge.inner().clone();
    tauri::async_runtime::spawn_blocking(move || read_artwork(&bridge, &game, &kind))
        .await
        .map_err(|e| e.to_string())?
}

fn request_host(endpoint: &str, command: Command) -> Result<Response, String> {
    let mut client = Client::connect(endpoint).map_err(|e| e.to_string())?;
    let response = client.request(None, command).map_err(|e| e.to_string())?;
    if response.v != PROTOCOL_VERSION {
        return Err(format!("host protocol version {} is unsupported", response.v));
    }
    Ok(response)
}

fn read_artwork(bridge: &Bridge, game: &str, kind: &str) -> Result<ArtworkData, String> {
    let response = request_host(&bridge.endpoint, Command::State)?;
    let value = accepted(response)?;
    let state: State = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let game: &GameSummary = state.game(game).ok_or("game is no longer available")?;
    let art = game.artwork.as_ref().ok_or("game has no artwork")?;
    let path = match kind {
        "hero" => art.hero.as_ref(),
        "logo" => art.logo.as_ref(),
        "header" => art.header.as_ref(),
        "icon" => art.icon.as_ref(),
        _ => return Err("unsupported artwork kind".into()),
    }
    .ok_or("artwork is not available")?;
    let root = if bridge.explicit_data_dir {
        bridge.data_dir.join("cache/artwork")
    } else {
        savescummer_platform::cache_dir().join("artwork")
    };
    let file = verified_artwork_path(&root, Path::new(path))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size > 16 * 1024 * 1024 {
        return Err("artwork is too large".into());
    }
    let mime = match file.extension().and_then(|e| e.to_str()) {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        _ => return Err("unsupported artwork format".into()),
    };
    Ok(ArtworkData { mime, bytes: std::fs::read(file).map_err(|e| e.to_string())? })
}

fn verified_artwork_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let file = path.canonicalize().map_err(|e| e.to_string())?;
    if !file.starts_with(&root) {
        return Err("artwork is outside the host cache".into());
    }
    Ok(file)
}

fn accepted(response: Response) -> Result<serde_json::Value, String> {
    if response.ok {
        response.result.ok_or("host response had no result".into())
    } else {
        Err(response.error.map_or_else(|| "host rejected the request".into(), |e| format!("{e}")))
    }
}

fn watch_host(app: tauri::AppHandle, bridge: Bridge) {
    std::thread::spawn(move || {
        let mut start_attempted = false;
        loop {
            match Client::connect(&bridge.endpoint) {
                Ok(mut client) => {
                    start_attempted = false;
                    let connected = (|| -> Result<(), String> {
                        accepted(
                            client
                                .request(None, Command::Hello { client: "SaveScummer.UI".into() })
                                .map_err(|e| e.to_string())?,
                        )?;
                        let selected = bridge.selection.lock().map_err(|e| e.to_string())?.clone();
                        let focused = bridge.focused.load(Ordering::Relaxed);
                        let capturing_shortcut = bridge.shortcut_capture.load(Ordering::Relaxed);
                        accepted(
                            client
                                .request(
                                    None,
                                    Command::UiReport {
                                        focused,
                                        visible: true,
                                        selected,
                                        capturing_shortcut: Some(capturing_shortcut),
                                    },
                                )
                                .map_err(|e| e.to_string())?,
                        )?;
                        accepted(client.request(None, Command::Watch).map_err(|e| e.to_string())?)?;
                        app.emit("host-status", "connected").ok();
                        while let Some(event) = client.next_event().map_err(|e| e.to_string())? {
                            if event.v != PROTOCOL_VERSION {
                                return Err(format!("host protocol version {} is unsupported", event.v));
                            }
                            match event.body {
                                EventBody::State { state } => {
                                    app.emit("host-state", state).ok();
                                }
                                EventBody::Labels { game } => {
                                    app.emit("host-labels", game).ok();
                                }
                                EventBody::ShowWindow => {
                                    if let Some(window) = app.get_webview_window("main") {
                                        window.show().ok();
                                        window.unminimize().ok();
                                        window.set_focus().ok();
                                    }
                                }
                                EventBody::Shutdown => {
                                    app.exit(0);
                                    return Ok(());
                                }
                            }
                        }
                        Err("host connection closed".into())
                    })();
                    if let Err(error) = connected {
                        app.emit("host-status", format!("reconnecting: {error}")).ok();
                    }
                }
                Err(ConnectError::NotRunning) => {
                    app.emit("host-status", "reconnecting").ok();
                    if !start_attempted {
                        start_attempted = true;
                        if let Err(error) = start_host(&bridge) {
                            app.emit("host-status", format!("reconnecting: {error}")).ok();
                        }
                    }
                }
                Err(ConnectError::Io(error)) => {
                    app.emit("host-status", format!("reconnecting: {error}")).ok();
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

fn start_host(bridge: &Bridge) -> Result<(), String> {
    if bridge.restart_env.is_none()
        && bridge.data_dir.parent().is_some_and(|parent| parent.join(".savescummer-demo").is_file())
    {
        return Err("start the demo UI from its host so the simulated environment is retained".into());
    }
    let exe = if let Some(path) = std::env::var_os("SAVESCUMMER_HOST_EXE") {
        PathBuf::from(path)
    } else {
        let dir = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("no UI executable folder")?
            .to_path_buf();
        ["SaveScummer", "savescummer-host"]
            .iter()
            .map(|name| dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
            .find(|path| path.is_file())
            .ok_or("can't find the host next to SaveScummer.UI")?
    };
    let mut command =
        savescummer_platform::process::bundled_host_command(&exe).unwrap_or_else(|| ProcessCommand::new(&exe));
    // Inside the AppImage, never hand the bundled GTK and WebKit to the host.
    savescummer_platform::process::outer_env(&mut command);
    command.arg("--minimized");
    if bridge.explicit_data_dir {
        command.arg("--data-dir").arg(&bridge.data_dir);
    }
    if let Some(env) = &bridge.restart_env {
        command.arg("--env").arg(env);
    }
    if bridge.restart_no_integrations {
        command.arg("--no-integrations");
    }
    if bridge.restart_no_catalog_update {
        command.arg("--no-catalog-update");
    }
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    savescummer_platform::process::detach(&mut command);
    command.spawn().map_err(|e| format!("can't start {}: {e}", exe.display()))?;
    Ok(())
}

fn bridge() -> Bridge {
    let args: Vec<String> = std::env::args().collect();
    let data_dir = args.windows(2).find(|pair| pair[0] == "--data-dir").map(|pair| PathBuf::from(&pair[1]));
    let restart_env = args.windows(2).find(|pair| pair[0] == "--restart-env").map(|pair| PathBuf::from(&pair[1]));
    let explicit_data_dir = data_dir.is_some();
    let data_dir = data_dir.unwrap_or_else(savescummer_platform::data_dir);
    Bridge {
        endpoint: savescummer_ipc::endpoint(&data_dir),
        data_dir,
        explicit_data_dir,
        restart_env,
        restart_no_integrations: args.iter().any(|arg| arg == "--restart-no-integrations"),
        restart_no_catalog_update: args.iter().any(|arg| arg == "--restart-no-catalog-update"),
        selection: Arc::new(Mutex::new(None)),
        focused: Arc::new(AtomicBool::new(true)),
        shortcut_capture: Arc::new(AtomicBool::new(false)),
    }
}

pub fn run() {
    // The window's Wayland app id (and X11 class) is GTK's program name, the
    // binary's by default. Naming it for the desktop entry lets the dock find
    // the entry, and so the icon, without relying on `StartupWMClass`.
    #[cfg(target_os = "linux")]
    glib::set_prgname(Some("com.savescummer.SaveScummer"));
    let bridge = bridge();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(bridge.clone())
        .invoke_handler(tauri::generate_handler![
            host_request,
            report_ui,
            set_shortcut_capture,
            artwork,
            open_website,
            open_save_search
        ])
        .setup(move |app| {
            watch_host(app.handle().clone(), bridge);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("could not start SaveScummer.UI");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::io::{BufRead, BufReader, Write};

    #[test]
    fn save_search_keeps_the_game_name_inside_the_query() {
        let url = save_search_url("google", "Tom & Jerry?x=1").unwrap();
        assert!(url.starts_with("https://www.google.com/search?q=What%20is%20the%20save%20game%20location%20of%20Tom%20%26%20Jerry%3Fx%3D1%20on%20"));
        assert!(save_search_url("chatgpt", "A").unwrap().starts_with("https://chatgpt.com/?prompt="));
        assert!(save_search_url("https://evil.example/", "A").is_none());
    }

    #[test]
    fn artwork_must_resolve_inside_the_cache() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("artwork");
        std::fs::create_dir(&cache).unwrap();
        let allowed = cache.join("hero.jpg");
        let outside = temp.path().join("private.jpg");
        std::fs::write(&allowed, b"ok").unwrap();
        std::fs::write(&outside, b"private").unwrap();
        assert_eq!(verified_artwork_path(&cache, &allowed).unwrap(), allowed.canonicalize().unwrap());
        assert!(verified_artwork_path(&cache, &outside).is_err());
        assert!(verified_artwork_path(&cache, &cache.join("../private.jpg")).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn request_matches_its_reply_even_when_an_event_arrives_first() {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = temp.path().join("fake.sock");
        let listener = std::os::unix::net::UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: savescummer_ipc::Request = serde_json::from_str(&line).unwrap();
            writeln!(stream, "{}", serde_json::json!({ "v": PROTOCOL_VERSION, "event": "show_window" })).unwrap();
            let response =
                serde_json::json!({ "v": PROTOCOL_VERSION, "re": request.id, "ok": true, "result": { "ready": true } });
            writeln!(stream, "{response}").unwrap();
        });
        let response = request_host(endpoint.to_str().unwrap(), Command::State).unwrap();
        assert_eq!(response.result.unwrap()["ready"], true);
        server.join().unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn a_version_mismatch_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = temp.path().join("fake.sock");
        let listener = std::os::unix::net::UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: savescummer_ipc::Request = serde_json::from_str(&line).unwrap();
            writeln!(
                stream,
                "{}",
                serde_json::json!({ "v": PROTOCOL_VERSION + 1, "re": request.id, "ok": true, "result": {} })
            )
            .unwrap();
        });
        assert!(request_host(endpoint.to_str().unwrap(), Command::State).unwrap_err().contains("version"));
        server.join().unwrap();
    }
}
