use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Device numbers change when a drive is attached again (and FAT/exFAT
/// inode numbers are made up per mount).
pub const IDENTITY_SURVIVES_REMOUNT: bool = false;

/// "Permission denied" always means permissions here: an open file never
/// blocks a rename or delete.
pub const DENIED_MAY_MEAN_IN_USE: bool = false;

/// Errors that mean the medium isn't there: a failing disk, a gone device, a
/// dead network share.
pub fn is_unavailable(e: &io::Error) -> bool {
    let Some(code) = e.raw_os_error() else { return false };
    #[cfg(target_os = "linux")]
    if code == libc::ENOMEDIUM {
        return true;
    }
    [libc::EIO, libc::ENXIO, libc::ENODEV, libc::ENOTCONN, libc::EHOSTDOWN, libc::ESTALE].contains(&code)
}

/// Nothing holds files exclusively here.
pub fn is_in_use(_e: &io::Error) -> bool {
    false
}

/// An open file blocks neither rename nor delete here, so nothing is worth
/// retrying (open saves are found by the open-file check instead).
pub fn is_transient(_e: &io::Error) -> bool {
    false
}

/// `ErrorKind::StorageFull` covers it.
pub fn is_disk_full(_e: &io::Error) -> bool {
    false
}

/// The mount point of the drive `path` is on: the highest folder above it on
/// the same device. None for the system drive's root and for relative paths.
pub fn mount_point(path: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() {
        return None;
    }
    let device = fs::metadata(path).ok()?.dev();
    let mut mount = path.to_path_buf();
    for ancestor in path.ancestors().skip(1) {
        if fs::metadata(ancestor).ok()?.dev() != device {
            break;
        }
        mount = ancestor.to_path_buf();
    }
    mount.parent().is_some().then_some(mount)
}

/// Whether a drive is mounted at `mount`: it's there, on another device than
/// the folder holding it. An empty folder left behind is not.
pub fn is_mounted(mount: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(here), Some(Ok(parent))) = (fs::metadata(mount), mount.parent().map(fs::metadata)) else {
        return false;
    };
    here.dev() != parent.dev()
}

pub fn identity(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path).ok()?;
    Some(format!("{:x}-{:x}", meta.dev(), meta.ino()))
}

pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path =
        |p: &Path| CString::new(p.as_os_str().as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e));
    let (from_c, to_c) = (path(from)?, path(to)?);
    // SAFETY: two NUL-terminated paths that outlive the call.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) };
    // SAFETY: as above; AT_FDCWD resolves relative paths like rename(2).
    #[cfg(not(target_os = "macos"))]
    let result = unsafe {
        libc::renameat2(libc::AT_FDCWD, from_c.as_ptr(), libc::AT_FDCWD, to_c.as_ptr(), libc::RENAME_NOREPLACE)
    };
    if result == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        // The file system has no exclusive rename (macOS: ENOTSUP; Linux:
        // EINVAL, or ENOSYS on an old kernel): the caller falls back.
        Some(libc::ENOTSUP | libc::EINVAL | libc::ENOSYS) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{}: this file system can't rename without replacing", to.display()),
        )),
        _ => Err(e),
    }
}
