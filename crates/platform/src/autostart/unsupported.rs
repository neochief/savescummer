//! No sign-in entry on this OS yet (PLAN-MACOS.md, LAUNCH AT LOGIN).

use std::path::Path;

pub fn set(on: bool, host_exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    let _ = (host_exe, data_dir);
    if on { Err("launch at sign-in isn't supported on this OS yet".into()) } else { Ok(()) }
}

pub fn is_enabled(host_exe: &Path) -> bool {
    let _ = host_exe;
    false
}
