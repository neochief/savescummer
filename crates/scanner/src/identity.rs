//! Follow filesystem aliases to identify the installation itself. Comparing
//! spelling (including case-folded spelling) cannot establish directory identity.

use std::{io, path::Path};

/// Compare existing directory aliases and missing descendants without folding
/// case globally. The nearest existing ancestor supplies filesystem identity.
pub fn location(path: &Path) -> io::Result<((u64, u64), Vec<std::ffi::OsString>)> {
    let mut ancestor = path;
    let mut tail = vec![];
    loop {
        match std::fs::metadata(ancestor) {
            Ok(_) => return Ok((directory(ancestor)?, tail)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tail.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| io::Error::other("no existing ancestor"))?
                        .to_os_string(),
                );
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| io::Error::other("no existing ancestor"))?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
pub fn directory(path: &Path) -> io::Result<(u64, u64)> {
    use std::{
        fs::OpenOptions,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        GetFileInformationByHandle,
    };

    // Opening without OPEN_REPARSE_POINT follows junctions and symbolic links.
    let directory = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(directory.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(unix)]
pub fn directory(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    // metadata follows symlinks, unlike the snapshot engine's identity checks.
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}
