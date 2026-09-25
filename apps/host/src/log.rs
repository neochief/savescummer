//! The host's log: `host.log` in the data folder, where all of the host's
//! own output goes (it is a GUI program with no console). It records
//! startup and shutdown steps and what the host noticed on its own, such as
//! games installed and uninstalled, so there is a record even when nothing
//! was attached to the host's output (started at sign-in, or by the UI).
//!
//! The file is capped: past [`MAX_BYTES`] it becomes `host.log.1` (replacing
//! the previous one) and a new file starts.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 1024 * 1024;

static FILE: OnceLock<Mutex<PathBuf>> = OnceLock::new();

/// Starts writing to `host.log` in the data folder.
pub fn init(data_dir: &std::path::Path) {
    let _ = FILE.set(Mutex::new(data_dir.join("host.log")));
}

/// One timestamped line in the log file.
pub fn line(text: &str) {
    let line = format!("[{}] {text}\n", crate::host::now());
    let Some(path) = FILE.get() else { return };
    let path = path.lock().unwrap_or_else(|e| e.into_inner());
    if fs::metadata(&*path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(&*path, path.with_extension("log.1"));
    }
    // A log that can't be written never stops the host.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&*path) {
        let _ = file.write_all(line.as_bytes());
    }
}
