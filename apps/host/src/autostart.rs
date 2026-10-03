//! Start at login: the user's recorded choice, and applying it.
//!
//! `launch_on_startup` in the settings is the choice: absent when nobody
//! chose, `1` or `0` when the installer (`--autostart`), Settings or
//! first-launch setup did. Whether the OS really starts the host comes
//! from the OS, never from here. An explicit choice is recorded before the
//! OS is changed, so a failed change still keeps first-launch setup from
//! turning startup on later.
//!
//! The flag, Settings and first-launch setup apply choices one at a time:
//! through the running host's [`Host::launch_lock`](crate::host::Host), or,
//! with no host running, while holding the data folder's host lock.

use std::path::Path;

use savescummer_platform::autostart::{self, Status};
use savescummer_storage::{self as db, Connection, Storage};

use crate::model::SETTING_LAUNCH;

/// The recorded choice; `None` when nobody chose yet.
pub fn recorded(conn: &Connection) -> Result<Option<bool>, db::Error> {
    Ok(db::setting(conn, SETTING_LAUNCH)?.map(|value| value == "1"))
}

fn record(storage: &mut Storage, on: bool) -> Result<(), db::Error> {
    storage.write(|c| db::set_setting(c, SETTING_LAUNCH, if on { "1" } else { "0" }))
}

/// An explicit choice (the flag, Settings): recorded, then applied. The
/// record stays when the OS refuses.
pub fn choose(storage: &mut Storage, on: bool, exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    record(storage, on).map_err(|e| format!("can't record the choice: {e}"))?;
    autostart::set(on, exe, data_dir)
}

/// What first-launch setup did about startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Default {
    /// Someone chose already, or the OS has an entry: left as it was.
    Kept,
    /// Nothing there: the entry was written now.
    Registered,
    /// Nothing there, but writing it failed or this copy can't have one.
    Failed,
}

/// First-launch setup: turns startup on when nothing was ever chosen and
/// the OS has no entry by our name. Runs once per profile; a choice or an
/// entry (ours, turned off, or another copy's) is never overwritten.
pub fn apply_default(storage: &mut Storage, exe: &Path, data_dir: Option<&Path>) -> Default {
    // The latest record, read now: a choice made a moment ago wins.
    match recorded(storage.conn()) {
        Ok(None) => {}
        Ok(Some(_)) => return Default::Kept,
        Err(e) => {
            crate::trace(&format!("start at login: can't read the choice: {e}"));
            return Default::Failed;
        }
    }
    match autostart::inspect(exe) {
        Status::Absent => {}
        Status::Unavailable => return Default::Failed,
        status => {
            crate::trace(&format!("start at login: kept as found ({status:?})"));
            return Default::Kept;
        }
    }
    match autostart::set(true, exe, data_dir) {
        Ok(()) => {
            if let Err(e) = record(storage, true) {
                crate::trace(&format!("start at login: on, but can't record it: {e}"));
            }
            crate::trace("start at login: on by default");
            Default::Registered
        }
        Err(e) => {
            crate::trace(&format!("start at login: the default failed: {e}"));
            Default::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("host.db")).unwrap();
        (dir, storage)
    }

    #[test]
    fn a_refused_choice_is_still_recorded() {
        let (_dir, mut storage) = storage();
        assert_eq!(recorded(storage.conn()).unwrap(), None);
        // Development builds refuse `on`; the choice stays.
        let exe = std::env::temp_dir().join("SaveScummerTest-choose").join("SaveScummer");
        assert!(choose(&mut storage, true, &exe, None).is_err());
        assert_eq!(recorded(storage.conn()).unwrap(), Some(true));
        assert_eq!(choose(&mut storage, false, &exe, None), Ok(()));
        assert_eq!(recorded(storage.conn()).unwrap(), Some(false));
    }

    #[test]
    fn the_default_never_overrides_a_choice() {
        let (_dir, mut storage) = storage();
        let exe = std::env::temp_dir().join("SaveScummerTest-default").join("SaveScummer");
        record(&mut storage, false).unwrap();
        assert_eq!(apply_default(&mut storage, &exe, None), Default::Kept);
        assert_eq!(recorded(storage.conn()).unwrap(), Some(false));
    }

    #[test]
    fn a_development_build_gets_no_default() {
        let (_dir, mut storage) = storage();
        let exe = std::env::temp_dir().join("SaveScummerTest-dev").join("SaveScummer");
        assert_eq!(apply_default(&mut storage, &exe, None), Default::Failed);
        assert_eq!(recorded(storage.conn()).unwrap(), None);
    }
}
