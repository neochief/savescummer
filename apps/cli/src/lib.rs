//! SaveScummer.CLI: a console client for scripts, testing and diagnostics.
//! It can drive the host in nearly every way the UI can, and never touches
//! the database or game files itself.
//!
//! Exit codes tell success, failure and rejection apart:
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | success |
//! | 1 | the operation ran and failed |
//! | 2 | usage error |
//! | 3 | rejected by the host (busy, unavailable, invalid, …) |
//! | 4 | no host: not running (with `--no-start`) or unreachable |

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value;

use savescummer_ipc::{
    Client, Command, ConnectError, EventBody, FlushPreview, HistoryEntry, HistoryPage, HotkeyAction, OpStatus,
    OpenTarget, Operation, Phase, Response, RowKind, State,
};

#[derive(Debug, Parser)]
#[command(name = "SaveScummer.CLI", version, about = "Drive the SaveScummer host from the command line")]
struct Cli {
    /// The host's data folder (development and tests).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Machine-readable output: one JSON value per line.
    #[arg(long, global = true)]
    json: bool,
    /// Fail instead of starting a host.
    #[arg(long, global = true)]
    no_start: bool,
    /// Use this request id (to test repeated requests).
    #[arg(long, global = true)]
    request_id: Option<String>,
    /// Extra arguments for a host this command starts (tests).
    #[arg(long = "host-arg", global = true, hide = true, allow_hyphen_values = true)]
    host_args: Vec<String>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

impl OnOff {
    fn value(self) -> bool {
        matches!(self, OnOff::On)
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OpenKind {
    /// A target's root folder.
    Saves,
    /// The game's folder in the checkpoint store.
    Checkpoints,
    /// One checkpoint or recovery checkpoint.
    Checkpoint,
    /// The folder holding the executable.
    Executable,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Key {
    Save,
    Load,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// The host's current state, summarized.
    Status,
    /// The installed games.
    Games,
    /// Print the current state, then every new state, until stopped.
    Watch,
    /// A game's history, newest first.
    History {
        game: String,
        #[arg(long)]
        limit: Option<usize>,
        /// Continue from a page position.
        #[arg(long)]
        cursor: Option<String>,
        /// Stream every page.
        #[arg(long)]
        all: bool,
    },
    /// Save a checkpoint.
    Save {
        game: String,
        #[arg(long)]
        label: Option<String>,
        /// Return the operation id at once.
        #[arg(long)]
        no_wait: bool,
    },
    /// Load the latest saved checkpoint, or an exact one.
    Load {
        game: String,
        #[arg(long)]
        checkpoint: Option<String>,
        #[arg(long)]
        no_wait: bool,
    },
    /// Put back the state from before a Load or Revert.
    Revert {
        game: String,
        /// The row's recovery checkpoint.
        checkpoint: String,
        #[arg(long)]
        no_wait: bool,
    },
    /// Delete a checkpoint (after the host's countdown).
    Delete {
        game: String,
        checkpoint: String,
        #[arg(long)]
        no_wait: bool,
    },
    /// Cancel a delete countdown.
    CancelDelete { operation: String },
    /// Delete every checkpoint and the history of a game.
    Flush {
        game: String,
        /// Only show what would be deleted.
        #[arg(long)]
        preview: bool,
        /// Confirm the flush.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        no_wait: bool,
    },
    /// Set or clear a saved checkpoint's label.
    Label {
        checkpoint: String,
        label: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// Add a game the catalog doesn't know.
    AddGame {
        #[arg(long)]
        name: String,
        #[arg(long)]
        exe: String,
        /// A folder, file or glob pattern.
        #[arg(long)]
        saves: String,
    },
    /// Change a game's executable, save location or name.
    Configure {
        game: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        exe: Option<String>,
        #[arg(long)]
        saves: Option<String>,
        #[arg(long)]
        reset_exe: bool,
        #[arg(long)]
        reset_saves: bool,
    },
    /// Scan for games (and re-check checkpoints with --full).
    Scan {
        #[arg(long)]
        full: bool,
    },
    /// Change settings.
    Settings {
        #[arg(long)]
        sounds: Option<OnOff>,
        #[arg(long)]
        launch_on_startup: Option<OnOff>,
    },
    /// Move the checkpoint store to another folder.
    MoveStore {
        path: String,
        #[arg(long)]
        no_wait: bool,
    },
    /// Show the UI, as launching the app again does.
    ShowUi,
    /// Stand in for the UI's focus and selection report.
    UiReport {
        #[arg(long)]
        focused: bool,
        #[arg(long)]
        visible: bool,
        #[arg(long)]
        selected: Option<String>,
    },
    /// Open (or only resolve) a folder the UI would open.
    Open {
        kind: OpenKind,
        #[arg(long)]
        game: Option<String>,
        #[arg(long, default_value_t = 0)]
        target: usize,
        #[arg(long)]
        checkpoint: Option<String>,
        /// Print the folder without opening a window.
        #[arg(long)]
        resolve_only: bool,
    },
    /// Resolve a blocked game's interruption again.
    Retry { game: String },
    /// The active catalog, or refresh it.
    Catalog {
        #[arg(long)]
        refresh: bool,
    },
    /// An operation's outcome.
    Outcome {
        operation: String,
        #[arg(long)]
        wait: bool,
    },
    /// A game's save set: the catalog's targets, any override, presence.
    SaveSet { game: String },
    /// Which game the hotkeys would act on.
    HotkeyTarget,
    /// When the host was running, newest first.
    HostRuns {
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Run exactly what a hotkey press runs (sounds included).
    Hotkey {
        key: Key,
        #[arg(long)]
        no_wait: bool,
    },
    /// Stop the host safely.
    Shutdown,
    /// Send one raw protocol line and print the answer (protocol tests).
    #[command(hide = true)]
    Raw { line: String },
}

enum Exit {
    Ok,
    Failed,
    Rejected,
    NoHost,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> ExitCode {
        ExitCode::from(match exit {
            Exit::Ok => 0,
            Exit::Failed => 1,
            Exit::Rejected => 3,
            Exit::NoHost => 4,
        })
    }
}

struct Session {
    client: Client,
    json: bool,
    request_id: Option<String>,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.clone().unwrap_or_else(savescummer_platform::data_dir);
    let endpoint = savescummer_ipc::endpoint(&data_dir);
    let client = match connect(&endpoint, &cli, &data_dir) {
        Ok(client) => client,
        Err(message) => {
            if cli.json {
                println!("{}", serde_json::json!({ "ok": false, "error": { "kind": "no_host", "detail": message } }));
            } else {
                eprintln!("{message}");
            }
            return Exit::NoHost.into();
        }
    };
    let mut session = Session { client, json: cli.json, request_id: cli.request_id.clone() };
    match run(&mut session, cli.command) {
        Ok(exit) => exit.into(),
        Err(e) => {
            eprintln!("lost the connection to the host: {e}");
            Exit::NoHost.into()
        }
    }
}

/// Connects, starting a host from the install folder if none runs. Never a
/// second host: the host itself refuses to start twice.
fn connect(endpoint: &str, cli: &Cli, data_dir: &std::path::Path) -> Result<Client, String> {
    match Client::connect(endpoint) {
        Ok(client) => return Ok(client),
        Err(ConnectError::NotRunning) if !cli.no_start => {}
        Err(ConnectError::NotRunning) => return Err("no host is running".into()),
        Err(ConnectError::Io(e)) => return Err(format!("can't reach the host: {e}")),
    }
    let exe = host_exe().ok_or("can't find SaveScummer next to the CLI")?;
    let mut command = std::process::Command::new(&exe);
    // A command needs a host, not a window.
    command.arg("--minimized");
    if let Some(dir) = &cli.data_dir {
        command.arg("--data-dir").arg(dir);
    }
    command.args(&cli.host_args);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    // The host outlives us.
    savescummer_platform::process::detach(&mut command);
    command.spawn().map_err(|e| format!("can't start {}: {e}", exe.display()))?;
    let _ = data_dir;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match Client::connect(endpoint) {
            Ok(client) => return Ok(client),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("the host didn't start: {e}")),
        }
    }
}

fn host_exe() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SAVESCUMMER_HOST_EXE") {
        return Some(PathBuf::from(path));
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let suffix = std::env::consts::EXE_SUFFIX;
    ["SaveScummer", "savescummer-host"].iter().map(|name| dir.join(format!("{name}{suffix}"))).find(|p| p.exists())
}

impl Session {
    fn send(&mut self, command: Command) -> std::io::Result<Response> {
        let id = self.request_id.take();
        self.client.request(id, command)
    }

    /// Waits until the host accepts operations.
    fn wait_ready(&mut self) -> std::io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let state = self.state()?;
            if state.phase != Phase::Starting || Instant::now() > deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn state(&mut self) -> std::io::Result<State> {
        let response = self.client.request(None, Command::State)?;
        serde_json::from_value(response.result.unwrap_or_default()).map_err(std::io::Error::other)
    }

    /// Prints an answer and returns the exit code it means.
    fn report(&self, response: &Response, readable: impl FnOnce(&Value) -> String) -> Exit {
        if self.json {
            if response.ok {
                println!("{}", response.result.clone().unwrap_or(Value::Null));
            } else {
                println!("{}", serde_json::json!({ "ok": false, "error": response.error }));
            }
        } else if response.ok {
            let text = readable(response.result.as_ref().unwrap_or(&Value::Null));
            if !text.is_empty() {
                println!("{text}");
            }
        } else if let Some(error) = &response.error {
            eprintln!("refused: {error}");
        }
        if response.ok { Exit::Ok } else { Exit::Rejected }
    }

    /// Submits an operation and, unless asked not to, waits for its outcome,
    /// printing each state it passes through.
    fn operation(&mut self, command: Command, no_wait: bool) -> std::io::Result<Exit> {
        self.wait_ready()?;
        let response = self.send(command)?;
        if !response.ok || no_wait {
            return Ok(self.report(&response, |v| {
                let op: Operation = serde_json::from_value(v.clone()).unwrap_or_else(|_| empty_op());
                format!("accepted: {} ({})", op.id, op.kind)
            }));
        }
        let mut op: Operation =
            serde_json::from_value(response.result.unwrap_or_default()).map_err(std::io::Error::other)?;
        let mut last = None;
        loop {
            if last != Some(op.status) {
                last = Some(op.status);
                if self.json && !op.status.is_final() {
                    println!("{}", serde_json::to_string(&op).expect("serializes"));
                } else if !self.json && !op.status.is_final() {
                    eprintln!("{}: {}", op.kind, status_text(op.status));
                }
            }
            if op.status.is_final() {
                break;
            }
            // Poll briefly so countdown states are seen as they happen.
            std::thread::sleep(Duration::from_millis(25));
            let answer = self.client.request(None, Command::Outcome { operation: op.id.clone(), wait: false })?;
            if let Some(value) = answer.result {
                op = serde_json::from_value(value).map_err(std::io::Error::other)?;
            }
        }
        if self.json {
            println!("{}", serde_json::to_string(&op).expect("serializes"));
        } else {
            println!("{}", describe_outcome(&op));
        }
        Ok(match op.status {
            OpStatus::Succeeded | OpStatus::Cancelled => Exit::Ok,
            _ => Exit::Failed,
        })
    }
}

fn empty_op() -> Operation {
    Operation {
        id: String::new(),
        request_id: None,
        game: None,
        kind: String::new(),
        status: OpStatus::Accepted,
        error: None,
        result: None,
        created_at: String::new(),
        finished_at: None,
        remaining_ms: None,
        checkpoint: None,
    }
}

fn status_text(status: OpStatus) -> &'static str {
    match status {
        OpStatus::Accepted => "accepted",
        OpStatus::Running => "running",
        OpStatus::CountingDown => "counting down",
        OpStatus::Waiting => "waiting for the game",
        OpStatus::Succeeded => "done",
        OpStatus::Failed => "failed",
        OpStatus::Cancelled => "cancelled",
    }
}

fn describe_outcome(op: &Operation) -> String {
    match op.status {
        OpStatus::Succeeded => {
            let r = op.result.clone().unwrap_or_default();
            match op.kind.as_str() {
                "save" => format!("saved checkpoint {}", r.checkpoint.unwrap_or_default()),
                "load" | "revert" => {
                    let mut text = format!(
                        "{} {} (kept the previous state as {})",
                        if op.kind == "load" { "loaded" } else { "reverted to" },
                        r.checkpoint.unwrap_or_default(),
                        r.recovery.unwrap_or_default()
                    );
                    if let Some(n) = r.removed_files.filter(|n| *n > 0) {
                        text.push_str(&format!("; removed {n} newer file(s), kept in the recovery point"));
                    }
                    text
                }
                "delete" => "deleted".to_string(),
                "flush" => {
                    let mut text = format!("flushed {} checkpoint(s)", r.count.unwrap_or(0));
                    for f in &r.failures {
                        text.push_str(&format!("\n  not deleted: {f}"));
                    }
                    text
                }
                other => format!("{other}: done"),
            }
        }
        OpStatus::Cancelled => format!("{}: cancelled", op.kind),
        _ => match &op.error {
            Some(e) => format!("{} failed: {e}", op.kind),
            None => format!("{} failed", op.kind),
        },
    }
}

fn run(s: &mut Session, command: Cmd) -> std::io::Result<Exit> {
    match command {
        Cmd::Status => {
            let response = s.send(Command::State)?;
            Ok(s.report(&response, |v| {
                serde_json::from_value::<State>(v.clone()).map(|st| status(&st)).unwrap_or_default()
            }))
        }
        Cmd::Games => {
            let response = s.send(Command::State)?;
            if s.json && response.ok {
                let state: State = serde_json::from_value(response.result.clone().unwrap_or_default())
                    .map_err(std::io::Error::other)?;
                println!("{}", serde_json::to_string(&state.games).expect("serializes"));
                return Ok(Exit::Ok);
            }
            Ok(s.report(&response, |v| {
                serde_json::from_value::<State>(v.clone()).map(|st| games(&st)).unwrap_or_default()
            }))
        }
        Cmd::Watch => {
            let response = s.send(Command::Watch)?;
            if !response.ok {
                return Ok(s.report(&response, |_| String::new()));
            }
            let mut out = std::io::stdout();
            while let Some(event) = s.client.next_event()? {
                match &event.body {
                    EventBody::State { state } if !s.json => {
                        let _ = writeln!(out, "--- revision {} ({:?})\n{}", state.revision, state.phase, status(state));
                    }
                    EventBody::Labels { game } if !s.json => {
                        let _ = writeln!(out, "--- labels changed: {game}");
                    }
                    EventBody::ShowWindow if !s.json => {
                        let _ = writeln!(out, "--- the UI was asked to come to the front");
                    }
                    EventBody::Shutdown if !s.json => {
                        let _ = writeln!(out, "--- the host is shutting down");
                    }
                    _ => {
                        let _ = writeln!(out, "{}", serde_json::to_string(&event).expect("serializes"));
                    }
                }
                let _ = out.flush();
                if matches!(event.body, EventBody::Shutdown) {
                    break;
                }
            }
            Ok(Exit::Ok)
        }
        Cmd::History { game, limit, cursor, all } => {
            let mut position = cursor;
            loop {
                let response = s.send(Command::History { game: game.clone(), cursor: position.clone(), limit })?;
                if !response.ok {
                    return Ok(s.report(&response, |_| String::new()));
                }
                let page: HistoryPage = serde_json::from_value(response.result.clone().unwrap_or_default())
                    .map_err(std::io::Error::other)?;
                if s.json {
                    if all {
                        for row in &page.rows {
                            println!("{}", serde_json::to_string(row).expect("serializes"));
                        }
                    } else {
                        println!("{}", serde_json::to_string(&page).expect("serializes"));
                    }
                } else {
                    for row in &page.rows {
                        println!("{}", history_line(row));
                    }
                    if !all && let Some(next) = &page.next {
                        println!("(more: --cursor {next})");
                    }
                }
                match (all, page.next) {
                    (true, Some(next)) => position = Some(next),
                    _ => return Ok(Exit::Ok),
                }
            }
        }
        Cmd::Save { game, label, no_wait } => s.operation(Command::Save { game, label }, no_wait),
        Cmd::Load { game, checkpoint, no_wait } => s.operation(Command::Load { game, checkpoint }, no_wait),
        Cmd::Revert { game, checkpoint, no_wait } => s.operation(Command::Revert { game, checkpoint }, no_wait),
        Cmd::Delete { game, checkpoint, no_wait } => s.operation(Command::Delete { game, checkpoint }, no_wait),
        Cmd::CancelDelete { operation } => {
            let response = s.send(Command::CancelDelete { operation })?;
            Ok(s.report(&response, |v| {
                let op: Operation = serde_json::from_value(v.clone()).unwrap_or_else(|_| empty_op());
                format!("delete {}: {}", op.id, status_text(op.status))
            }))
        }
        Cmd::Flush { game, preview, yes, no_wait } => {
            if preview || !yes {
                let response = s.send(Command::FlushPreview { game: game.clone(), cursor: None, limit: Some(500) })?;
                let exit = s.report(&response, |v| {
                    let p: FlushPreview = match serde_json::from_value(v.clone()) {
                        Ok(p) => p,
                        Err(_) => return String::new(),
                    };
                    let mut text = format!(
                        "{} saved, {} recovery, {} temporary; {} bytes",
                        p.saved, p.recovery, p.temporary, p.size
                    );
                    for item in &p.items {
                        text.push_str(&format!(
                            "\n  {} {}{}",
                            item.kind,
                            item.path,
                            item.label.as_ref().map(|l| format!(" ({l})")).unwrap_or_default()
                        ));
                    }
                    if !preview {
                        text.push_str("\nrun again with --yes to delete all of it");
                    }
                    text
                });
                return Ok(exit);
            }
            s.operation(Command::Flush { game }, no_wait)
        }
        Cmd::Label { checkpoint, label, clear } => {
            let label = if clear { None } else { label };
            let response = s.send(Command::SetLabel { checkpoint, label })?;
            Ok(s.report(&response, |v| match v.get("label").and_then(|l| l.as_str()) {
                Some(label) => format!("label: {label}"),
                None => "label cleared".into(),
            }))
        }
        Cmd::AddGame { name, exe, saves } => {
            let response = s.send(Command::AddGame { name, executable: exe, save_location: saves })?;
            Ok(s.report(&response, |v| format!("added {}", v.get("game").and_then(|g| g.as_str()).unwrap_or_default())))
        }
        Cmd::Configure { game, name, exe, saves, reset_exe, reset_saves } => {
            let response = s.send(Command::Configure {
                game,
                name,
                executable: exe,
                save_location: saves,
                reset_executable: reset_exe,
                reset_save_location: reset_saves,
            })?;
            Ok(s.report(&response, |_| "configured".into()))
        }
        Cmd::Scan { full } => {
            s.wait_ready()?;
            let response = s.send(Command::Scan { full })?;
            Ok(s.report(&response, |v| {
                format!("scan finished; {} new game(s)", v.get("new_games").and_then(|n| n.as_u64()).unwrap_or(0))
            }))
        }
        Cmd::Settings { sounds, launch_on_startup } => {
            let response = s.send(Command::Settings {
                play_sounds: sounds.map(OnOff::value),
                launch_on_startup: launch_on_startup.map(OnOff::value),
            })?;
            Ok(s.report(&response, |v| v.to_string()))
        }
        Cmd::MoveStore { path, no_wait } => s.operation(Command::MoveStore { path }, no_wait),
        Cmd::ShowUi => {
            let response = s.send(Command::ShowUi)?;
            Ok(s.report(&response, |v| {
                match v["ui"].as_str().unwrap_or_default() {
                    "front" => "asked the open UI to come to the front",
                    "started" => "started the UI",
                    "starting" => "the UI is already starting",
                    _ => "there is no UI to show in this install",
                }
                .into()
            }))
        }
        Cmd::UiReport { focused, visible, selected } => {
            let response = s.send(Command::UiReport { focused, visible, selected })?;
            Ok(s.report(&response, |_| String::new()))
        }
        Cmd::Open { kind, game, target, checkpoint, resolve_only } => {
            let need_game = || game.clone().unwrap_or_default();
            let target = match kind {
                OpenKind::Saves => OpenTarget::TargetRoot { game: need_game(), target },
                OpenKind::Checkpoints => OpenTarget::Checkpoints { game: need_game() },
                OpenKind::Checkpoint => OpenTarget::Checkpoint { checkpoint: checkpoint.clone().unwrap_or_default() },
                OpenKind::Executable => OpenTarget::Executable { game: need_game() },
            };
            let response = s.send(Command::Open { target, resolve_only })?;
            Ok(s.report(&response, |v| v.get("path").and_then(|p| p.as_str()).unwrap_or_default().to_string()))
        }
        Cmd::Retry { game } => {
            let response = s.send(Command::Retry { game })?;
            Ok(s.report(&response, |_| "resolved".into()))
        }
        Cmd::Catalog { refresh } => {
            let response = s.send(if refresh { Command::CatalogRefresh } else { Command::Catalog })?;
            Ok(s.report(&response, |v| {
                let info = v.get("catalog").unwrap_or(v);
                let text = |k: &str| info.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
                let mut out = format!(
                    "{} at {} ({} games, {})",
                    text("repo"),
                    text("revision"),
                    info.get("games").and_then(|g| g.as_u64()).unwrap_or(0),
                    text("source")
                );
                if info.get("updates").and_then(|u| u.as_bool()) != Some(true) {
                    out.push_str("\nupdates are off");
                } else if let Some(checked) = info.get("checked_at").and_then(|c| c.as_str()) {
                    out.push_str(&format!("\nlast checked {checked}"));
                }
                if let Some(problem) = info.get("problem").and_then(|p| p.as_str()) {
                    out.push_str(&format!("\nthe last check failed: {problem}"));
                }
                if v.get("changed").and_then(|c| c.as_bool()) == Some(true) {
                    out.push_str("\nupdated; the library was rescanned");
                }
                out
            }))
        }
        Cmd::Outcome { operation, wait } => {
            let response = s.send(Command::Outcome { operation, wait })?;
            let exit = s.report(&response, |v| {
                serde_json::from_value::<Operation>(v.clone()).map(|op| describe_outcome(&op)).unwrap_or_default()
            });
            Ok(exit)
        }
        Cmd::SaveSet { game } => {
            let response = s.send(Command::SaveSet { game })?;
            Ok(s.report(&response, |v| serde_json::to_string_pretty(v).unwrap_or_default()))
        }
        Cmd::HostRuns { limit } => {
            let response = s.send(Command::HostRuns { limit })?;
            Ok(s.report(&response, |v| {
                let runs = v.get("runs").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                runs.iter()
                    .map(|r| {
                        let text = |k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
                        let end = if r.get("current").and_then(|c| c.as_bool()) == Some(true) {
                            "running now".to_string()
                        } else if let Some(end) = r.get("ended_at").and_then(|e| e.as_str()) {
                            format!("until {end}")
                        } else {
                            format!("until some time after {} (didn't exit cleanly)", text("last_seen_at"))
                        };
                        format!("{} {end}", text("started_at"))
                    })
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    )
            }))
        }
        Cmd::HotkeyTarget => {
            let response = s.send(Command::HotkeyTarget)?;
            Ok(s.report(&response, |v| match v.get("game").and_then(|g| g.as_str()) {
                Some(game) => format!("{game} ({})", v.get("source").and_then(|x| x.as_str()).unwrap_or_default()),
                None => "none".into(),
            }))
        }
        Cmd::Hotkey { key, no_wait } => {
            let action = match key {
                Key::Save => HotkeyAction::Save,
                Key::Load => HotkeyAction::Load,
            };
            s.operation(Command::Hotkey { action }, no_wait)
        }
        Cmd::Shutdown => {
            let response = s.send(Command::Shutdown)?;
            let exit = s.report(&response, |_| "shutting down".into());
            // Wait until the host is gone, so tools can replace its files.
            while s.client.next_event().ok().flatten().is_some() {}
            Ok(exit)
        }
        Cmd::Raw { line } => {
            let value: Value = serde_json::from_str(&line).map_err(std::io::Error::other)?;
            let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let response = s.client.request_raw(&line, &id)?;
            println!("{}", serde_json::to_string(&response).expect("serializes"));
            Ok(if response.ok { Exit::Ok } else { Exit::Rejected })
        }
    }
}

fn status(state: &State) -> String {
    let mut text = format!(
        "host {} ({:?}), catalog {}\ncheckpoints: {}{}\nactive stack: {}\nhotkeys act on: {}\n",
        state.host_version,
        state.phase,
        &state.catalog_revision[..state.catalog_revision.len().min(10)],
        state.store.path,
        if state.store.available { "" } else { " (unavailable)" },
        if state.active_stack.is_empty() { "(none)".to_string() } else { state.active_stack.join(" > ") },
        state.hotkey_target.clone().unwrap_or_else(|| "(none)".into()),
    );
    text.push_str(&games(state));
    for d in &state.deletes {
        text.push_str(&format!(
            "\ndelete {} of {}: {}",
            d.checkpoint.clone().unwrap_or_default(),
            d.game.clone().unwrap_or_default(),
            status_text(d.status)
        ));
    }
    text
}

fn games(state: &State) -> String {
    let mut lines = Vec::new();
    for g in &state.games {
        let tag = g.install_tag.as_ref().map(|t| format!(" [{t}]")).unwrap_or_default();
        let running = if g.running { " running" } else { "" };
        let reason = |a: &savescummer_ipc::Availability| {
            if a.available { "yes".to_string() } else { a.reason.map(|r| r.as_str().to_string()).unwrap_or_default() }
        };
        let latest = g
            .latest
            .as_ref()
            .map(|l| {
                format!(" latest {}{}", l.created_at, l.label.as_ref().map(|x| format!(" \"{x}\"")).unwrap_or_default())
            })
            .unwrap_or_default();
        lines.push(format!(
            "{}{}{}  ({})  save: {}  load: {}{}{}",
            g.name,
            tag,
            running,
            g.id,
            reason(&g.save),
            reason(&g.load),
            latest,
            g.blocked.as_ref().map(|b| format!("  BLOCKED: {}", b.kind.as_str())).unwrap_or_default()
        ));
    }
    if lines.is_empty() { "(no games)".into() } else { lines.join("\n") }
}

fn history_line(row: &HistoryEntry) -> String {
    let kind = match row.kind {
        RowKind::Saved => "Saved",
        RowKind::Loaded => "Loaded",
        RowKind::Reverted => "Reverted",
        RowKind::GameStarted => "Game started",
        RowKind::GameClosed => "Game closed",
    };
    let mut text = format!("{}  {kind}", row.at);
    if let Some(label) = &row.label {
        text.push_str(&format!(" \"{label}\""));
    }
    if let Some(at) = row.saved_at.as_ref().filter(|_| row.kind != RowKind::Saved) {
        text.push_str(&format!(" · {at}"));
    }
    if let Some(n) = row.removed_files.filter(|n| *n > 0) {
        text.push_str(&format!(" · removed {n} newer save(s)"));
    }
    if row.cloud_replaced {
        text.push_str(" · Steam Cloud replaced the restored save");
    }
    if let Some(checkpoint) = &row.checkpoint {
        text.push_str(&format!("  [{checkpoint}]"));
    }
    if let Some(reason) = row.unavailable {
        text.push_str(&format!("  (unavailable: {})", reason.as_str()));
    }
    text
}
