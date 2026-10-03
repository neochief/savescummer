//! No sign-in entry on this OS yet.

use std::path::Path;

pub fn set(on: bool, host_exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    let _ = (host_exe, data_dir);
    if on { Err("start at login isn't supported on this OS yet".into()) } else { Ok(()) }
}

pub fn is_enabled(host_exe: &Path) -> bool {
    let _ = host_exe;
    false
}

pub fn inspect(host_exe: &Path) -> super::Status {
    let _ = host_exe;
    super::Status::Unavailable
}
