use crate::HistoryStatus;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub type Id = String;
pub fn new_id() -> Id {
    uuid::Uuid::new_v4().to_string()
}

#[derive(
    Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq, thiserror::Error,
)]
#[error("{code:?}: {message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Busy,
    RecoveryNeeded,
    Unavailable,
    InvalidPath,
    InvalidTarget,
    NotFound,
    Io,
    Storage,
    ConfirmationRequired,
    ShuttingDown,
    InvalidRequest,
    CursorExpired,
    ResponseTooLarge,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Game {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub info: String,
    pub data_dir: PathBuf,
    pub executables: Vec<PathBuf>,
    pub installed: bool,
    pub configuration_error: Option<String>,
    #[serde(default)]
    pub detected_locations: Vec<GameLocation>,
    /// Older configured games are conservatively treated as user overrides.
    #[serde(default = "default_true")]
    pub user_configured: bool,
}
fn default_true() -> bool {
    true
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GameLocation {
    pub data_dir: PathBuf,
    pub executables: Vec<PathBuf>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    Saved,
    Recovery,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    Deleted,
    Changed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Snapshot {
    pub id: Id,
    pub game_id: Id,
    /// Original resolved live directory. Legacy records with no recoverable
    /// origin stay ineligible until rediscovered at a configured location.
    pub original_data_dir: Option<PathBuf>,
    pub registration_order: u64,
    pub path: PathBuf,
    pub identity: String,
    pub fingerprint: String,
    pub removed_at: Option<u64>,
    pub removal_reason: Option<RemovalReason>,
    pub kind: SnapshotKind,
    pub saved_at: Option<u64>,
    /// Estimated ordering timestamp for manual backups; not a claimed save time.
    pub selection_time: u64,
    pub discovered_at: u64,
    /// Last filesystem observation; service state also applies restore eligibility.
    pub available: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    Saved,
    ExistingBackup,
    Loaded,
    Reverted,
    GameStarted,
    GameClosed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct History {
    pub id: Id,
    /// Observation epoch prevents joining sessions across unobserved downtime.
    pub observation_run: Id,
    pub sequence: u64,
    pub game_id: Id,
    pub kind: HistoryKind,
    pub recorded_at: u64,
    pub snapshot_id: Option<Id>,
    pub recovery_id: Option<Id>,
    pub target_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Save,
    Load {
        /// Saved checkpoint ID; None selects the newest eligible checkpoint.
        target: Option<Id>,
    },
    Revert {
        /// Recovery checkpoint ID from a Loaded/Reverted row's recovery_id.
        target: Id,
    },
    Delete {
        /// Exact saved or recovery checkpoint to remove.
        target: Id,
    },
    Flush {
        confirmed_revision: u64,
    },
    Recover {
        operation: Id,
        choice: RecoveryChoice,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryChoice {
    Retry,
    KeepCurrent,
    RestoreBefore,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    RecoveryReady,
    StageReady,
    PublishingSave,
    MovingOriginal,
    OriginalMoved,
    InstallingReplacement,
    ReplacementInstalled,
    Finished,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Pending,
    Completed,
    Failed,
    RecoveryNeeded,
    Resolved,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Operation {
    pub id: Id,
    pub request_id: Id,
    pub game_id: Id,
    pub action: Action,
    pub status: OperationStatus,
    pub phase: Phase,
    pub live: PathBuf,
    pub source: Option<PathBuf>,
    pub source_id: Option<Id>,
    pub source_identity: Option<String>,
    pub source_fingerprint: Option<String>,
    pub target_id: Option<Id>,
    pub snapshot_path: Option<PathBuf>,
    pub recovery: PathBuf,
    pub recovery_staging: PathBuf,
    pub staging: PathBuf,
    pub original: PathBuf,
    pub recovery_complete: bool,
    pub staging_identity: Option<String>,
    pub original_identity: Option<String>,
    pub recovery_identity: Option<String>,
    pub recovery_fingerprint: Option<String>,
    pub started_at: u64,
    pub bytes_copied: u64,
    pub error: Option<Error>,
    pub resolution: Option<String>,
}
impl Operation {
    pub fn blocks(&self) -> bool {
        matches!(
            self.status,
            OperationStatus::Pending | OperationStatus::RecoveryNeeded
        )
    }
    pub fn retained_paths(&self) -> Vec<PathBuf> {
        vec![
            self.recovery.clone(),
            self.recovery_staging.clone(),
            self.staging.clone(),
            self.original.clone(),
        ]
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GameAvailability {
    pub data_available: bool,
    pub default_snapshot_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct Settings {
    pub play_sounds: bool,
    pub launch_on_startup: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            play_sounds: true,
            launch_on_startup: false,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GameArtwork {
    pub steam_app_id: u32,
    /// Only complete, validated images are published here.
    pub icon_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct State {
    #[serde(default)]
    pub history_sequence: u64,
    #[serde(default)]
    pub snapshot_order: u64,
    #[serde(skip)]
    pub clear_history: Vec<Id>,
    #[serde(default)]
    pub history_status: BTreeMap<Id, HistoryStatus>,
    /// Host artwork projection; not persisted or used for operation revisions.
    #[serde(default)]
    pub artwork: BTreeMap<Id, GameArtwork>,
    #[serde(default)]
    pub artwork_revision: u64,
    #[serde(default)]
    pub settings: Settings,
    pub revision: u64,
    pub games: BTreeMap<Id, Game>,
    pub snapshots: BTreeMap<Id, Snapshot>,
    pub history: Vec<History>,
    /// UI projection. `history` retains audit records and stale action IDs.
    pub visible_history: Vec<History>,
    pub operations: BTreeMap<Id, Operation>,
    /// Most recently activated game first.
    pub active_stack: Vec<Id>,
    /// Derived by the host; clients must not inspect game directories or select
    /// default checkpoints themselves. Not persisted by the repository.
    #[serde(default)]
    pub availability: BTreeMap<Id, GameAvailability>,
    #[serde(default)]
    pub discovery_errors: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FlushPreview {
    #[serde(default)]
    pub next_cursor: Option<String>,
    pub revision: u64,
    pub saved: usize,
    pub recovery: usize,
    pub retained: usize,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    Save,
    Load,
}

impl ShortcutAction {
    pub fn action(self) -> Action {
        match self {
            Self::Save => Action::Save,
            Self::Load => Action::Load { target: None },
        }
    }
}

/// A file-manager menu resolves paths to opaque checkpoint IDs before execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplorerTarget {
    pub game_id: Id,
    pub action: Action,
}
