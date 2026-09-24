//! Store discovery: Steam libraries and app manifests, the GOG registry,
//! Epic's launcher manifests and loose install folders. It produces
//! [`Install`] records and the real [`Probe`]; it contains no save policy.
//!
//! Everything the scanner reads about the machine comes from an
//! [`Environment`]: detected from the OS in production, or loaded from a
//! file so tests never touch real game libraries.

pub mod vdf;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use savescummer_catalog::{Bundle, Install, KnownFolders, Platform, Presence, Probe, SteamAccount, Store};

/// A GOG install as the GOG registry (or a test environment) lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GogGame {
    pub id: u64,
    pub path: PathBuf,
}

/// A standalone install as the uninstall registry (or a test environment)
/// lists it: the key name and its install folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallEntry {
    pub key: String,
    pub path: PathBuf,
}

/// A registry hive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

/// A registry key: where the host reads install records and what it watches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryKey {
    pub hive: Hive,
    pub path: String,
}

impl RegistryKey {
    fn new(hive: Hive, path: &str) -> RegistryKey {
        RegistryKey { hive, path: path.to_string() }
    }
}

/// Where Windows keeps installed programs: machine-wide in both registry
/// views, then the user's own.
const UNINSTALL_KEYS: [(Hive, &str); 3] = [
    (Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    (Hive::LocalMachine, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    (Hive::CurrentUser, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
];

/// Where GOG Galaxy lists its games, in both registry views.
const GOG_KEYS: [(Hive, &str); 2] =
    [(Hive::LocalMachine, r"SOFTWARE\WOW6432Node\GOG.com\Games"), (Hive::LocalMachine, r"SOFTWARE\GOG.com\Games")];

/// Where loose installs are probed, and which store an install found there
/// belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LooseRoot {
    pub path: PathBuf,
    pub store: Store,
}

/// Everything discovery may look at.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub platform: Platform,
    pub folders: KnownFolders,
    /// A file holding the running Steam client's account id, standing in for
    /// the registry's `ActiveUser` in tests. Read at every scan and game start.
    #[serde(default)]
    pub steam_active_user_file: Option<PathBuf>,
    /// Read the Windows registry (Steam's ActiveUser, GOG games). Off in
    /// test environments.
    #[serde(default)]
    pub use_registry: bool,
    /// GOG games when the registry isn't used.
    #[serde(default)]
    pub gog_games: Vec<GogGame>,
    /// Uninstall-registry entries given directly (unit tests).
    #[serde(default)]
    pub uninstall: Vec<UninstallEntry>,
    /// Registry keys whose subkeys are uninstall entries: the real three on
    /// Windows, a scratch key in end-to-end tests. Read at every scan and
    /// watched for changes.
    #[serde(default)]
    pub uninstall_keys: Vec<RegistryKey>,
    #[serde(default)]
    pub epic_manifests: Option<PathBuf>,
    #[serde(default)]
    pub loose_roots: Vec<LooseRoot>,
}

impl Environment {
    pub fn from_file(path: &Path) -> Result<Environment, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The real machine.
    pub fn detect() -> Environment {
        let platform = Platform::current();
        let folders = os::known_folders();
        let mut loose_roots = Vec::new();
        if let Some(pf) = &folders.programfiles {
            loose_roots.push(LooseRoot { path: pf.join("Epic Games"), store: Store::Epic });
            loose_roots.push(LooseRoot { path: pf.clone(), store: Store::Standalone });
        }
        if let Some(pf) = &folders.programfiles_x86 {
            loose_roots.push(LooseRoot { path: pf.clone(), store: Store::Standalone });
        }
        if let Some(local) = &folders.localappdata {
            loose_roots.push(LooseRoot { path: local.join("Programs"), store: Store::Standalone });
        }
        if platform == Platform::Windows {
            loose_roots.push(LooseRoot { path: PathBuf::from(r"C:\GOG Games"), store: Store::Gog });
        }
        let epic_manifests = folders
            .programdata
            .as_ref()
            .map(|p| p.join("Epic").join("EpicGamesLauncher").join("Data").join("Manifests"));
        Environment {
            platform,
            folders,
            steam_active_user_file: None,
            use_registry: platform == Platform::Windows,
            gog_games: Vec::new(),
            uninstall: Vec::new(),
            uninstall_keys: if platform == Platform::Windows {
                UNINSTALL_KEYS.iter().map(|(hive, path)| RegistryKey::new(*hive, path)).collect()
            } else {
                Vec::new()
            },
            epic_manifests,
            loose_roots,
        }
    }

    pub fn case_insensitive(&self) -> bool {
        self.platform.case_insensitive()
    }

    /// Steam libraries: the main Steam folder plus `libraryfolders.vdf`.
    pub fn steam_libraries(&self) -> Vec<PathBuf> {
        let Some(root) = &self.folders.steam_root else { return Vec::new() };
        let mut libraries = vec![root.clone()];
        let file = root.join("steamapps").join("libraryfolders.vdf");
        if let Some(map) = fs::read_to_string(&file).ok().and_then(|t| vdf::parse(&t))
            && let Some(folders) = map.map("libraryfolders")
        {
            for (_, entry) in folders.maps() {
                if let Some(path) = entry.text("path") {
                    let path = PathBuf::from(path);
                    if !libraries.iter().any(|l| same_key(l, &path, self.case_insensitive())) {
                        libraries.push(path);
                    }
                }
            }
        }
        libraries
    }

    /// The current Steam account, in the order PLAN-CATALOG.md 4.4 gives:
    /// the running client's ActiveUser, `MostRecent` in loginusers.vdf, the
    /// newest `Timestamp` there, then the only `userdata` folder.
    pub fn steam_account(&self) -> Option<SteamAccount> {
        let root = self.folders.steam_root.as_ref()?;
        if let Some(active) = self.active_user().filter(|id| *id != 0) {
            return Some(SteamAccount { account_id: active });
        }
        if let Some(account) = login_users(&root.join("config").join("loginusers.vdf")) {
            return Some(account);
        }
        let userdata: Vec<u32> = fs::read_dir(root.join("userdata"))
            .ok()?
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
            .filter(|id| *id != 0)
            .collect();
        (userdata.len() == 1).then(|| SteamAccount { account_id: userdata[0] })
    }

    fn active_user(&self) -> Option<u32> {
        if let Some(file) = &self.steam_active_user_file {
            return fs::read_to_string(file).ok()?.trim().parse().ok();
        }
        if self.use_registry {
            return os::steam_active_user();
        }
        if self.platform == Platform::Linux {
            // ~/.steam/registry.vdf: Registry/HKCU/Software/Valve/Steam/ActiveProcess/ActiveUser
            let home = self.folders.home.as_ref()?;
            let text = fs::read_to_string(home.join(".steam").join("registry.vdf")).ok()?;
            let map = vdf::parse(&text)?;
            let active = map.path(&["Registry", "HKCU", "Software", "Valve", "Steam", "ActiveProcess"])?;
            return active.text("ActiveUser")?.parse().ok();
        }
        None
    }

    fn gog(&self) -> Vec<GogGame> {
        if self.use_registry { os::gog_games() } else { self.gog_games.clone() }
    }

    /// The install folder an uninstall-registry key names, if any.
    fn uninstall_location(&self, key: &str) -> Option<PathBuf> {
        self.uninstall
            .iter()
            .find(|e| e.key.eq_ignore_ascii_case(key))
            .map(|e| e.path.clone())
            .or_else(|| self.uninstall_keys.iter().find_map(|root| os::uninstall_location(root, key)))
    }

    /// Shared folders other programs use, for the host's safety checks.
    pub fn broad_folders(&self) -> Vec<PathBuf> {
        let f = &self.folders;
        let mut out: Vec<PathBuf> = [
            &f.home,
            &f.appdata,
            &f.localappdata,
            &f.locallow,
            &f.documents,
            &f.public,
            &f.programdata,
            &f.programfiles,
            &f.programfiles_x86,
            &f.windir,
            &f.saved_games,
            &f.xdg_data_home,
            &f.xdg_config_home,
            &f.steam_root,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        if let Some(home) = &f.home {
            for rel in [
                "AppData",
                "Documents",
                "Documents/My Games",
                "My Games",
                "Saved Games",
                "Desktop",
                "Downloads",
                "Library",
                "Library/Application Support",
                "Library/Group Containers",
                "Library/Containers",
                "Library/Preferences",
                ".config",
                ".local",
                ".local/share",
            ] {
                out.push(home.join(rel));
            }
            if let Some(users) = home.parent() {
                out.push(users.to_path_buf());
            }
        }
        if let Some(docs) = &f.documents {
            out.push(docs.join("My Games"));
        }
        if let Some(local) = &f.localappdata {
            out.push(local.join("Programs"));
        }
        for library in self.steam_libraries() {
            out.push(library.join("steamapps"));
            out.push(library.join("steamapps").join("common"));
            out.push(library.join("steamapps").join("compatdata"));
            out.push(library);
        }
        if let Some(root) = &f.steam_root {
            out.push(root.join("userdata"));
        }
        for loose in &self.loose_roots {
            out.push(loose.path.clone());
        }
        out.into_iter().map(|p| savescummer_snapshots::real_path(&p).unwrap_or(p)).collect()
    }

    /// Store locations worth watching for installs (PLAN-HOST.md, Watched
    /// locations): each library's `steamapps`, the main `libraryfolders.vdf`
    /// and Epic's manifests folder.
    pub fn watch_locations(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.steam_libraries().into_iter().map(|l| l.join("steamapps")).collect();
        if let Some(root) = &self.folders.steam_root {
            out.push(root.join("steamapps").join("libraryfolders.vdf"));
        }
        if let Some(epic) = &self.epic_manifests {
            out.push(epic.clone());
        }
        out
    }

    /// Registry keys worth watching for installs: the uninstall keys and,
    /// when the registry is read, GOG's games key.
    pub fn watch_registry_keys(&self) -> Vec<RegistryKey> {
        let mut out = self.uninstall_keys.clone();
        if self.use_registry {
            out.extend(GOG_KEYS.iter().map(|(hive, path)| RegistryKey::new(*hive, path)));
        }
        out
    }
}

fn login_users(path: &Path) -> Option<SteamAccount> {
    let map = vdf::parse(&fs::read_to_string(path).ok()?)?;
    let users = map.map("users")?;
    let mut best: Option<(u64, u64)> = None; // (timestamp, id64)
    for (id, user) in users.maps() {
        let Ok(id64) = id.parse::<u64>() else { continue };
        if user.text("MostRecent") == Some("1") {
            return SteamAccount::from_id64(id64);
        }
        let stamp = user.text("Timestamp").and_then(|t| t.parse::<u64>().ok()).unwrap_or(0);
        if best.is_none_or(|(s, _)| stamp > s) {
            best = Some((stamp, id64));
        }
    }
    best.and_then(|(_, id)| SteamAccount::from_id64(id))
}

fn same_key(a: &Path, b: &Path, ci: bool) -> bool {
    let key = |p: &Path| {
        let t = p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string();
        if ci { t.to_lowercase() } else { t }
    };
    key(a) == key(b)
}

/// What a discovery pass found.
#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub installs: Vec<Install>,
    /// Store locations that couldn't be read (an unplugged library). Games
    /// whose installs lived there keep their previous state.
    pub unreadable: Vec<PathBuf>,
}

/// Finds every install of every catalog game. Each store's records are read
/// once and looked up by id or folder name, never one check per catalog game
/// per library.
pub fn discover(bundle: &Bundle, env: &Environment) -> Discovery {
    let ci = env.case_insensitive();
    let mut found = Discovery::default();

    let mut by_steam: HashMap<u64, Vec<&str>> = HashMap::new();
    let mut by_gog: HashMap<u64, Vec<&str>> = HashMap::new();
    for game in &bundle.games {
        for id in &game.detect.steam {
            by_steam.entry(*id).or_default().push(&game.id);
        }
        for id in &game.detect.gog {
            by_gog.entry(*id).or_default().push(&game.id);
        }
    }

    // Steam: list each library's steamapps once.
    for library in env.steam_libraries() {
        let steamapps = library.join("steamapps");
        let entries = match fs::read_dir(&steamapps) {
            Ok(entries) => entries,
            Err(_) => {
                if savescummer_snapshots::presence(&steamapps) != Presence::Missing {
                    found.unreadable.push(library.clone());
                }
                continue;
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(appid) = name
                .strip_prefix("appmanifest_")
                .and_then(|s| s.strip_suffix(".acf"))
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            let Some(games) = by_steam.get(&appid) else { continue };
            let Some(manifest) = fs::read_to_string(entry.path()).ok().and_then(|t| vdf::parse(&t)) else { continue };
            let Some(state) = manifest.map("AppState") else { continue };
            let Some(dir) = state.text("installdir") else { continue };
            // Steam writes the manifest when an install starts and rewrites it
            // when it completes. Only a finished install counts; the flag stays
            // set while an installed game updates.
            if !steam_fully_installed(state.text("StateFlags")) {
                continue;
            }
            let install_dir = steamapps.join("common").join(dir);
            if !install_dir.is_dir() {
                continue;
            }
            let proton_prefix = (env.platform == Platform::Linux)
                .then(|| steamapps.join("compatdata").join(appid.to_string()).join("pfx"));
            // The executable is the install evidence (a Proton install runs
            // the Windows build, so any platform's executable counts).
            for game in games.iter().filter(|id| {
                bundle.game(id).is_none_or(|g| {
                    let exes = &g.executables;
                    exes.is_empty()
                        || Platform::ALL
                            .iter()
                            .any(|p| exes.for_platform(*p).iter().any(|exe| install_dir.join(exe).exists()))
                })
            }) {
                found.installs.push(Install {
                    catalog_id: game.to_string(),
                    store: Store::Steam,
                    os: env.platform,
                    install_dir: install_dir.clone(),
                    proton_prefix: proton_prefix.clone(),
                });
            }
        }
    }

    // GOG: the registry lists each install with its id.
    for gog in env.gog() {
        if let Some(games) = by_gog.get(&gog.id)
            && gog.path.is_dir()
        {
            for game in games {
                push_unique(
                    &mut found.installs,
                    Install {
                        catalog_id: game.to_string(),
                        store: Store::Gog,
                        os: env.platform,
                        install_dir: gog.path.clone(),
                        proton_prefix: None,
                    },
                    ci,
                );
            }
        }
    }

    // Epic: one launcher manifest per installed game, matched by folder name
    // and an existing executable.
    let epic = env.epic_manifests.as_deref().map(epic_installs).unwrap_or_default();
    let has_epic_manifests = !epic.is_empty();
    let mut by_dir: BTreeMap<String, Vec<&savescummer_catalog::Game>> = BTreeMap::new();
    for game in &bundle.games {
        for dir in &game.install_dirs {
            by_dir.entry(dir.to_lowercase()).or_default().push(game);
        }
    }
    for location in &epic {
        let Some(folder) = location.file_name().map(|n| n.to_string_lossy().to_lowercase()) else { continue };
        for game in by_dir.get(&folder).into_iter().flatten() {
            if has_executable(game, location, env.platform) {
                push_unique(
                    &mut found.installs,
                    Install {
                        catalog_id: game.id.clone(),
                        store: Store::Epic,
                        os: env.platform,
                        install_dir: location.clone(),
                        proton_prefix: None,
                    },
                    ci,
                );
            }
        }
    }

    // Standalone installers the addendum names by uninstall key. Only games
    // that list a key cost a lookup, and the executable must exist.
    for game in &bundle.games {
        for key in &game.detect.uninstall {
            let Some(location) = env.uninstall_location(key) else { continue };
            if location.is_dir() && has_executable(game, &location, env.platform) {
                push_unique(
                    &mut found.installs,
                    Install {
                        catalog_id: game.id.clone(),
                        store: Store::Standalone,
                        os: env.platform,
                        install_dir: location,
                        proton_prefix: None,
                    },
                    ci,
                );
            }
        }
    }

    // Loose folders, for every game that has them, even when a store install
    // was found: a second copy is its own record.
    for game in &bundle.games {
        for dir in &game.install_dirs {
            for root in &env.loose_roots {
                if root.store == Store::Epic && has_epic_manifests {
                    continue; // the launcher's manifests are the better source
                }
                let candidate = root.path.join(dir);
                if candidate.is_dir() && has_executable(game, &candidate, env.platform) {
                    push_unique(
                        &mut found.installs,
                        Install {
                            catalog_id: game.id.clone(),
                            store: root.store,
                            os: env.platform,
                            install_dir: candidate,
                            proton_prefix: None,
                        },
                        ci,
                    );
                }
            }
        }
    }
    found
}

/// Adds an install unless the same folder is already an install of that game
/// (a loose candidate that is a store install is that install).
fn push_unique(installs: &mut Vec<Install>, install: Install, ci: bool) {
    let real = |p: &Path| savescummer_snapshots::real_path(p).unwrap_or_else(|_| p.to_path_buf());
    let this = real(&install.install_dir);
    if installs.iter().any(|i| i.catalog_id == install.catalog_id && same_key(&real(&i.install_dir), &this, ci)) {
        return;
    }
    installs.push(install);
}

/// Steam's `StateFlags` has bit 4 (fully installed) once an install has
/// completed. A manifest without the field is taken as installed.
fn steam_fully_installed(flags: Option<&str>) -> bool {
    match flags {
        None => true,
        Some(text) => text.trim().parse::<u64>().is_ok_and(|f| f & 4 != 0),
    }
}

fn has_executable(game: &savescummer_catalog::Game, dir: &Path, platform: Platform) -> bool {
    let exes = game.executables.for_platform(platform);
    // A game without listed executables can't be validated by one.
    !exes.is_empty() && exes.iter().any(|exe| dir.join(exe).exists())
}

fn epic_installs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if entry.path().extension().is_none_or(|e| !e.eq_ignore_ascii_case("item")) {
            continue;
        }
        let Some(item) =
            fs::read_to_string(entry.path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            continue;
        };
        if let Some(location) = item.get("InstallLocation").and_then(|v| v.as_str()) {
            out.push(PathBuf::from(location));
        }
    }
    out
}

/// The real machine, as the resolver observes it.
pub struct RealProbe<'a> {
    pub env: &'a Environment,
    account: Option<SteamAccount>,
}

impl<'a> RealProbe<'a> {
    /// Reads the Steam account once, so one scan resolves every game under
    /// the same account.
    pub fn new(env: &'a Environment) -> RealProbe<'a> {
        RealProbe { env, account: env.steam_account() }
    }
}

impl Probe for RealProbe<'_> {
    fn presence(&self, path: &Path) -> Presence {
        savescummer_snapshots::presence(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn same_dir(&self, a: &Path, b: &Path) -> bool {
        let real = |p: &Path| savescummer_snapshots::real_path(p).unwrap_or_else(|_| p.to_path_buf());
        same_key(&real(a), &real(b), self.env.case_insensitive())
    }

    fn install_identity(&self, install_dir: &Path) -> Option<String> {
        savescummer_snapshots::identity(install_dir)
    }

    fn steam_account(&self) -> Option<SteamAccount> {
        self.account
    }

    fn folders(&self) -> KnownFolders {
        self.env.folders.clone()
    }

    fn list_dirs(&self, path: &Path) -> Option<Vec<String>> {
        let entries = fs::read_dir(path).ok()?;
        Some(
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
        )
    }
}

#[cfg(windows)]
mod os {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;

    use savescummer_catalog::KnownFolders;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
        RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
    };
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Documents, FOLDERID_LocalAppData, FOLDERID_LocalAppDataLow, FOLDERID_Profile, FOLDERID_ProgramData,
        FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Public, FOLDERID_RoamingAppData, FOLDERID_SavedGames,
        FOLDERID_Windows, SHGetKnownFolderPath,
    };
    use windows_sys::core::GUID;

    use super::{GogGame, Hive, RegistryKey};

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
}

#[cfg(not(windows))]
mod os {
    use std::path::PathBuf;

    use savescummer_catalog::KnownFolders;

    use super::{GogGame, Hive, RegistryKey};

    pub fn known_folders() -> KnownFolders {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let xdg = |var: &str, rel: &str| {
            std::env::var_os(var).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(rel)))
        };
        let steam_root = home.as_ref().and_then(|h| {
            [h.join(".steam/steam"), h.join(".local/share/Steam"), h.join("Library/Application Support/Steam")]
                .into_iter()
                .find(|p| p.is_dir())
        });
        KnownFolders {
            xdg_data_home: xdg("XDG_DATA_HOME", ".local/share"),
            xdg_config_home: xdg("XDG_CONFIG_HOME", ".config"),
            home,
            steam_root,
            ..Default::default()
        }
    }

    pub fn steam_active_user() -> Option<u32> {
        None
    }

    pub fn gog_games() -> Vec<GogGame> {
        Vec::new()
    }

    pub fn uninstall_location(_root: &RegistryKey, _key: &str) -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests;
