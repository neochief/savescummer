//! Start at login: the OS sign-in entry that starts the host minimized.
//!
//! The host is the only writer of this entry (the checkbox and
//! `--autostart on|off` share this code). `off` only removes an entry that
//! points at this host, and development builds never create one: signing in
//! must never start a debug host, and a dev host must not touch the installed
//! app's entry.

use std::path::Path;

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), path = "unsupported.rs")]
pub(crate) mod imp;

const DEV_BUILD_REFUSAL: &str = "development builds never create a sign-in entry";

/// Whether this build may create a sign-in entry. Only release builds (built
/// with `SAVESCUMMER_RELEASE_BUILD=1`) may.
pub fn available() -> bool {
    option_env!("SAVESCUMMER_RELEASE_BUILD") == Some("1")
}

/// Writes (`on`) or removes (`!on`) the OS sign-in entry for `host_exe`,
/// which starts it with `--minimized` (and `--data-dir` when given).
pub fn set(on: bool, host_exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    if on && !available() {
        return Err(DEV_BUILD_REFUSAL.into());
    }
    imp::set(on, host_exe, data_dir)
}

/// Whether a sign-in entry pointing at `host_exe` exists (and, on macOS, is
/// allowed to run).
pub fn is_enabled(host_exe: &Path) -> bool {
    imp::is_enabled(host_exe)
}

/// What the OS has for this app's sign-in entry, before anything changes it.
/// First-launch setup only writes the default where it finds [`Status::Absent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// No entry of ours or anyone else's.
    Absent,
    /// Ours, pointing at this host, and allowed to run.
    Enabled,
    /// Ours, turned off in the OS's settings (Task Manager, the desktop's
    /// startup apps). Only the user turns it on again.
    Disabled,
    /// macOS: registered, waiting for the user's approval in Login Items.
    NeedsApproval,
    /// An entry by our name for another copy, or not ours at all.
    Foreign,
    /// This build or location can't have a working entry (a development
    /// build, a copy on a disk image, a missing bundle).
    Unavailable,
}

/// Reads the entry's state without changing anything.
pub fn inspect(host_exe: &Path) -> Status {
    if !available() {
        return Status::Unavailable;
    }
    imp::inspect(host_exe)
}

/// macOS: opens Login Items in System Settings, where the user approves
/// the agent. Elsewhere there is nothing to open.
pub fn open_approval_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return imp::open_approval_settings();
    #[cfg(not(target_os = "macos"))]
    Err("there is no approval to give on this OS".into())
}

/// Linux AppImage: re-points an enabled entry at this AppImage when another
/// version wrote it (each version is a new file). Elsewhere the program's
/// path is fixed, and this never changes anything.
pub fn keep_current(host_exe: &Path) {
    #[cfg(target_os = "linux")]
    imp::keep_current(host_exe);
    #[cfg(not(target_os = "linux"))]
    let _ = host_exe;
}

/// macOS: the entry exists but the user turned it off in System Settings,
/// where only they can turn it on again (Login Items).
pub fn needs_approval(host_exe: &Path) -> bool {
    #[cfg(target_os = "macos")]
    return imp::needs_approval(host_exe);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = host_exe;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A host path that no real entry can point at.
    fn unique_exe() -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("SaveScummerTest-{}-{nanos}", std::process::id())).join("SaveScummer")
    }

    #[test]
    fn tests_are_dev_builds() {
        assert!(!available());
    }

    #[test]
    fn on_is_refused_in_dev_builds() {
        let err = set(true, &unique_exe(), None).unwrap_err();
        assert_eq!(err, DEV_BUILD_REFUSAL);
    }

    #[test]
    fn dev_builds_have_no_usable_entry() {
        assert_eq!(inspect(&unique_exe()), Status::Unavailable);
    }

    #[test]
    fn off_without_our_entry_is_ok_and_changes_nothing() {
        // The random exe path can never match a real entry, so `off` must
        // leave everything as it was.
        let exe = unique_exe();
        assert!(!is_enabled(&exe));
        assert_eq!(set(false, &exe, None), Ok(()));
        assert!(!is_enabled(&exe));
    }
}
