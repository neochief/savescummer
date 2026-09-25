//! Launch on startup: the OS sign-in entry that starts the host minimized.
//!
//! The host is the only writer of this entry (the checkbox and
//! `--autostart on|off` share this code). `off` only removes an entry that
//! points at this host, and development builds never create one: signing in
//! must never start a debug host, and a dev host must not touch the installed
//! app's entry.

use std::path::Path;

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(not(windows), path = "unsupported.rs")]
mod imp;

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

/// Whether a sign-in entry pointing at `host_exe` exists.
pub fn is_enabled(host_exe: &Path) -> bool {
    imp::is_enabled(host_exe)
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
    fn off_without_our_entry_is_ok_and_changes_nothing() {
        // The random exe path can never match a real entry, so `off` must
        // leave everything as it was.
        let exe = unique_exe();
        assert!(!is_enabled(&exe));
        assert_eq!(set(false, &exe, None), Ok(()));
        assert!(!is_enabled(&exe));
    }
}
