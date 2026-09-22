use super::*;
use savescummer_monitor::Process;
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{Com::CoTaskMemFree, Diagnostics::ToolHelp::*, Registry::*, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub fn known_folders() -> BTreeMap<String, PathBuf> {
    let mut folders = BTreeMap::new();
    for (name, id) in [
        ("APPDATA", FOLDERID_RoamingAppData),
        ("LOCALAPPDATA", FOLDERID_LocalAppData),
        ("LOCALLOW", FOLDERID_LocalAppDataLow),
        ("DOCUMENTS", FOLDERID_Documents),
        ("SAVED_GAMES", FOLDERID_SavedGames),
        ("HOME", FOLDERID_Profile),
        ("PUBLIC", FOLDERID_Public),
        ("PROGRAMDATA", FOLDERID_ProgramData),
        ("PROGRAMFILES", FOLDERID_ProgramFiles),
    ] {
        let mut raw = std::ptr::null_mut();
        if unsafe {
            SHGetKnownFolderPath(
                &id,
                KF_FLAG_DONT_VERIFY as u32,
                std::ptr::null_mut(),
                &mut raw,
            )
        } >= 0
        {
            let mut len = 0;
            unsafe {
                while *raw.add(len) != 0 {
                    len += 1;
                }
            }
            let path = PathBuf::from(OsString::from_wide(unsafe {
                std::slice::from_raw_parts(raw, len)
            }));
            unsafe {
                CoTaskMemFree(raw.cast());
            }
            folders.insert(name.into(), path);
        }
    }
    folders
}
fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(Some(0))
        .collect()
}
pub fn set_startup(
    host: &Path,
    data_dir: &Path,
    desktop: Option<&Path>,
    enabled: bool,
) -> std::io::Result<()> {
    if !host.is_absolute() || !data_dir.is_absolute() || !host.is_file() {
        return Err(std::io::Error::other(
            "startup requires an existing absolute host path and absolute data directory",
        ));
    }
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide("SaveScummer");
    let status = if enabled {
        // The trailing slash in a quoted argument must be doubled for argv parsing.
        let quote = |path: &Path| -> std::io::Result<String> {
            let value = path
                .to_str()
                .ok_or_else(|| std::io::Error::other("invalid Unicode in startup path"))?;
            if value.contains(['"', '\0']) {
                return Err(std::io::Error::other("invalid startup path"));
            }
            let trailing = value.chars().rev().take_while(|c| *c == '\\').count();
            Ok(format!("\"{value}{}\"", "\\".repeat(trailing)))
        };
        let mut command = format!(
            "{} --minimized --data-dir {}",
            quote(host)?,
            quote(data_dir)?
        );
        if let Some(desktop) = desktop {
            if !desktop.is_absolute() || !desktop.is_file() {
                return Err(std::io::Error::other("desktop executable is unavailable"));
            }
            command.push_str(&format!(" --desktop {}", quote(desktop)?));
        }
        let command = wide(&command);
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        }
    } else {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) }
    };
    if status == ERROR_SUCCESS || (!enabled && status == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32))
    }
}
/// Windows does not define a `windows-sys` constant for this Win32 error.
const ERROR_UNSUPPORTED_TYPE: u32 = 1630;
/// True when the existing sign-in entry starts this host with this data
/// directory. The app uses this to adopt an externally created entry (the
/// per-user installer) so the persisted preference and the OS registration
/// cannot disagree.
pub fn startup_enabled(host: &Path, data_dir: &Path) -> std::io::Result<bool> {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide("SaveScummer");
    let mut kind = 0u32;
    let mut size = 0u32;
    let probe = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if probe == ERROR_FILE_NOT_FOUND || probe == ERROR_UNSUPPORTED_TYPE {
        // Absent, or stored with an unexpected type (for example REG_EXPAND_SZ
        // written by another tool): not an entry this app owns.
        return Ok(false);
    }
    if probe != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(probe as i32));
    }
    if size == 0 {
        return Ok(false);
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if read != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(read as i32));
    }
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    let value = String::from_utf16_lossy(&buffer[..len]);
    Ok(startup_entry_matches(&value, host, data_dir))
}
/// Splits a Windows command line into arguments, honoring the standard
/// backslash rules around quoted arguments (a backslash run before a closing
/// quote is halved; any other backslashes are literal).
fn command_arguments(command: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                let mut backslashes = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    backslashes += 1;
                }
                if chars.peek() == Some(&'"') {
                    current.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 1 {
                        // An escaped quote inside the argument.
                        current.push('"');
                        chars.next();
                    } else {
                        quoted = !quoted;
                        chars.next();
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                }
            }
            '"' => quoted = !quoted,
            character if character.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    arguments.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        arguments.push(current);
    }
    arguments
}
/// Resolves a path for comparison: canonicalization fixes case and 8.3 short
/// names when the file exists; a failed resolve falls back to the raw path.
fn normalized_path(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_lowercase()
}
/// True when `command` launches `host` with `data_dir`. Used for the sign-in
/// entry, which is always written in that form. An entry without `--data-dir`
/// is accepted so an entry written by an older build is still adopted.
fn startup_entry_matches(command: &str, host: &Path, data_dir: &Path) -> bool {
    let arguments = command_arguments(command);
    let Some(entry_host) = arguments.first() else {
        return false;
    };
    if normalized_path(Path::new(entry_host)) != normalized_path(host) {
        return false;
    }
    match arguments
        .iter()
        .position(|argument| argument == "--data-dir")
        .and_then(|position| arguments.get(position + 1))
    {
        Some(entry_data_dir) => {
            normalized_path(Path::new(entry_data_dir)) == normalized_path(data_dir)
        }
        None => true,
    }
}
pub fn explore(path: &Path) -> std::io::Result<()> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        Err(std::io::Error::other(format!(
            "Explorer could not open the directory ({})",
            result as isize
        )))
    } else {
        Ok(())
    }
}
fn registry_string(root: HKEY, key: &str, name: &str, view: u32) -> Option<PathBuf> {
    let mut bytes = 0;
    let key = wide(key);
    let name = wide(name);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | view;
    if unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    } != 0
    {
        return None;
    }
    let mut buffer = vec![0u16; bytes as usize / 2];
    if unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    } != 0
    {
        return None;
    }
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    Some(PathBuf::from(OsString::from_wide(&buffer[..len])))
}
pub fn steam_roots() -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Some(path) = registry_string(HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath", 0)
    {
        roots.push(path);
    }
    for view in [RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY] {
        if let Some(path) = registry_string(
            HKEY_LOCAL_MACHINE,
            "Software\\Valve\\Steam",
            "InstallPath",
            view,
        ) {
            roots.push(path);
        }
    }
    roots
}
pub fn applications() -> savescummer_scanner::ApplicationScan {
    let mut scan = savescummer_scanner::ApplicationScan::default();
    let uninstall = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for (view, read_view) in [
            (KEY_WOW64_32KEY, RRF_SUBKEY_WOW6432KEY),
            (KEY_WOW64_64KEY, RRF_SUBKEY_WOW6464KEY),
        ] {
            let mut handle = std::ptr::null_mut();
            let status = unsafe {
                RegOpenKeyExW(
                    root,
                    wide(uninstall).as_ptr(),
                    0,
                    KEY_READ | view,
                    &mut handle,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                continue;
            }
            if status != ERROR_SUCCESS {
                scan.errors.push(format!(
                    "cannot read uninstall registry view {view}: {status}"
                ));
                continue;
            }
            let mut index = 0;
            loop {
                let mut key = [0u16; 256];
                let mut size = key.len() as u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        handle,
                        index,
                        key.as_mut_ptr(),
                        &mut size,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if status == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if status != ERROR_SUCCESS {
                    scan.errors.push(format!(
                        "cannot enumerate uninstall registry view {view}: {status}"
                    ));
                    break;
                }
                let key = String::from_utf16_lossy(&key[..size as usize]);
                if let Some(install_dir) = registry_string(
                    root,
                    &format!("{uninstall}\\{key}"),
                    "InstallLocation",
                    read_view,
                ) && install_dir.is_absolute()
                {
                    scan.records
                        .push(savescummer_scanner::ApplicationRecord { key, install_dir });
                }
                index += 1;
            }
            unsafe {
                RegCloseKey(handle);
            }
        }
    }
    scan
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn observe() -> Result<Observation> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(Error::new(
            ErrorCode::Io,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let snapshot = Handle(raw);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut processes = vec![];
    let mut found = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while found {
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
        if !raw.is_null() {
            let handle = Handle(raw);
            let mut buffer = vec![0u16; 32768];
            let mut len = buffer.len() as u32;
            if unsafe { QueryFullProcessImageNameW(handle.0, 0, buffer.as_mut_ptr(), &mut len) }
                != 0
            {
                let path = PathBuf::from(OsString::from_wide(&buffer[..len as usize]));
                let executable = dunce::canonicalize(&path).unwrap_or(path);
                processes.push(Process {
                    pid: entry.th32ProcessID,
                    executable,
                });
            }
        }
        found = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    let mut foreground = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground);
    }
    Ok(Observation {
        processes,
        foreground_pid: (foreground != 0).then_some(foreground),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_command_lines() {
        let arguments = command_arguments(
            r#""C:\Program Files\SaveScummer.Host.exe" --minimized --data-dir "C:\Data\SaveScummer""#,
        );
        assert_eq!(
            arguments,
            vec![
                r"C:\Program Files\SaveScummer.Host.exe",
                "--minimized",
                "--data-dir",
                r"C:\Data\SaveScummer",
            ]
        );
        // A backslash run before the closing quote is halved by the writer.
        assert_eq!(
            command_arguments(r#""C:\ends\\" tail""#),
            vec![r"C:\ends\", "tail"]
        );
        // An odd run escapes a quote; it does not end the argument.
        assert_eq!(command_arguments(r#""say \"hi\"""#), vec![r#"say "hi""#]);
        assert!(command_arguments("").is_empty());
    }

    #[test]
    fn startup_entry_must_match_host_and_data_directory() {
        let temp = tempfile::tempdir().unwrap();
        let host = temp.path().join("SaveScummer.Host.exe");
        let other_host = temp.path().join("NotSaveScummer.Host.exe");
        let data = temp.path().join("Data");
        let other_data = temp.path().join("Other");
        std::fs::write(&host, b"").unwrap();
        std::fs::write(&other_host, b"").unwrap();
        std::fs::create_dir(&data).unwrap();
        std::fs::create_dir(&other_data).unwrap();

        let command = format!(
            "\"{}\" --minimized --data-dir \"{}\"",
            host.display(),
            data.display()
        );
        assert!(startup_entry_matches(&command, &host, &data));
        assert!(!startup_entry_matches(&command, &host, &other_data));
        assert!(!startup_entry_matches(&command, &other_host, &data));

        // Path spelling and case differences still match through resolution.
        let upper = format!(
            "\"{}\" --minimized --data-dir \"{}\"",
            host.display().to_string().to_uppercase(),
            data.display().to_string().to_uppercase()
        );
        assert!(startup_entry_matches(&upper, &host, &data));

        // An entry written before --data-dir was included is still adopted.
        let legacy = format!("\"{}\" --minimized", host.display());
        assert!(startup_entry_matches(&legacy, &host, &data));

        // A longer path containing the host name as a substring must not match.
        let decoy = format!("\"{}\" --minimized", other_host.display());
        assert!(!startup_entry_matches(&decoy, &host, &data));
        assert!(!startup_entry_matches("", &host, &data));
    }
}
