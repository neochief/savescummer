//! Windows: known folders through the shell, and the registry's install
//! records (uninstall keys, GOG's games, Steam's ActiveUser).

use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use savescummer_catalog::KnownFolders;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW,
    RegGetValueW, RegOpenKeyExW,
};
use windows_sys::Win32::UI::Shell::{
    FOLDERID_Documents, FOLDERID_LocalAppData, FOLDERID_LocalAppDataLow, FOLDERID_Profile, FOLDERID_ProgramData,
    FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Public, FOLDERID_RoamingAppData, FOLDERID_SavedGames,
    FOLDERID_Windows, SHGetKnownFolderPath,
};
use windows_sys::core::GUID;

use crate::{GogGame, Hive, RegistryKey};

fn known(id: &GUID) -> Option<PathBuf> {
    // SAFETY: SHGetKnownFolderPath allocates the string; we free it.
    unsafe {
        let mut out: *mut u16 = std::ptr::null_mut();
        if SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut out) != 0 || out.is_null() {
            return None;
        }
        let len = (0..).take_while(|&i| *out.add(i) != 0).count();
        let path = std::ffi::OsString::from_wide(std::slice::from_raw_parts(out, len));
        CoTaskMemFree(out as *const _);
        Some(PathBuf::from(path))
    }
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

pub fn reg_string(root: HKEY, key: &str, value: &str) -> Option<String> {
    let (k, v) = (wide(key), wide(value));
    let mut buffer = vec![0u16; 2048];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: the buffer and its byte size are passed together.
    let status = unsafe {
        RegGetValueW(
            root,
            k.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr() as *mut _,
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

fn reg_dword(root: HKEY, key: &str, value: &str) -> Option<u32> {
    let (k, v) = (wide(key), wide(value));
    let mut data = 0u32;
    let mut size = 4u32;
    // SAFETY: a DWORD-sized output buffer.
    let status = unsafe {
        RegGetValueW(
            root,
            k.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    (status == 0).then_some(data)
}

pub fn known_folders() -> KnownFolders {
    let home = known(&FOLDERID_Profile);
    let steam_root = reg_string(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")
        .or_else(|| reg_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"))
        .map(|p| PathBuf::from(p.replace('/', "\\")))
        .and_then(|p| savescummer_snapshots::real_path(&p).ok());
    KnownFolders {
        home,
        appdata: known(&FOLDERID_RoamingAppData),
        localappdata: known(&FOLDERID_LocalAppData),
        locallow: known(&FOLDERID_LocalAppDataLow),
        documents: known(&FOLDERID_Documents),
        public: known(&FOLDERID_Public),
        programdata: known(&FOLDERID_ProgramData),
        programfiles: known(&FOLDERID_ProgramFiles),
        programfiles_x86: known(&FOLDERID_ProgramFilesX86),
        windir: known(&FOLDERID_Windows),
        saved_games: known(&FOLDERID_SavedGames),
        xdg_data_home: None,
        xdg_config_home: None,
        steam_root,
    }
}

pub fn steam_active_user() -> Option<u32> {
    reg_dword(HKEY_CURRENT_USER, r"Software\Valve\Steam\ActiveProcess", "ActiveUser")
}

/// The install folder of one uninstall entry under `root`.
pub fn uninstall_location(root: &RegistryKey, key: &str) -> Option<PathBuf> {
    let hive = match root.hive {
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        Hive::CurrentUser => HKEY_CURRENT_USER,
    };
    let path = format!(r"{}\{key}", root.path);
    // Inno Setup installers don't always fill InstallLocation.
    let text = reg_string(hive, &path, "InstallLocation")
        .filter(|t| !t.trim().is_empty())
        .or_else(|| reg_string(hive, &path, "Inno Setup: App Path"))?;
    let text = text.trim().trim_matches('"').trim_end_matches(['\\', '/']);
    (!text.is_empty()).then(|| PathBuf::from(text))
}

pub fn gog_games() -> Vec<GogGame> {
    let mut out = Vec::new();
    for base in [r"SOFTWARE\WOW6432Node\GOG.com\Games", r"SOFTWARE\GOG.com\Games"] {
        let key = wide(base);
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: the handle is closed below.
        if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, key.as_ptr(), 0, KEY_READ, &mut handle) } != 0 {
            continue;
        }
        let mut index = 0;
        loop {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            // SAFETY: the name buffer and its length are passed together.
            let status = unsafe {
                RegEnumKeyExW(
                    handle,
                    index,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != 0 {
                break;
            }
            index += 1;
            let sub = String::from_utf16_lossy(&name[..len as usize]);
            let full = format!(r"{base}\{sub}");
            let id = reg_string(HKEY_LOCAL_MACHINE, &full, "gameID")
                .and_then(|s| s.parse().ok())
                .or_else(|| sub.parse().ok());
            if let (Some(id), Some(path)) = (id, reg_string(HKEY_LOCAL_MACHINE, &full, "path")) {
                out.push(GogGame { id, path: PathBuf::from(path) });
            }
        }
        // SAFETY: opened above.
        unsafe { RegCloseKey(handle) };
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// Windows Steam keeps ActiveUser in the registry, not in a file.
pub fn steam_registry_file(_folders: &KnownFolders) -> Option<PathBuf> {
    None
}
