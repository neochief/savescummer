use std::io;
use std::path::Path;

/// Windows reports "access denied" for a file another program holds open,
/// as well as for real permission problems.
pub const DENIED_MAY_MEAN_IN_USE: bool = true;

/// NOT_READY, BAD_NETPATH, NETNAME_DELETED, BAD_NET_NAME, UNEXP_NET_ERR,
/// DEVICE_NOT_CONNECTED, NO_MEDIA_IN_DRIVE.
pub fn is_unavailable(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(21 | 53 | 64 | 67 | 59 | 1167 | 1112))
}

/// SHARING_VIOLATION, LOCK_VIOLATION.
pub fn is_in_use(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(32 | 33))
}

/// HANDLE_DISK_FULL, DISK_FULL.
pub fn is_disk_full(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(39 | 112))
}

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

#[cfg(test)]
mod tests {
    use super::super::presence;
    use savescummer_core::Presence;
    use std::fs;
    use std::path::Path;

    #[test]
    fn an_absent_drive_is_unknown() {
        // Find a drive letter that doesn't exist on this machine.
        let free = ('D'..='Z').rev().find(|l| fs::metadata(format!("{l}:\\")).is_err()).unwrap();
        assert_eq!(presence(Path::new(&format!("{free}:\\Games\\save"))), Presence::Unknown);
    }
}
