//! Small file system helpers with the semantics the rules need: three-way
//! presence, real paths, file identities, renames that never replace, and
//! copies that tell reading from writing failures.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use savescummer_core::{ErrorKind, Failure, Presence};

// File identity, renames and what the OS's error codes mean, per OS.
#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(unix, path = "unix.rs")]
mod imp;

/// Whether a path exists. `Missing` only when the nearest existing parent
/// can be read and the entry isn't in it; an unplugged drive, an unreachable
/// share or an access error is `Unknown`. So is anything under a remembered
/// drive that isn't mounted now (see [`remember_drives`]), and anything
/// [guarded](set_guard).
pub fn presence(path: &Path) -> Presence {
    match on_disk(path) {
        Presence::Unknown => Presence::Unknown,
        _ if under_a_disconnected_drive(path) => Presence::Unknown,
        seen => seen,
    }
}

fn on_disk(path: &Path) -> Presence {
    if is_guarded(path) {
        return Presence::Unknown;
    }
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

// ---- Progress, so a watchdog can tell file work that's stuck (a read
// waiting on a permission prompt) from work that's slow.

static PROGRESS: AtomicU64 = AtomicU64::new(0);

/// A counter that moves whenever file work gets anywhere.
pub fn progress() -> u64 {
    PROGRESS.load(Ordering::Relaxed)
}

pub(crate) fn advance() {
    PROGRESS.fetch_add(1, Ordering::Relaxed);
}

// ---- Locations the OS guards (macOS privacy, PLAN-MACOS.md PRIVACY
// PERMISSIONS). Touching one before the user allowed it makes macOS ask, and
// the read waits until someone answers: the host says which paths to leave
// alone for now.

type Guard = Box<dyn Fn(&Path) -> bool + Send + Sync>;

static GUARD: RwLock<Option<Guard>> = RwLock::new(None);

/// Sets which paths must not be touched yet: their presence is unknown,
/// their real path an error, and walking them fails.
pub fn set_guard(blocked: impl Fn(&Path) -> bool + Send + Sync + 'static) {
    *GUARD.write().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(blocked));
}

/// Whether `path` must not be touched yet.
pub fn is_guarded(path: &Path) -> bool {
    GUARD.read().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|blocked| blocked(path))
}

/// The failure for a guarded path.
pub fn guarded_failure(path: &Path) -> Failure {
    Failure::new(ErrorKind::AccessNeeded, "macOS hasn't allowed access to this location yet").path(path)
}

// ---- Drives the host has seen (PLAN-HOST, "A drive the host has seen stays
// expected"). On macOS and Linux an unplugged drive's mount point vanishes or
// turns into an empty folder, which would otherwise read as "missing".

/// The mount points of drives the host relies on.
static DRIVES: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn drives() -> std::sync::MutexGuard<'static, BTreeSet<PathBuf>> {
    DRIVES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts from the drives remembered earlier (the host keeps them in its
/// database).
pub fn load_drives(mount_points: impl IntoIterator<Item = PathBuf>) {
    *drives() = mount_points.into_iter().collect();
}

/// The drives remembered now, to keep.
pub fn remembered_drives() -> Vec<PathBuf> {
    drives().iter().cloned().collect()
}

/// Remembers the drive of every path in `used` that is there now, and forgets
/// drives nothing in `used` lives on any more (a library removed, a location
/// reconfigured). `used` is everything the host relies on: the checkpoint
/// store, save locations, Steam libraries, install folders. Returns whether
/// the remembered set changed.
pub fn remember_drives(used: &[PathBuf]) -> bool {
    let mut seen: BTreeSet<PathBuf> =
        drives().iter().filter(|m| used.iter().any(|p| p.starts_with(m))).cloned().collect();
    for path in used {
        if on_disk(path) == Presence::Present
            && !under_a_disconnected_drive(path)
            && let Some(mount) = imp::mount_point(path)
        {
            seen.insert(mount);
        }
    }
    let mut current = drives();
    let changed = *current != seen;
    *current = seen;
    changed
}

fn under_a_disconnected_drive(path: &Path) -> bool {
    drives().iter().any(|mount| path.starts_with(mount) && !imp::is_mounted(mount))
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
    if is_guarded(path) {
        return Err(format!("{}: macOS hasn't allowed access to this location yet", path.display()));
    }
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
    if is_guarded(path) {
        return None;
    }
    imp::identity(path)
}

/// A checkpoint folder's identity, where the OS keeps it stable when a drive
/// is detached and attached again (Windows). macOS and Linux renumber drives
/// on every attach, so there a checkpoint is judged by its signature alone.
pub fn folder_identity(path: &Path) -> Option<String> {
    if imp::IDENTITY_SURVIVES_REMOUNT { imp::identity(path) } else { None }
}

/// Renames without replacing an existing entry. Where the file system can do
/// that in one operation (APFS, ext4, NTFS…), a file created at the same
/// moment makes the rename fail instead of being overwritten. Where it can't
/// (exFAT on macOS, some network drives), it checks, then renames: the tiny
/// gap in between is accepted rather than refusing to work on such drives.
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    advance();
    match imp::rename_noreplace(from, to) {
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            if fs::symlink_metadata(to).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "destination exists"));
            }
            fs::rename(from, to)
        }
        result => result,
    }
}

/// Which side of a copy failed.
pub enum CopyError {
    Read(io::Error),
    Write(io::Error),
}

/// Copies one file, keeping its modification time. Never replaces an
/// existing destination; a copy that fails partway is removed, so it can be
/// tried again.
pub fn copy_file(from: &Path, to: &Path) -> Result<u64, CopyError> {
    let mut source = File::open(from).map_err(CopyError::Read)?;
    let meta = source.metadata().map_err(CopyError::Read)?;
    let mut dest = fs::OpenOptions::new().write(true).create_new(true).open(to).map_err(CopyError::Write)?;
    let copied = copy_into(&mut source, &meta, &mut dest);
    if copied.is_err() {
        drop(dest);
        let _ = fs::remove_file(to);
    }
    copied
}

/// [`copy_file`] within a retry budget: a transient failure on either side
/// is tried again.
pub fn copy_file_retrying(from: &Path, to: &Path, budget: &crate::retry::Budget) -> Result<u64, CopyError> {
    let mut last = None;
    let result = budget.run(|| match copy_file(from, to) {
        Ok(n) => Ok(n),
        Err(CopyError::Read(e)) => {
            last = Some(true);
            Err(e)
        }
        Err(CopyError::Write(e)) => {
            last = Some(false);
            Err(e)
        }
    });
    result.map_err(|e| if last == Some(true) { CopyError::Read(e) } else { CopyError::Write(e) })
}

fn copy_into(source: &mut File, meta: &fs::Metadata, dest: &mut File) -> Result<u64, CopyError> {
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
        advance();
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

/// An error that may clear by itself within moments, worth a retry: on
/// Windows sharing and lock violations and "access denied" (a pending
/// delete or a handle that forbids sharing reports it too). Not proof that
/// the game holds the file.
pub fn is_transient(e: &io::Error) -> bool {
    imp::is_transient(e)
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
