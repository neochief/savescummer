//! Small Windows helpers shared by the modules.

use std::ffi::OsStr;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_sys::core::PWSTR;

/// A NUL-terminated UTF-16 copy of `s`, for `PCWSTR` arguments.
pub(crate) fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

/// Copies `s` into a fixed UTF-16 buffer (e.g. a tray tooltip), truncating so
/// the terminating NUL always fits.
pub(crate) fn copy_wide(dst: &mut [u16], s: &str) {
    let src: Vec<u16> = s.encode_utf16().take(dst.len().saturating_sub(1)).collect();
    dst[..src.len()].copy_from_slice(&src);
    dst[src.len()] = 0;
}

/// `%LOCALAPPDATA%` through the known-folder API (follows folder redirection),
/// falling back to the environment variable.
pub(crate) fn local_app_data() -> PathBuf {
    let mut raw: PWSTR = std::ptr::null_mut();
    // SAFETY: on success the API hands us a NUL-terminated string that we own
    // and must free with CoTaskMemFree (also on failure, where it may be null).
    let path = unsafe {
        let hr = SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut raw);
        let path = (hr >= 0 && !raw.is_null()).then(|| {
            let len = (0..).take_while(|&i| *raw.add(i) != 0).count();
            PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len)))
        });
        CoTaskMemFree(raw as *const _);
        path
    };
    path.or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from)).unwrap_or_else(std::env::temp_dir)
}

/// Opens `path` with the shell's "open" verb (Explorer for a folder).
pub(crate) fn shell_open(path: &Path) -> std::io::Result<()> {
    let verb = wide("open");
    let file = wide(path);
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value greater than 32.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("could not open {} (shell error {})", path.display(), result as isize)))
    }
}
