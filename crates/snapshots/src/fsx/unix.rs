use std::fs;
use std::io;
use std::path::Path;

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

/// `ErrorKind::StorageFull` covers it.
pub fn is_disk_full(_e: &io::Error) -> bool {
    false
}

pub fn identity(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path).ok()?;
    Some(format!("{:x}-{:x}", meta.dev(), meta.ino()))
}

pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "destination exists"));
    }
    fs::rename(from, to)
}
