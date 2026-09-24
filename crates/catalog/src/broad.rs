//! The broad-folder rule on path templates (PLAN-CATALOG.md 3.1 rule 7).
//!
//! A Load makes everything a target matches identical to the checkpoint, so
//! a target may never take a whole folder other programs share, or a wildcard
//! directly inside one. An exact name inside such a folder is fine. The host
//! applies the same rule to real paths; this is the builder's and resolver's
//! copy of it, working on templates before anything is resolved.

use crate::glob::has_wildcard;

/// Shared folders, spelled as lowercase templates. Every bare placeholder is
/// broad too (see [`is_broad_folder_template`]).
const SHARED: &[&str] = &[
    "{home}/library",
    "{home}/library/application support",
    "{home}/library/group containers",
    "{home}/library/containers",
    "{home}/library/preferences",
    "{home}/library/caches",
    "{home}/library/saved application state",
    "{home}/appdata",
    "{home}/appdata/roaming",
    "{home}/appdata/local",
    "{home}/appdata/locallow",
    "{home}/appdata/local/programs",
    "{localappdata}/programs",
    "{home}/documents",
    "{home}/documents/my games",
    "{documents}/my games",
    "{home}/my games",
    "{home}/saved games",
    "{home}/desktop",
    "{home}/downloads",
    "{home}/.config",
    "{home}/.local",
    "{home}/.local/share",
    "{home}/.steam",
    "{home}/.var",
    "{home}/.var/app",
    "{xdg_config_home}/unity3d",
    "{home}/.config/unity3d",
    "{appdata}/godot",
    "{appdata}/godot/app_userdata",
    "{xdg_data_home}/godot",
    "{xdg_data_home}/godot/app_userdata",
    "{steam_userdata}",
];

/// Whether a template names a broad folder itself.
pub fn is_broad_folder_template(template: &str) -> bool {
    let normalized = normalize(template);
    if normalized.is_empty() || normalized == "/" {
        return true;
    }
    let segments: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    // A bare placeholder is a whole root.
    if segments.len() == 1 && segments[0].starts_with('{') {
        return true;
    }
    // A top-level absolute folder (`/home`, `/var`) is an OS folder.
    if normalized.starts_with('/') && segments.len() <= 1 {
        return true;
    }
    SHARED.contains(&normalized.as_str())
}

/// Whether a save template would take a broad folder whole, or a wildcard
/// directly inside one.
pub fn is_broad_template(template: &str) -> bool {
    let normalized = normalize(template);
    let segments: Vec<&str> = normalized.split('/').collect();
    match segments.iter().position(|s| has_wildcard(s)) {
        None => is_broad_folder_template(&normalized),
        Some(0) => true,
        Some(first_wildcard) => {
            let root = segments[..first_wildcard].join("/");
            is_broad_folder_template(&root)
        }
    }
}

fn normalize(template: &str) -> String {
    let mut text = template.trim().replace('\\', "/").to_lowercase();
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    while text.contains("//") {
        text = text.replace("//", "/");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_roots_and_shared_folders_are_broad() {
        for t in [
            "{INSTALL_DIR}",
            "{HOME}",
            "{APPDATA}/",
            "{HOME}/Library/Application Support",
            "{HOME}/AppData/LocalLow",
            "{DOCUMENTS}/My Games",
            "{HOME}/.local/share",
            "/",
        ] {
            assert!(is_broad_template(t), "{t}");
        }
    }

    #[test]
    fn wildcards_directly_in_a_broad_folder_are_broad() {
        assert!(is_broad_template("{INSTALL_DIR}/save*"));
        assert!(is_broad_template("{HOME}/Library/Application Support/*"));
        assert!(is_broad_template("{DOCUMENTS}/*.sav"));
    }

    #[test]
    fn exact_names_inside_broad_folders_are_fine() {
        assert!(!is_broad_template("{HOME}/Library/Application Support/com.vlambeer.nuclearthrone"));
        assert!(!is_broad_template("{INSTALL_DIR}/data/save_data.xml"));
        assert!(!is_broad_template("{INSTALL_DIR}/Save.ini"));
        assert!(!is_broad_template("{APPDATA}/Void_War"));
        assert!(!is_broad_template("{INSTALL_DIR}/save/user_*.dat"));
        assert!(!is_broad_template("{STEAM_USERDATA}/588650/remote"));
    }
}
