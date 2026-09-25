//! macOS: the guarded locations, the mount table, and the code signature.

use std::ffi::{CStr, c_void};
use std::io;
use std::path::{Path, PathBuf};

use super::{Category, Table};

pub fn table() -> Table {
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let folder = |rel: &str, category| {
        let path = home.join(rel);
        // The folders themselves aren't guarded, only what's inside: their
        // real paths are safe to resolve.
        (std::fs::canonicalize(&path).unwrap_or(path), category)
    };
    Table {
        folders: vec![
            folder("Documents", Category::Documents),
            folder("Desktop", Category::Desktop),
            folder("Downloads", Category::Downloads),
            folder("Library/Mobile Documents", Category::IcloudDrive),
            folder("Library/Containers", Category::AppData),
            folder("Library/Group Containers", Category::AppData),
        ],
        volumes: Some(PathBuf::from("/Volumes")),
        app_bundles: true,
    }
}

/// Whether a volume is mounted exactly at `path`, from the kernel's mount
/// table: nothing on the volume is touched. `getfsstat` into our own buffer,
/// because `getmntinfo`'s buffer is shared and not thread-safe.
pub fn is_mount_point(path: &Path) -> bool {
    // SAFETY: a null buffer asks for the count only.
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if count <= 0 {
        return false;
    }
    // Room for volumes mounted since.
    // SAFETY: plain-data structs, zeroed.
    let mut mounts: Vec<libc::statfs> = vec![unsafe { std::mem::zeroed() }; count as usize + 8];
    let bytes = (mounts.len() * size_of::<libc::statfs>()) as libc::c_int;
    // SAFETY: the buffer holds `bytes` bytes; MNT_NOWAIT never blocks on a
    // slow or gone network volume.
    let got = unsafe { libc::getfsstat(mounts.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
    mounts.truncate(got.max(0) as usize);
    mounts.iter().any(|m| {
        // SAFETY: a NUL-terminated name inside the fixed-size field.
        let on = unsafe { CStr::from_ptr(m.f_mntonname.as_ptr()) };
        Path::new(&*on.to_string_lossy()) == path
    })
}

/// TCC refuses with `EPERM` ("Operation not permitted"); file permissions
/// give `EACCES`.
pub fn is_privacy_refusal(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::EPERM)
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCodeCopySelf(flags: u32, code: *mut *const c_void) -> i32;
    fn SecCodeCopySigningInformation(code: *const c_void, flags: u32, info: *mut *const c_void) -> i32;
    static kSecCodeInfoUnique: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
    fn CFDataGetLength(data: *const c_void) -> isize;
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFRelease(object: *const c_void);
}

/// The running code's `kSecCodeInfoUnique` (its code directory hash), as
/// hex. Every build has its own, ad-hoc signed or not.
pub fn code_identity() -> Option<String> {
    // SAFETY: Security and CoreFoundation calls following their ownership
    // rules: both copied objects are released, the borrowed data isn't.
    unsafe {
        let mut code = std::ptr::null();
        if SecCodeCopySelf(0, &mut code) != 0 || code.is_null() {
            return None;
        }
        let mut info = std::ptr::null();
        let status = SecCodeCopySigningInformation(code, 0, &mut info);
        CFRelease(code);
        if status != 0 || info.is_null() {
            return None;
        }
        let unique = CFDictionaryGetValue(info, kSecCodeInfoUnique);
        let hex = (!unique.is_null()).then(|| {
            let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(unique), CFDataGetLength(unique) as usize);
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        });
        CFRelease(info);
        hex
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_build_has_an_identity() {
        let id = code_identity().expect("Apple Silicon binaries are always signed");
        assert!(id.len() >= 40, "{id}");
        assert_eq!(code_identity(), Some(id));
    }

    #[test]
    fn the_root_is_mounted_and_home_isnt() {
        assert!(is_mount_point(Path::new("/")));
        assert!(!is_mount_point(&std::env::home_dir().unwrap()));
    }
}
