//! macOS and Linux: folders from the environment; no registry.

use std::path::PathBuf;

use savescummer_catalog::KnownFolders;

use crate::{GogGame, RegistryKey};

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

pub fn gog_games() -> Vec<GogGame> {
    Vec::new()
}

pub fn uninstall_location(_root: &RegistryKey, _key: &str) -> Option<PathBuf> {
    None
}

/// Linux Steam keeps the running client's ActiveUser in
/// `~/.steam/registry.vdf`. macOS Steam has a `registry.vdf` too, but whether
/// it records ActiveUser isn't known yet (PLAN-MACOS.md, SCANNER).
pub fn steam_registry_file(folders: &KnownFolders) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return None;
    }
    folders.home.as_ref().map(|home| home.join(".steam").join("registry.vdf"))
}
