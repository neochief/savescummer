//! Protocol messages. Plain data: stable opaque ids, UTC timestamps, no UI
//! concepts, structured errors with no user-facing wording.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use savescummer_core::history::RowKind;
pub use savescummer_core::{AccessInfo, ErrorKind, Failure, Filter, Presence};

// ---- requests -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub v: u32,
    /// Client-made request id. Repeating it for an operation, even after a
    /// host restart, returns the same operation instead of running it twice.
    pub id: String,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    Save,
    Load,
}

/// What a client can ask. `game` accepts a game id, or a name that matches
/// exactly one game (case-insensitive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Hello {
        client: String,
    },
    // Queries
    State,
    Watch,
    History {
        game: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    FlushPreview {
        game: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    Outcome {
        operation: String,
        /// Wait until the operation finishes before answering.
        #[serde(default)]
        wait: bool,
    },
    SaveSet {
        game: String,
    },
    Catalog,
    HotkeyTarget,
    /// When the host was running, newest first (diagnostics).
    HostRuns {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    // Operations
    Save {
        game: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Load {
        game: String,
        /// An exact saved checkpoint ("Load this save"); the latest when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint: Option<String>,
    },
    Revert {
        game: String,
        /// The recovery checkpoint of a Loaded or Reverted row.
        checkpoint: String,
    },
    Delete {
        game: String,
        checkpoint: String,
    },
    Flush {
        game: String,
    },
    MoveStore {
        path: String,
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
    // Not operations
    SetLabel {
        checkpoint: String,
        #[serde(default)]
        label: Option<String>,
    },
    AddGame {
        name: String,
        executable: String,
        save_location: String,
    },
    Configure {
        game: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executable: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        save_location: Option<String>,
        /// Known games: return the executable to the catalog's.
        #[serde(default)]
        reset_executable: bool,
        /// Known games: return the save set to the catalog's.
        #[serde(default)]
        reset_save_location: bool,
        /// Refuse Save, Load and Revert while the game runs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expert_mode: Option<bool>,
    },
    Scan {
        /// A full scan re-checks every checkpoint on disk too.
        #[serde(default)]
        full: bool,
    },
    Settings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        play_sounds: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        launch_on_startup: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        save_shortcut: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        load_shortcut: Option<String>,
        /// Delete checkpoints older than 30 days, of every game.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flush_old_checkpoints: Option<bool>,
    },
    /// The UI's focus and selection, so hotkeys act on the selected game
    /// while the window is focused. A connection that sends it and watches
    /// is the UI's.
    UiReport {
        focused: bool,
        #[serde(default)]
        visible: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capturing_shortcut: Option<bool>,
    },
    Open {
        target: OpenTarget,
        /// Only resolve the folder and return it; open nothing.
        #[serde(default)]
        resolve_only: bool,
    },
    /// Where a file picker starts for a path the user typed or the host
    /// stored: the entry itself when it exists, else the nearest existing
    /// folder above it (for a pattern, above the wildcard). `exists` in the
    /// answer says it's the entry itself. Opens nothing.
    PickerStart {
        path: String,
    },
    CatalogRefresh,
    /// Show the UI: bring the connected one to the front, or start one.
    /// What a second launch of the app and the tray's Main window send.
    ShowUi,
    /// Runs exactly what a hotkey press runs, sounds included.
    Hotkey {
        action: HotkeyAction,
    },
    /// Asks macOS for access to where a waiting game lives (the UI's Allow
    /// access). Answers once the user has.
    RequestAccess {
        game: String,
    },
    Shutdown,
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::Hello { .. } => "hello",
            Command::State => "state",
            Command::Watch => "watch",
            Command::History { .. } => "history",
            Command::FlushPreview { .. } => "flush_preview",
            Command::Outcome { .. } => "outcome",
            Command::SaveSet { .. } => "save_set",
            Command::Catalog => "catalog",
            Command::HotkeyTarget => "hotkey_target",
            Command::HostRuns { .. } => "host_runs",
            Command::Save { .. } => "save",
            Command::Load { .. } => "load",
            Command::Revert { .. } => "revert",
            Command::Delete { .. } => "delete",
            Command::Flush { .. } => "flush",
            Command::MoveStore { .. } => "move_store",
            Command::Retry { .. } => "retry",
            Command::Play { .. } => "play",
            Command::CloseGame { .. } => "close_game",
            Command::SetLabel { .. } => "set_label",
            Command::AddGame { .. } => "add_game",
            Command::Configure { .. } => "configure",
            Command::Scan { .. } => "scan",
            Command::Settings { .. } => "settings",
            Command::UiReport { .. } => "ui_report",
            Command::Open { .. } => "open",
            Command::PickerStart { .. } => "picker_start",
            Command::CatalogRefresh => "catalog_refresh",
            Command::ShowUi => "show_ui",
            Command::Hotkey { .. } => "hotkey",
            Command::RequestAccess { .. } => "request_access",
            Command::Shutdown => "shutdown",
        }
    }
}

/// What to open, by what it is. Clients never send paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenTarget {
    /// The folder a target covers: its exact entry, or for a pattern the
    /// folder before the wildcard.
    TargetRoot {
        game: String,
        #[serde(default)]
        target: usize,
    },
    /// The game's folder in the checkpoint store.
    Checkpoints { game: String },
    /// A checkpoint or a kept recovery checkpoint.
    Checkpoint { checkpoint: String },
    /// The folder holding the game's executable.
    Executable { game: String },
    /// The system privacy page chosen by the host for this game.
    AccessSettings { game: String },
}

// ---- responses and events -------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub v: u32,
    pub re: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Failure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventBody {
    /// The full current state; each one stands on its own.
    State { state: Box<State> },
    /// A game's labels changed: re-read shown history pages in place.
    Labels { game: String },
    /// For the UI: the user asked to see the app, so come to the front.
    ShowWindow,
    /// The last message before the host exits.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    #[serde(flatten)]
    pub body: EventBody,
}

/// Anything the host sends.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Response(Response),
    Event(Event),
}

impl Incoming {
    pub fn parse(line: &str) -> Result<Incoming, String> {
        let value: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if value.get("re").is_some() {
            serde_json::from_value(value).map(Incoming::Response).map_err(|e| e.to_string())
        } else {
            serde_json::from_value(value).map(Incoming::Event).map_err(|e| e.to_string())
        }
    }
}

// ---- results ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub host_version: String,
    pub instance: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    /// Preparing under a reservation, before durable acceptance. Only in state.busy.
    Checking,
    Accepted,
    Running,
    /// A Delete waiting for the game's turn.
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl OpStatus {
    pub fn is_final(self) -> bool {
        matches!(self, OpStatus::Succeeded | OpStatus::Failed | OpStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpResult {
    /// Save: the new checkpoint. Load/Revert: the checkpoint restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
    /// Load/Revert: the recovery checkpoint kept first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
    /// Load/Revert: live files removed (kept in the recovery point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_files: Option<u32>,
    /// Flush and store moves: how many checkpoints were handled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Flush: the deletions that failed; those checkpoints stay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    pub kind: String,
    pub status: OpStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Failure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<OpResult>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Ready,
    ShuttingDown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<ErrorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

impl Availability {
    pub fn yes() -> Availability {
        Availability { available: true, reason: None, failure: None }
    }

    pub fn no(reason: ErrorKind) -> Availability {
        Availability { available: false, reason: Some(reason), failure: None }
    }
}

/// Stable guidance selected by the host, independent of a temporary operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidanceKind {
    Blocked,
    AccessNeeded,
    NoSaveLocation,
    InvalidTarget,
    TargetUnavailable,
    RunningPlayFirst,
    RunningLoad,
    RunningSaveFirst,
    RunningSaveOrLoad,
    PlayFirst,
    NoGameData,
    NoSaves,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Remedy {
    Configure,
    RequestAccess,
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guidance {
    pub kind: GuidanceKind,
    pub save: bool,
    pub load: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<Remedy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointBrief {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameKind {
    Known,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameSummary {
    pub id: String,
    pub name: String,
    pub kind: GameKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    /// Tells two installs of one game apart ("Steam", "GOG").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    pub installed: bool,
    pub running: bool,
    #[serde(default)]
    pub can_play: bool,
    #[serde(default)]
    pub can_close: bool,
    /// Enables Save, Load, Revert and closing the game while it runs.
    /// Off by default.
    #[serde(default)]
    pub expert_mode: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// The executable is the user's choice rather than the catalog's.
    #[serde(default)]
    pub executable_overridden: bool,
    pub save: Availability,
    pub load: Availability,
    /// Game-wide gates; history rows carry checkpoint eligibility separately.
    pub restore: Availability,
    pub delete: Availability,
    pub flush: Availability,
    pub configure: Availability,
    pub retry: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<Guidance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<Failure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<AccessInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<CheckpointBrief>,
    pub has_history: bool,
    /// Bytes a Flush would delete; unknown while the store is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoints_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy: Option<Operation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<Failure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_result: Option<Operation>,
    /// `save_interrupted`: the last save was interrupted and not completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// Bumps when the game's labels change.
    pub labels_version: u64,
    /// Cached Steam art, as local files; absent for custom games and games
    /// without art.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<Artwork>,
    /// Bumps when the game's visible history changes.
    pub history_version: u64,
}

/// A game's art in the host's cache, scaled for display. Each file is
/// complete whenever it is named here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artwork {
    /// Hero art: the sidebar card's background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    /// The fallback background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// The small square icon beside the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsInfo {
    pub play_sounds: bool,
    #[serde(default)]
    pub save_shortcut: String,
    #[serde(default)]
    pub load_shortcut: String,
    pub launch_on_startup: bool,
    pub launch_on_startup_available: bool,
    /// macOS: the user turned launch at login off in System Settings; only
    /// they can turn it on again there (Login Items).
    #[serde(default)]
    pub launch_on_startup_needs_approval: bool,
    /// Checkpoints older than 30 days are deleted, installed games or not.
    pub flush_old_checkpoints: bool,
    pub checkpoint_store: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanOrigin {
    User,
    Background,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub origin: ScanOrigin,
    pub full: bool,
    /// Known games found for the first time by this scan.
    pub new_games: usize,
    pub finished_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<ScanOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_full: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_user: Option<ScanResult>,
    pub scans: u64,
    pub full_scans: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreInfo {
    pub path: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub instance: String,
    pub revision: u64,
    pub host_version: String,
    pub phase: Phase,
    pub settings: SettingsInfo,
    pub store: StoreInfo,
    pub scan: ScanInfo,
    /// Running games, top first.
    pub active_stack: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey_target: Option<String>,
    pub catalog_revision: String,
    pub games: Vec<GameSummary>,
    /// Deletes waiting or running.
    pub deletes: Vec<Operation>,
}

impl State {
    pub fn game(&self, id: &str) -> Option<&GameSummary> {
        self.games.iter().find(|g| g.id == id)
    }
}

/// Checkpoint-specific eligibility. Clients also apply the live game's gates
/// and pending deletion state; activity does not invalidate a history page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowActions {
    /// Saved rows: Load this save.
    pub load: bool,
    /// Loaded and Reverted rows.
    pub revert: bool,
    pub delete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub kind: RowKind,
    pub at: String,
    /// Saved: its checkpoint. Loaded/Reverted: the recovery checkpoint the
    /// row's Revert restores (and Delete removes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
    /// Loaded/Reverted: the checkpoint that was restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<String>,
    /// The label and time of the save the row is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<String>,
    /// Reverted: the time of the row it reverted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_files: Option<u32>,
    #[serde(default)]
    pub cloud_replaced: bool,
    /// The checkpoint exists but can't be used now (another save set, the
    /// store or a folder unreadable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<ErrorKind>,
    /// A delete is underway for this row.
    #[serde(default)]
    pub deleting: bool,
    pub actions: RowActions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub rows: Vec<HistoryEntry>,
    /// The next page's position; None at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlushItem {
    pub path: String,
    /// `saved`, `recovery` or `temporary`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlushPreview {
    pub saved: usize,
    pub recovery: usize,
    pub temporary: usize,
    pub size: u64,
    pub items: Vec<FlushItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetInfo {
    pub root: String,
    pub filter: Filter,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<String>,
    pub presence: Presence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveSetInfo {
    /// The catalog's resolved targets (known games).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<Vec<TargetInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_problem: Option<String>,
    /// The user's save location, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// What operations use now.
    pub active: Vec<TargetInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<Failure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogInfo {
    pub repo: String,
    pub revision: String,
    pub games: usize,
    /// `embedded`, `downloaded` or `file`.
    pub source: String,
    /// Whether the host looks for newer catalogs (`--no-catalog-update` turns it off).
    #[serde(default)]
    pub updates: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    /// Why the last look for a newer catalog failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opened {
    pub path: String,
    pub opened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRuns {
    pub runs: Vec<HostRun>,
}

/// One stretch of time the host was running. Time between runs wasn't
/// observed: nothing is known about games started or closed then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRun {
    pub id: String,
    pub started_at: String,
    /// The last time the run is known to have been alive.
    pub last_seen_at: String,
    /// Absent when the run is the current one or didn't end cleanly
    /// (a crash or power loss somewhere after `last_seen_at`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyTargetInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    /// `window` (the UI's selection) or `active` (the active game).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        let request = Request {
            v: crate::PROTOCOL_VERSION,
            id: "r1".into(),
            command: Command::Save { game: "steam-1".into(), label: Some("boss".into()) },
        };
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(text, r#"{"v":2,"id":"r1","type":"save","game":"steam-1","label":"boss"}"#);
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), request);
    }

    #[test]
    fn incoming_messages_are_told_apart() {
        let response = r#"{"v":2,"re":"r1","ok":false,"error":{"kind":"busy","game":"g"}}"#;
        match Incoming::parse(response).unwrap() {
            Incoming::Response(r) => assert_eq!(r.error.unwrap().kind, ErrorKind::Busy),
            other => panic!("{other:?}"),
        }
        assert!(matches!(Incoming::parse(r#"{"v":2,"event":"shutdown"}"#).unwrap(), Incoming::Event(_)));
    }
}

#[cfg(test)]
mod examples {
    use super::*;

    /// Every shared example in `protocol/examples` parses with these types
    /// and survives a round trip.
    #[test]
    fn shared_examples_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/examples");
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).expect("protocol/examples") {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            if name.starts_with("request-") {
                let request: Request = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
                let again: Request = serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
                assert_eq!(request, again, "{name}");
            } else {
                Incoming::parse(text.trim()).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
            seen += 1;
        }
        assert!(seen >= 10);
    }

    #[test]
    fn the_host_runs_example_matches_its_type() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/examples/response-host-runs.json");
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let runs: HostRuns = serde_json::from_value(value["result"].clone()).unwrap();
        assert!(runs.runs[0].current && runs.runs[1].ended_at.is_none());
    }
}
