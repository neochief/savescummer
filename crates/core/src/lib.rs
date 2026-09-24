//! The portable rules: nothing here knows about the OS, SQLite, the protocol
//! or a UI. The host wires real implementations around them.

pub mod common;
pub mod error;
pub mod history;
pub mod labels;
pub mod recovery;
pub mod safety;
pub mod stack;

pub use error::{ErrorKind, Failure};
pub use savescummer_catalog::{Filter, Platform, Presence, Store, Target};

/// Reserved suffixes: an interrupted Load's copies and set-aside files.
/// They are never backed up, restored or matched by any filter.
pub const SUFFIX_NEW: &str = ".ssnew";
pub const SUFFIX_OLD: &str = ".ssold";

pub fn has_reserved_suffix(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(SUFFIX_NEW) || lower.ends_with(SUFFIX_OLD)
}

/// What is never part of a checkpoint, even inside a save folder: Steam's
/// own files, logs and crash dumps, and our reserved suffixes. The list is
/// short on purpose: a name that could plausibly be a save never goes on it.
pub fn builtin_excluded(name: &str, is_dir: bool) -> bool {
    let lower = name.to_lowercase();
    if has_reserved_suffix(&lower) {
        return true;
    }
    if is_dir {
        return lower == "logs" || lower == "crashes";
    }
    matches!(
        lower.as_str(),
        "steam_autocloud.vdf" | "remotecache.vdf" | "log.txt" | "client_log.txt" | "output_log.txt"
    ) || lower.ends_with(".log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_excludes() {
        assert!(builtin_excluded("Player.log", false));
        assert!(builtin_excluded("Logs", true));
        assert!(builtin_excluded("Crashes", true));
        assert!(builtin_excluded("steam_autocloud.vdf", false));
        assert!(builtin_excluded("save1.ssnew", false));
        assert!(builtin_excluded("save1.SSOLD", false));
        assert!(builtin_excluded("output_log.txt", false));
        assert!(!builtin_excluded("save.dat", false));
        assert!(!builtin_excluded("logs.sav", false));
        assert!(!builtin_excluded("logs", false), "a file named logs could be a save");
        assert!(!builtin_excluded("changelog.txt", false));
    }
}
