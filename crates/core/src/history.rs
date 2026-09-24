//! Which history rows are visible. The host computes this once, the same for
//! every client; storage only keeps the result as an index.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Saved,
    Loaded,
    Reverted,
    GameStarted,
    GameClosed,
}

impl RowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RowKind::Saved => "saved",
            RowKind::Loaded => "loaded",
            RowKind::Reverted => "reverted",
            RowKind::GameStarted => "game_started",
            RowKind::GameClosed => "game_closed",
        }
    }

    pub fn parse(text: &str) -> Option<RowKind> {
        Some(match text {
            "saved" => RowKind::Saved,
            "loaded" => RowKind::Loaded,
            "reverted" => RowKind::Reverted,
            "game_started" => RowKind::GameStarted,
            "game_closed" => RowKind::GameClosed,
            _ => return None,
        })
    }

    pub fn is_marker(self) -> bool {
        matches!(self, RowKind::GameStarted | RowKind::GameClosed)
    }
}

/// What the visibility rule needs to know about one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFacts {
    pub kind: RowKind,
    pub session: Option<String>,
    /// For Saved: its saved checkpoint exists. For Loaded and Reverted: its
    /// recovery checkpoint exists. Unused for markers. A temporarily
    /// unavailable checkpoint still exists.
    pub checkpoint_exists: bool,
}

/// Visibility for every row of one game:
///
/// - Saved, Loaded and Reverted rows are visible while the checkpoint they
///   own exists.
/// - Game started and Game closed markers are visible while their session
///   contains a visible Saved, Loaded or Reverted row. Sessions with nothing
///   left in them disappear.
pub fn visibility(rows: &[RowFacts]) -> Vec<bool> {
    use std::collections::HashSet;
    let live_sessions: HashSet<&str> = rows
        .iter()
        .filter(|r| !r.kind.is_marker() && r.checkpoint_exists)
        .filter_map(|r| r.session.as_deref())
        .collect();
    rows.iter()
        .map(|r| {
            if r.kind.is_marker() {
                r.session.as_deref().is_some_and(|s| live_sessions.contains(s))
            } else {
                r.checkpoint_exists
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: RowKind, session: Option<&str>, exists: bool) -> RowFacts {
        RowFacts { kind, session: session.map(str::to_string), checkpoint_exists: exists }
    }

    #[test]
    fn rows_follow_their_checkpoints() {
        let rows = [
            row(RowKind::Saved, None, true),
            row(RowKind::Saved, None, false),
            row(RowKind::Loaded, None, true),
            row(RowKind::Reverted, None, false),
        ];
        assert_eq!(visibility(&rows), vec![true, false, true, false]);
    }

    #[test]
    fn empty_sessions_disappear() {
        let rows = [
            row(RowKind::GameStarted, Some("s1"), false),
            row(RowKind::Saved, Some("s1"), false),
            row(RowKind::GameClosed, Some("s1"), false),
            row(RowKind::GameStarted, Some("s2"), false),
            row(RowKind::GameClosed, Some("s2"), false),
            row(RowKind::GameStarted, Some("s3"), false),
            row(RowKind::Loaded, Some("s3"), true),
            row(RowKind::GameClosed, Some("s3"), false),
        ];
        assert_eq!(visibility(&rows), vec![false, false, false, false, false, true, true, true]);
    }

    #[test]
    fn no_checkpoints_means_empty_history() {
        let rows = [row(RowKind::GameStarted, Some("s"), false), row(RowKind::Saved, Some("s"), false)];
        assert!(visibility(&rows).iter().all(|v| !v));
    }
}
