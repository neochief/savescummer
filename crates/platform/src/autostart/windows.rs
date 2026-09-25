//! The Windows sign-in entry: a `Run` value under HKCU,
//! `SaveScummer` = `"<host_exe>" --minimized [--data-dir "<dir>"]`.

use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

use crate::win::wide;

/// Where an entry lives under HKCU. Injectable so tests can use a
/// throwaway key instead of the user's real `Run` value.
pub(super) struct Entry {
    pub key: &'static str,
    pub name: &'static str,
}

pub(super) const RUN: Entry = Entry { key: r"Software\Microsoft\Windows\CurrentVersion\Run", name: "SaveScummer" };

pub(super) fn set_at(entry: &Entry, on: bool, host_exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    if on {
        write(entry, &command_line(host_exe, data_dir))
    } else if read(entry).is_some_and(|cmd| points_at(&cmd, host_exe)) {
        delete(entry)
    } else {
        // No entry, or another install's entry: nothing of ours to remove.
        Ok(())
    }
}

pub(super) fn is_enabled_at(entry: &Entry, host_exe: &Path) -> bool {
    read(entry).is_some_and(|cmd| points_at(&cmd, host_exe))
}

/// The value's text, or `None` when it (or its key) doesn't exist.
pub(super) fn read(entry: &Entry) -> Option<String> {
    let key = wide(entry.key);
    let name = wide(entry.name);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut bytes = 0u32;
    // SAFETY: first call only asks for the size; the second fills a buffer
    // of exactly that many bytes. Strings are NUL-terminated.
    unsafe {
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn write(entry: &Entry, command: &str) -> Result<(), String> {
    let key = wide(entry.key);
    let name = wide(entry.name);
    let data = wide(command);
    // SAFETY: `data` is a NUL-terminated UTF-16 string and the byte count
    // includes the terminator, as REG_SZ requires.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(()),
        code => Err(format!("could not write the sign-in entry (error {code})")),
    }
}

fn delete(entry: &Entry) -> Result<(), String> {
    let key = wide(entry.key);
    let name = wide(entry.name);
    // SAFETY: NUL-terminated strings that outlive the call.
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    match status {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        code => Err(format!("could not remove the sign-in entry (error {code})")),
    }
}

pub fn set(on: bool, host_exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    set_at(&RUN, on, host_exe, data_dir)
}

pub fn is_enabled(host_exe: &Path) -> bool {
    is_enabled_at(&RUN, host_exe)
}

/// The command line the entry runs.
fn command_line(host_exe: &Path, data_dir: Option<&Path>) -> String {
    let mut cmd = format!("{} --minimized", quote(&host_exe.to_string_lossy()));
    if let Some(dir) = data_dir {
        cmd.push_str(" --data-dir ");
        cmd.push_str(&quote(&dir.to_string_lossy()));
    }
    cmd
}

/// Quotes one argument for the Windows command-line parser. Paths can't
/// contain `"`, but a trailing backslash (`C:\`) would escape the closing
/// quote, so trailing backslashes are doubled.
fn quote(arg: &str) -> String {
    let trailing = arg.len() - arg.trim_end_matches('\\').len();
    format!("\"{arg}{}\"", "\\".repeat(trailing))
}

/// Whether the entry's command starts with `host_exe` (case-insensitive,
/// either slash).
fn points_at(command: &str, host_exe: &Path) -> bool {
    let command = command.trim_start();
    let exe = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => command.split(' ').next().unwrap_or(""),
    };
    let normalize = |s: &str| s.trim_end_matches('\\').replace('/', "\\").to_lowercase();
    !exe.is_empty() && normalize(exe) == normalize(&host_exe.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A host path that no real entry can point at.
    fn unique_exe() -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        PathBuf::from(format!(r"C:\SaveScummerTest-{}-{nanos}\SaveScummer.exe", std::process::id()))
    }

    #[test]
    fn off_leaves_the_real_run_value_alone() {
        // Only reads the real Run value; the random exe path can never match
        // it, so `off` must leave it exactly as it was.
        let before = read(&RUN);
        let exe = unique_exe();
        assert!(!is_enabled(&exe));
        assert_eq!(set(false, &exe, None), Ok(()));
        assert_eq!(read(&RUN), before);
    }

    #[test]
    fn command_line_quotes_paths() {
        let exe = Path::new(r"C:\Program Files\SaveScummer\SaveScummer.exe");
        assert_eq!(command_line(exe, None), r#""C:\Program Files\SaveScummer\SaveScummer.exe" --minimized"#);
        assert_eq!(
            command_line(exe, Some(Path::new(r"D:\"))),
            r#""C:\Program Files\SaveScummer\SaveScummer.exe" --minimized --data-dir "D:\\""#
        );
    }

    #[test]
    fn points_at_compares_the_exe_case_insensitively() {
        let exe = Path::new(r"C:\Apps\SaveScummer.exe");
        assert!(points_at(r#""c:\apps\savescummer.EXE" --minimized"#, exe));
        assert!(points_at(r#""C:/Apps/SaveScummer.exe""#, exe));
        assert!(points_at(r"C:\Apps\SaveScummer.exe --minimized", exe));
        assert!(!points_at(r#""C:\Other\SaveScummer.exe" --minimized"#, exe));
        assert!(!points_at("", exe));
    }

    /// Exercises the real write path against a throwaway key, never `Run`.
    #[test]
    fn write_and_remove_against_a_throwaway_key() {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};

        struct Cleanup(&'static str);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let key = crate::win::wide(self.0);
                // SAFETY: NUL-terminated key path; deletes only our test key.
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
            }
        }

        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let key: &'static str =
            Box::leak(format!(r"Software\SaveScummerTest-{}-{nanos}", std::process::id()).into_boxed_str());
        let _cleanup = Cleanup(key);
        let entry = Entry { key, name: "SaveScummer" };
        let exe = Path::new(r"C:\Apps\SaveScummer.exe");
        let other = Path::new(r"D:\Other\SaveScummer.exe");

        set_at(&entry, true, exe, Some(Path::new(r"C:\Data"))).unwrap();
        assert_eq!(read(&entry).as_deref(), Some(r#""C:\Apps\SaveScummer.exe" --minimized --data-dir "C:\Data""#));
        assert!(is_enabled_at(&entry, exe));
        assert!(!is_enabled_at(&entry, other));

        // Another install's `off` leaves our entry alone.
        set_at(&entry, false, other, None).unwrap();
        assert!(is_enabled_at(&entry, exe));

        set_at(&entry, false, exe, None).unwrap();
        assert_eq!(read(&entry), None);
        // Removing again is still fine.
        set_at(&entry, false, exe, None).unwrap();
    }
}
