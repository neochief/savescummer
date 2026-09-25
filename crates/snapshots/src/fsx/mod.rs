//! Small file system helpers with the semantics the rules need: three-way
//! presence, real paths, file identities, renames that never replace, and
//! copies that tell reading from writing failures.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use savescummer_core::{ErrorKind, Failure, Presence};

// File identity, renames and what the OS's error codes mean, per OS.
#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "unix.rs")]
mod imp;

/// Whether a path exists. `Missing` only when the nearest existing parent
/// can be read and the entry isn't in it; an unplugged drive, an unreachable
/// share or an access error is `Unknown`.
pub fn presence(path: &Path) -> Presence {
    match fs::symlink_metadata(path) {
        Ok(_) => Presence::Present,
        Err(e) if is_not_found(&e) => {
            for ancestor in path.ancestors().skip(1) {
                if ancestor.as_os_str().is_empty() {
                    break;
                }
                match fs::metadata(ancestor) {
                    Ok(meta) if meta.is_dir() => {
                        return if fs::read_dir(ancestor).is_ok() { Presence::Missing } else { Presence::Unknown };
                    }
                    // A file where a folder should be: the path can't exist.
                    Ok(_) => return Presence::Missing,
                    Err(e) if is_not_found(&e) => continue,
                    Err(_) => return Presence::Unknown,
                }
            }
            Presence::Unknown
        }
        Err(_) => Presence::Unknown,
    }
}

fn is_not_found(e: &io::Error) -> bool {
    // A drive that isn't ready reports "not found" on some systems. A path
    // under a file ("not a directory") can't exist either.
    matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) && !is_unavailable(e)
}

/// Errors that mean the medium isn't there right now.
pub fn is_unavailable(e: &io::Error) -> bool {
    imp::is_unavailable(e)
}

/// The real path: links, junctions and on-disk case resolved for the part
/// that exists, the rest appended as given. A link that can't be resolved
/// is an error.
pub fn real_path(path: &Path) -> Result<PathBuf, String> {
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    let mut current = path;
    loop {
        match fs::canonicalize(current) {
            Ok(real) => {
                let mut real = dunce::simplified(&real).to_path_buf();
                for name in rest.iter().rev() {
                    real.push(name);
                }
                return Ok(real);
            }
            Err(e) => {
                if fs::symlink_metadata(current).is_ok() && !is_unavailable(&e) {
                    return Err(format!("{}: {e}", current.display()));
                }
                match (current.parent(), current.file_name()) {
                    (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
                        rest.push(name);
                        current = parent;
                    }
                    _ => return Ok(path.to_path_buf()),
                }
            }
        }
    }
}

/// A file's identity, stable across renames on one volume.
pub fn identity(path: &Path) -> Option<String> {
    imp::identity(path)
}

/// Renames without ever replacing an existing entry.
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    imp::rename_noreplace(from, to)
}

/// Which side of a copy failed.
pub enum CopyError {
    Read(io::Error),
    Write(io::Error),
}

/// Copies one file, keeping its modification time. Never replaces an
/// existing destination.
pub fn copy_file(from: &Path, to: &Path) -> Result<u64, CopyError> {
    let mut source = File::open(from).map_err(CopyError::Read)?;
    let meta = source.metadata().map_err(CopyError::Read)?;
    let mut dest = fs::OpenOptions::new().write(true).create_new(true).open(to).map_err(CopyError::Write)?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CopyError::Read(e)),
        };
        dest.write_all(&buffer[..n]).map_err(CopyError::Write)?;
        total += n as u64;
    }
    if let Ok(modified) = meta.modified() {
        dest.set_modified(modified).map_err(CopyError::Write)?;
    }
    dest.sync_all().map_err(CopyError::Write)?;
    Ok(total)
}

/// The error kind for a failure writing somewhere.
pub fn write_kind(e: &io::Error) -> ErrorKind {
    if is_disk_full(e) {
        ErrorKind::DiskFull
    } else if e.kind() == io::ErrorKind::PermissionDenied {
        ErrorKind::AccessDenied
    } else if is_unavailable(e) {
        ErrorKind::TargetUnavailable
    } else if is_in_use(e) {
        ErrorKind::InUse
    } else {
        ErrorKind::Io
    }
}

/// The error kind for a failed rename of a live file.
pub fn rename_kind(e: &io::Error) -> ErrorKind {
    if is_in_use(e) {
        ErrorKind::InUse
    } else if e.kind() == io::ErrorKind::PermissionDenied {
        // Windows reports a file held open by another program this way too.
        if imp::DENIED_MAY_MEAN_IN_USE { ErrorKind::InUse } else { ErrorKind::AccessDenied }
    } else if is_unavailable(e) {
        ErrorKind::TargetUnavailable
    } else {
        ErrorKind::Io
    }
}

/// Another program holds the file (Windows' sharing and lock violations).
pub fn is_in_use(e: &io::Error) -> bool {
    imp::is_in_use(e)
}

pub fn is_disk_full(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::StorageFull || imp::is_disk_full(e)
}

pub fn failure(kind: ErrorKind, e: &io::Error, path: &Path) -> Failure {
    Failure::new(kind, e.to_string()).path(path)
}

/// A reserved sibling name: `name.ssnew` or `name.ssold`.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_is_three_way() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(presence(dir.path()), Presence::Present);
        assert_eq!(presence(&dir.path().join("a/b/c")), Presence::Missing);
        let file = dir.path().join("f");
        fs::write(&file, b"x").unwrap();
        assert_eq!(presence(&file.join("inside")), Presence::Missing);
    }

    #[test]
    fn rename_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        assert!(rename_noreplace(&a, &b).is_err());
        assert_eq!(fs::read(&b).unwrap(), b"b");
        let c = dir.path().join("c");
        rename_noreplace(&a, &c).unwrap();
        assert_eq!(fs::read(&c).unwrap(), b"a");
    }

    #[test]
    fn identity_survives_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        fs::write(&a, b"a").unwrap();
        let before = identity(&a).unwrap();
        let b = dir.path().join("b");
        fs::rename(&a, &b).unwrap();
        assert_eq!(identity(&b).unwrap(), before);
    }

    #[test]
    fn real_path_appends_the_missing_part() {
        let dir = tempfile::tempdir().unwrap();
        let real = real_path(&dir.path().join("x/y")).unwrap();
        assert!(real.ends_with("x/y"));
        assert!(real.is_absolute());
    }
}
