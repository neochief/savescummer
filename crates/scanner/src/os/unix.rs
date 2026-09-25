//! macOS and Linux: folders from the environment; no registry. On macOS,
//! GOG Galaxy's installs come from its database.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use savescummer_catalog::KnownFolders;

use crate::{GogGame, RegistryKey};

/// Where GOG Galaxy on macOS records its installs, for every user.
const GALAXY_DB: &str = "/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db";

pub fn known_folders() -> KnownFolders {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg =
        |var: &str, rel: &str| std::env::var_os(var).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(rel)));
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

/// GOG Galaxy has no Linux client, so only macOS lists anything.
pub fn gog_games() -> Vec<GogGame> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    galaxy_games(Path::new(GALAXY_DB))
}

/// The installs a GOG Galaxy database records. Opened read-only and without
/// waiting: a missing, locked or unexpected database lists nothing, and the
/// next scan tries again.
pub fn galaxy_games(db: &Path) -> Vec<GogGame> {
    let read = || -> rusqlite::Result<Vec<GogGame>> {
        let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(Duration::ZERO)?;
        let mut query = connection.prepare("SELECT productId, installationPath FROM InstalledBaseProducts")?;
        let rows = query.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
        Ok(rows
            .flatten()
            .filter_map(|(id, path)| Some(GogGame { id: u64::try_from(id).ok()?, path: PathBuf::from(path) }))
            .collect())
    };
    read().unwrap_or_default()
}

pub fn uninstall_location(_root: &RegistryKey, _key: &str) -> Option<PathBuf> {
    None
}

/// Linux Steam keeps the running client's ActiveUser in
/// `~/.steam/registry.vdf`. macOS Steam has a `registry.vdf` too, but it
/// holds no ActiveUser, not even while a logged-in client runs (checked
/// 2026-09-25: only HKLM `SteamPID` and HKCU settings), so macOS falls back to
/// `loginusers.vdf` (PLAN-MACOS.md, SCANNER). The running account does show
/// in `logs/connection_log.txt` (`Logged On … [U:1:<account id>]`).
pub fn steam_registry_file(folders: &KnownFolders) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return None;
    }
    folders.home.as_ref().map(|home| home.join(".steam").join("registry.vdf"))
}
