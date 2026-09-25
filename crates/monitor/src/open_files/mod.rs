//! Which of some files a game's processes hold open (PLAN-HOST, LOAD,
//! "Interference from the running game"). Best-effort: a process may refuse
//! to be inspected, and a file opened after the check isn't seen.

/// A file by identity: device and inode, taken just before the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

impl FileId {
    /// The identity of the file at `path` now, following no link.
    #[cfg(unix)]
    pub fn of(path: &std::path::Path) -> Option<FileId> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(FileId { dev: meta.dev(), ino: meta.ino() })
    }

    #[cfg(not(unix))]
    pub fn of(_path: &std::path::Path) -> Option<FileId> {
        None
    }
}

/// What an inspection found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inspection {
    /// (pid, index into the files asked about) for every file found open.
    pub open: Vec<(u32, usize)>,
    /// Processes that couldn't be inspected, with why. Nothing found open
    /// in them is not the same as nothing open.
    pub incomplete: Vec<(u32, String)>,
}

/// Whether this OS can inspect other processes' open files at all. Where it
/// can't (Windows, for now), the check is skipped.
pub const SUPPORTED: bool = imp::SUPPORTED;

/// Looks through every open file of `pids` for any of `files`. A process
/// that exited meanwhile holds nothing.
pub fn inspect(pids: &[u32], files: &[FileId]) -> Inspection {
    let mut found = Inspection::default();
    for &pid in pids {
        match imp::open_files(pid) {
            Ok(open) => {
                for (index, file) in files.iter().enumerate() {
                    if open.iter().any(|o| imp::same(o, file)) {
                        found.open.push((pid, index));
                    }
                }
            }
            Err(Unreadable::Exited) => {}
            Err(Unreadable::Refused(why)) => found.incomplete.push((pid, why)),
        }
    }
    found
}

/// Why a process's open files couldn't be listed.
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
enum Unreadable {
    Exited,
    Refused(String),
}

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), path = "unsupported.rs")]
mod imp;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_a_file_this_process_holds_and_forgets_it_once_closed() {
        let dir = std::env::temp_dir().join(format!("ss-open-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (held, other) = (dir.join("held.sav"), dir.join("other.sav"));
        fs::write(&held, b"h").unwrap();
        fs::write(&other, b"o").unwrap();
        let ids = [FileId::of(&other).unwrap(), FileId::of(&held).unwrap()];
        let me = std::process::id();

        let file = fs::File::open(&held).unwrap();
        let found = inspect(&[me], &ids);
        assert_eq!(found.open, vec![(me, 1)]);
        assert!(found.incomplete.is_empty(), "{:?}", found.incomplete);

        drop(file);
        assert_eq!(inspect(&[me], &ids), Inspection::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_process_that_is_gone_holds_nothing() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(inspect(&[pid], &[FileId { dev: 1, ino: 1 }]), Inspection::default());
    }

    #[test]
    fn a_process_of_another_user_is_incomplete_not_empty() {
        // launchd / init belongs to root; an ordinary user can't read it.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let found = inspect(&[1], &[FileId { dev: 1, ino: 1 }]);
        assert!(found.open.is_empty());
        assert_eq!(found.incomplete.len(), 1, "{found:?}");
    }
}
