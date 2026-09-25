//! Linux: `stat` on each entry of `/proc/<pid>/fd` gives the device and
//! inode of the file behind it. Needs the same rights as reading the
//! process's descriptors: the user's own processes, and not with `/proc`
//! mounted `hidepid` or the process in another pid namespace.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;

use super::{FileId, Unreadable};

pub const SUPPORTED: bool = true;

pub type Open = FileId;

pub fn open_files(pid: u32) -> Result<Vec<Open>, Unreadable> {
    let dir = format!("/proc/{pid}/fd");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Unreadable::Exited),
        Err(e) => return Err(Unreadable::Refused(format!("{dir}: {e}"))),
    };
    let mut open = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Unreadable::Refused(format!("{dir}: {e}")))?;
        match fs::metadata(entry.path()) {
            Ok(meta) => open.push(FileId { dev: meta.dev(), ino: meta.ino() }),
            // Closed between listing and looking.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(Unreadable::Refused(format!("{}: {e}", entry.path().display()))),
        }
    }
    Ok(open)
}

pub fn same(open: &Open, file: &FileId) -> bool {
    open == file
}
