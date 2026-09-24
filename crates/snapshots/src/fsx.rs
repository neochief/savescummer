//! Small file system helpers with the semantics the rules need: three-way
//! presence, real paths, file identities, renames that never replace, and
//! copies that tell reading from writing failures.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use savescummer_core::{ErrorKind, Failure, Presence};

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
    // A drive that isn't ready reports "not found" on some systems.
    e.kind() == io::ErrorKind::NotFound && !is_unavailable(e)
}

/// Errors that mean the medium isn't there right now.
pub fn is_unavailable(e: &io::Error) -> bool {
    if cfg!(windows) {
        // NOT_READY, BAD_NETPATH, NETNAME_DELETED, BAD_NET_NAME, UNEXP_NET_ERR,
        // DEVICE_NOT_CONNECTED, NO_MEDIA_IN_DRIVE
        matches!(e.raw_os_error(), Some(21 | 53 | 64 | 67 | 59 | 1167 | 1112))
    } else {
        matches!(e.raw_os_error(), Some(5 | 6 | 19 | 107 | 112 | 116 | 123))
    }
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
#[cfg(windows)]
pub fn identity(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        let ok = GetFileInformationByHandle(handle, &mut info);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Some(format!("{:x}-{:x}", info.dwVolumeSerialNumber, index))
    }
}

#[cfg(unix)]
pub fn identity(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path).ok()?;
    Some(format!("{:x}-{:x}", meta.dev(), meta.ino()))
}

/// Renames without ever replacing an existing entry.
#[cfg(windows)]
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let a: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // No MOVEFILE_REPLACE_EXISTING: an existing destination fails the move.
    if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "destination exists"));
    }
    fs::rename(from, to)
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
        if cfg!(windows) { ErrorKind::InUse } else { ErrorKind::AccessDenied }
    } else if is_unavailable(e) {
        ErrorKind::TargetUnavailable
    } else {
        ErrorKind::Io
    }
}

pub fn is_in_use(e: &io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33))
}

pub fn is_disk_full(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::StorageFull || (cfg!(windows) && matches!(e.raw_os_error(), Some(39 | 112)))
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

    #[cfg(windows)]
    #[test]
    fn an_absent_drive_is_unknown() {
        // Find a drive letter that doesn't exist on this machine.
        let free = ('D'..='Z').rev().find(|l| fs::metadata(format!("{l}:\\")).is_err()).unwrap();
        assert_eq!(presence(Path::new(&format!("{free}:\\Games\\save"))), Presence::Unknown);
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
