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
