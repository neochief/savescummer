use savescummer_core::{Error, ErrorCode, Result, SnapshotIo};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn io_error(path: &Path, error: std::io::Error) -> Error {
    Error::new(ErrorCode::Io, format!("{}: {error}", path.display()))
}
fn regular(metadata: &fs::Metadata, path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::new(
                ErrorCode::InvalidPath,
                format!("reparse points are unsupported: {}", path.display()),
            ));
        }
    }
    if metadata.file_type().is_symlink() || !(metadata.is_dir() || metadata.is_file()) {
        return Err(Error::new(
            ErrorCode::InvalidPath,
            format!(
                "links and special files are unsupported: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Windows Explorer naming is explicit instead of inferred from the host locale.
/// Additional verified copy labels can be supplied without broad prefix matching.
pub struct FileSnapshots {
    pub copy_label: String,
    pub recognized_labels: Vec<String>,
}
impl Default for FileSnapshots {
    fn default() -> Self {
        Self {
            copy_label: "Copy".into(),
            recognized_labels: vec!["Copy".into(), "Copie".into()],
        }
    }
}
impl FileSnapshots {
    pub fn recognizes(&self, live_name: &str, candidate: &str) -> bool {
        self.recognized_labels.iter().any(|label| {
            let base = format!("{live_name} - {label}");
            if candidate == base {
                return true;
            }
            candidate
                .strip_prefix(&format!("{base} ("))
                .and_then(|s| s.strip_suffix(')'))
                .is_some_and(|s| {
                    !s.starts_with('0')
                        && s.chars().all(|c| c.is_ascii_digit())
                        && s.parse::<u64>().is_ok_and(|n| n >= 2)
                })
        })
    }
    fn copy_directory(
        source: &Path,
        destination: &Path,
        copied: &mut u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<()> {
        let metadata = fs::symlink_metadata(source).map_err(|e| io_error(source, e))?;
        regular(&metadata, source)?;
        if !metadata.is_dir() {
            return Err(Error::new(
                ErrorCode::InvalidPath,
                "snapshot source must be a directory",
            ));
        }
        fs::create_dir(destination).map_err(|e| io_error(destination, e))?;
        for entry in fs::read_dir(source).map_err(|e| io_error(source, e))? {
            let entry = entry.map_err(|e| io_error(source, e))?;
            let from = entry.path();
            let to = destination.join(entry.file_name());
            let metadata = fs::symlink_metadata(&from).map_err(|e| io_error(&from, e))?;
            regular(&metadata, &from)?;
            if metadata.is_dir() {
                Self::copy_directory(&from, &to, copied, progress)?;
            } else {
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.custom_flags(0x00200000);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW);
                }
                let mut input = options.open(&from).map_err(|e| io_error(&from, e))?;
                regular(&input.metadata().map_err(|e| io_error(&from, e))?, &from)?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&to)
                    .map_err(|e| io_error(&to, e))?;
                let mut buffer = vec![0; 1024 * 1024];
                loop {
                    let n = input.read(&mut buffer).map_err(|e| io_error(&from, e))?;
                    if n == 0 {
                        break;
                    }
                    output
                        .write_all(&buffer[..n])
                        .map_err(|e| io_error(&to, e))?;
                    *copied += n as u64;
                    progress(*copied);
                }
                output.sync_all().map_err(|e| io_error(&to, e))?;
            }
        }
        #[cfg(unix)]
        fs::File::open(destination)
            .and_then(|f| f.sync_all())
            .map_err(|e| io_error(destination, e))?;
        Ok(())
    }
}
impl SnapshotIo for FileSnapshots {
    fn accessible_dir(&self, path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                regular(&metadata, path)?;
                if !metadata.is_dir() {
                    return Err(Error::new(
                        ErrorCode::InvalidPath,
                        format!("not a directory: {}", path.display()),
                    ));
                }
                fs::read_dir(path).map_err(|e| io_error(path, e))?;
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error(path, e)),
        }
    }
    fn exists(&self, path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error(path, e)),
        }
    }
    fn identity(&self, path: &Path) -> Result<String> {
        identity(path).map_err(|e| io_error(path, e))
    }
    fn fingerprint(&self, path: &Path) -> Result<String> {
        fn inspect(path: &Path, hash: &mut blake3::Hasher) -> Result<()> {
            let metadata = fs::symlink_metadata(path).map_err(|e| io_error(path, e))?;
            regular(&metadata, path)?;
            hash.update(if metadata.is_dir() { b"d" } else { b"f" });
            hash.update(&metadata.len().to_le_bytes());
            let modified = metadata
                .modified()
                .map_err(|e| io_error(path, e))?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            hash.update(&modified.to_le_bytes());
            let id = identity(path).map_err(|e| io_error(path, e))?;
            hash.update(&(id.len() as u64).to_le_bytes());
            hash.update(id.as_bytes());
            if metadata.is_dir() {
                let mut children = fs::read_dir(path)
                    .map_err(|e| io_error(path, e))?
                    .collect::<std::io::Result<Vec<_>>>()
                    .map_err(|e| io_error(path, e))?;
                children.sort_by_key(|entry| entry.file_name());
                for entry in children {
                    let name = entry.file_name();
                    let bytes = name.as_encoded_bytes();
                    hash.update(&(bytes.len() as u64).to_le_bytes());
                    hash.update(bytes);
                    inspect(&entry.path(), hash)?;
                }
            }
            // A concurrent edit invalidates this observation; don't import a
            // partially inspected tree as a completed new generation.
            let after = fs::symlink_metadata(path).map_err(|e| io_error(path, e))?;
            if metadata.modified().ok() != after.modified().ok()
                || metadata.len() != after.len()
                || id != identity(path).map_err(|e| io_error(path, e))?
            {
                return Err(Error::new(
                    ErrorCode::Unavailable,
                    "backup changed during inspection",
                ));
            }
            Ok(())
        }
        let mut hash = blake3::Hasher::new();
        inspect(path, &mut hash)?;
        Ok(hash.finalize().to_hex().to_string())
    }
    fn modified_ms(&self, path: &Path) -> Result<u64> {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64
            })
            .map_err(|e| io_error(path, e))
    }
    fn copy(&self, source: &Path, destination: &Path, progress: &mut dyn FnMut(u64)) -> Result<()> {
        Self::copy_directory(source, destination, &mut 0, progress)
    }
    fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
        rename_exclusive(source, destination).map_err(|e| io_error(destination, e))
    }
    fn saved_candidates(&self, live: &Path) -> Result<Vec<PathBuf>> {
        let parent = live
            .parent()
            .ok_or_else(|| Error::new(ErrorCode::InvalidPath, "missing parent"))?;
        let name = live
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::new(ErrorCode::InvalidPath, "directory name is not Unicode"))?;
        let entries = match fs::read_dir(parent) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::new(
                    ErrorCode::Unavailable,
                    "backup parent is unavailable",
                ));
            }
            Err(e) => return Err(io_error(parent, e)),
        };
        let mut paths = vec![];
        for entry in entries {
            let entry = entry.map_err(|e| io_error(parent, e))?;
            if self.recognizes(name, &entry.file_name().to_string_lossy())
                && self.accessible_dir(&entry.path()).unwrap_or(false)
            {
                paths.push(entry.path());
            }
        }
        Ok(paths)
    }
    fn next_saved_path(&self, live: &Path, reserved: &[PathBuf]) -> Result<PathBuf> {
        let name = live
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::new(ErrorCode::InvalidPath, "directory name is not Unicode"))?;
        for n in 1..1_000_000 {
            let suffix = if n == 1 {
                String::new()
            } else {
                format!(" ({n})")
            };
            let candidate = live.with_file_name(format!("{name} - {}{suffix}", self.copy_label));
            if !reserved.contains(&candidate) && !self.exists(&candidate)? {
                return Ok(candidate);
            }
        }
        Err(Error::new(ErrorCode::Io, "no free backup name"))
    }
    fn remove(&self, path: &Path) -> Result<()> {
        if !self.exists(path)? {
            return Ok(());
        }
        regular(
            &fs::symlink_metadata(path).map_err(|e| io_error(path, e))?,
            path,
        )?;
        fs::remove_dir_all(path).map_err(|e| io_error(path, e))
    }
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
#[cfg(windows)]
fn rename_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    // No REPLACE_EXISTING or COPY_ALLOWED: same-volume atomic rename only.
    if unsafe { MoveFileExW(wide(from).as_ptr(), wide(to).as_ptr(), 0x8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn rename_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    if unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(target_os = "macos")]
fn rename_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(windows)]
fn identity(path: &Path) -> std::io::Result<String> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(0x02000000 | 0x00200000)
        .open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(format!(
        "{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}
#[cfg(unix)]
fn identity(path: &Path) -> std::io::Result<String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_native_names_only() {
        let fs = FileSnapshots::default();
        for name in ["Game - Copy", "Game - Copy (2)", "Game - Copie (12)"] {
            assert!(fs.recognizes("Game", name));
        }
        for name in [
            "Game2 - Copy",
            "Game - Copy old",
            "Game - Copy (0)",
            "Game - Copy (02)",
            "Game.recovery-12",
            "Game.staging-12",
        ] {
            assert!(!fs.recognizes("Game", name));
        }
    }
    #[test]
    fn rename_never_overwrites_even_an_empty_directory() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("data"), b"keep").unwrap();
        assert!(FileSnapshots::default().rename(&a, &b).is_err());
        assert_eq!(fs::read(a.join("data")).unwrap(), b"keep");
        assert!(b.is_dir());
    }
}
