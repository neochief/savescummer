//! Structured failures. Every failure reaches clients as a stable kind, the
//! game and the exact paths involved; wording belongs to the clients.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// A save file is in use, so setting it aside failed (Load stage 2).
    InUse,
    /// A file couldn't be read during a copy.
    ReadFailed,
    /// The destination is full (the store, or a target's drive).
    DiskFull,
    /// Writing was denied (the store, or a target for `.ssnew` copies).
    AccessDenied,
    /// A target's drive or share can't be read: presence unknown.
    TargetUnavailable,
    /// A target contains a link or special file.
    LinkInTarget,
    /// The checkpoint changed or vanished since it was shown.
    CheckpointChanged,
    /// The checkpoint shares no target with the current save set.
    DifferentSaveSet,
    /// A target is invalid, broad, overlapping, or a link that changed.
    InvalidTarget,
    /// The host is shutting down.
    ShuttingDown,
    /// A delete target changed or points at live data.
    DeleteMismatch,
    /// The checkpoint store can't be reached.
    StoreUnavailable,
    /// Putting the restored files in place failed (Load stage 3).
    SwapFailed,
    /// A failed Load couldn't be undone; the game is blocked.
    RollbackFailed,
    /// The Load was applied but couldn't be recorded.
    NotRecorded,
    /// A deletion couldn't finish.
    DeleteIncomplete,
    /// A target root that held data at Save time is missing.
    RootMissing,
    /// No target matches anything: nothing to save.
    NoGameData,
    /// No usable saved checkpoint to load.
    NoSaves,
    /// The game has no valid save location.
    NoSaveLocation,
    /// Another operation runs for this game.
    Busy,
    /// The game waits on an unresolved interruption.
    Blocked,
    /// The game, checkpoint or operation doesn't exist.
    NotFound,
    /// A label edit for a checkpoint that no longer exists.
    Gone,
    /// The request is malformed or not allowed.
    InvalidRequest,
    /// The configuration value failed validation.
    InvalidConfig,
    /// Client and host protocol versions differ.
    VersionMismatch,
    /// A history page position is stale; read again from the start.
    Reload,
    /// A reply would be too large.
    TooLarge,
    /// The host is still starting.
    Starting,
    /// A checkpoint and the live saves disagree about a path's kind.
    KindConflict,
    /// A checkpoint folder can't be read right now (unreadable, not changed).
    CheckpointUnreadable,
    /// Anything else the file system reported.
    Io,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        // serde's names, without going through JSON.
        match self {
            ErrorKind::InUse => "in_use",
            ErrorKind::ReadFailed => "read_failed",
            ErrorKind::DiskFull => "disk_full",
            ErrorKind::AccessDenied => "access_denied",
            ErrorKind::TargetUnavailable => "target_unavailable",
            ErrorKind::LinkInTarget => "link_in_target",
            ErrorKind::CheckpointChanged => "checkpoint_changed",
            ErrorKind::DifferentSaveSet => "different_save_set",
            ErrorKind::InvalidTarget => "invalid_target",
            ErrorKind::ShuttingDown => "shutting_down",
            ErrorKind::DeleteMismatch => "delete_mismatch",
            ErrorKind::StoreUnavailable => "store_unavailable",
            ErrorKind::SwapFailed => "swap_failed",
            ErrorKind::RollbackFailed => "rollback_failed",
            ErrorKind::NotRecorded => "not_recorded",
            ErrorKind::DeleteIncomplete => "delete_incomplete",
            ErrorKind::RootMissing => "root_missing",
            ErrorKind::NoGameData => "no_game_data",
            ErrorKind::NoSaves => "no_saves",
            ErrorKind::NoSaveLocation => "no_save_location",
            ErrorKind::Busy => "busy",
            ErrorKind::Blocked => "blocked",
            ErrorKind::NotFound => "not_found",
            ErrorKind::Gone => "gone",
            ErrorKind::InvalidRequest => "invalid_request",
            ErrorKind::InvalidConfig => "invalid_config",
            ErrorKind::VersionMismatch => "version_mismatch",
            ErrorKind::Reload => "reload",
            ErrorKind::TooLarge => "too_large",
            ErrorKind::Starting => "starting",
            ErrorKind::KindConflict => "kind_conflict",
            ErrorKind::CheckpointUnreadable => "checkpoint_unreadable",
            ErrorKind::Io => "io",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub kind: ErrorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The raw technical detail, for a Details section and logs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

impl Failure {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Failure {
        Failure { kind, game: None, paths: Vec::new(), detail: detail.into() }
    }

    pub fn path(mut self, path: impl AsRef<std::path::Path>) -> Failure {
        self.paths.push(path.as_ref().to_string_lossy().into_owned());
        self
    }

    pub fn paths<P: AsRef<std::path::Path>>(mut self, paths: impl IntoIterator<Item = P>) -> Failure {
        self.paths.extend(paths.into_iter().map(|p| p.as_ref().to_string_lossy().into_owned()));
        self
    }

    pub fn game(mut self, game: impl Into<String>) -> Failure {
        self.game = Some(game.into());
        self
    }

    pub fn with_game_if_missing(mut self, game: &str) -> Failure {
        if self.game.is_none() {
            self.game = Some(game.to_string());
        }
        self
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind.as_str())?;
        if let Some(game) = &self.game {
            write!(f, " [{game}]")?;
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        for path in &self.paths {
            write!(f, "\n  {path}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Failure {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_names_match_serde() {
        for kind in [ErrorKind::InUse, ErrorKind::DifferentSaveSet, ErrorKind::Io, ErrorKind::NoSaveLocation] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
    }
}
