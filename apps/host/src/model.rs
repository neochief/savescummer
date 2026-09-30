//! The host's records: games (known and custom in one model) and settings.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use savescummer_catalog::{Context, Install, Outcome};
use savescummer_core::{Failure, Target};
use savescummer_ipc::GameKind;
use savescummer_platform::privacy::Category;

/// A save location the user typed (an override or a custom game's), with
/// the real folder its root resolved to when it was configured. A link that
/// later points elsewhere fails validation until configured again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserLocation {
    pub text: String,
    pub real_root: PathBuf,
}

/// One game record, persisted as JSON. Known games carry the catalog's
/// answer; custom games carry the user's paths. User choices survive every
/// scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub kind: GameKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    pub installed: bool,
    // Known games: what discovery and the resolver said last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installs: Vec<Install>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identities: Vec<Option<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub catalog_executables: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_tag: Option<String>,
    // User choices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<UserLocation>,
    /// Allows Save, Load, Revert and closing the game while it runs.
    /// Off by default because many games keep progress in memory.
    #[serde(default, skip_serializing_if = "is_false")]
    pub expert_mode: bool,
    /// Creation order, so custom games and records keep a stable order.
    #[serde(default)]
    pub created: u64,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Game {
    /// Existing records used the inverse `wait_for_exit` setting.
    pub fn from_record_json(data: &str) -> serde_json::Result<Self> {
        let mut value: serde_json::Value = serde_json::from_str(data)?;
        if let Some(fields) = value.as_object_mut()
            && let Some(wait) = fields.remove("wait_for_exit").and_then(|value| value.as_bool())
        {
            fields.entry("expert_mode").or_insert_with(|| serde_json::Value::Bool(!wait));
        }
        serde_json::from_value(value)
    }

    pub fn is_custom(&self) -> bool {
        self.kind == GameKind::Custom
    }

    /// The executables the monitor maps to this game.
    pub fn executables(&self) -> Vec<PathBuf> {
        match &self.executable {
            Some(exe) => vec![exe.clone()],
            None => self.catalog_executables.clone(),
        }
    }

    /// The configured executable shown to clients.
    pub fn main_executable(&self) -> Option<PathBuf> {
        self.executable.clone().or_else(|| self.catalog_executables.first().cloned())
    }

    /// Folders holding the game's own files; broad for its targets.
    pub fn install_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self.installs.iter().map(|i| i.install_dir.clone()).collect();
        if self.is_custom()
            && let Some(parent) = self.executable.as_ref().and_then(|e| e.parent())
        {
            dirs.push(parent.to_path_buf());
        }
        dirs
    }

    pub fn store(&self) -> Option<&'static str> {
        self.installs.first().map(|i| i.store.as_str())
    }

    pub fn is_steam(&self) -> bool {
        self.installs.iter().any(|i| i.store == savescummer_catalog::Store::Steam)
    }

    /// Steam must launch its own install for Cloud sync and Proton setup.
    pub fn steam_launch_id(&self) -> Option<u64> {
        if self.executable.is_some() || !self.is_steam() {
            return None;
        }
        self.catalog_id.as_deref()?.strip_prefix("steam-")?.parse().ok()
    }

    /// The folder in the checkpoint store: the name with the id, for people.
    pub fn store_folder(&self) -> String {
        let name = savescummer_snapshots::checkpoint::sanitize(&self.name);
        let id = savescummer_snapshots::checkpoint::sanitize(&self.id.replace('#', "~"));
        format!("{name} ({id})")
    }
}

/// What the host derives from a game's record at every scan and before
/// every operation. Never persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    /// The validated save set operations use, with real roots and presence.
    pub active: Result<Vec<Target>, Failure>,
    pub has_data: bool,
    pub warnings: Vec<String>,
    /// The macOS privacy category the game waits for, with a path that
    /// needs it: the game is inactive until it's granted.
    pub access: Option<(PathBuf, Category)>,
}

impl Default for Derived {
    fn default() -> Self {
        Derived {
            active: Err(Failure::new(savescummer_core::ErrorKind::NoSaveLocation, "not resolved yet")),
            has_data: false,
            warnings: Vec::new(),
            access: None,
        }
    }
}

pub const SETTING_PLAY_SOUNDS: &str = "play_sounds";
pub const SETTING_SAVE_SHORTCUT: &str = "save_shortcut";
pub const SETTING_LOAD_SHORTCUT: &str = "load_shortcut";
pub const SETTING_STORE: &str = "checkpoint_store";
pub const SETTING_LAUNCH: &str = "launch_on_startup";
pub const SETTING_FLUSH_OLD: &str = "flush_old_checkpoints";
pub const SETTING_COUNTER: &str = "game_counter";
/// The mount points of drives the host relies on, as a JSON list.
pub const SETTING_DRIVES: &str = "drives";

#[cfg(test)]
mod tests {
    use super::Game;

    #[test]
    fn old_wait_setting_keeps_its_behavior() {
        let base = r#"{"id":"game","kind":"custom","name":"Game","installed":true"#;
        let enabled = Game::from_record_json(&format!(r#"{base},"wait_for_exit":false}}"#)).unwrap();
        let disabled = Game::from_record_json(&format!(r#"{base},"wait_for_exit":true}}"#)).unwrap();
        let default = Game::from_record_json(&format!("{base}}}")).unwrap();
        assert!(enabled.expert_mode);
        assert!(!disabled.expert_mode);
        assert!(!default.expert_mode);
        let saved = serde_json::to_value(enabled).unwrap();
        assert_eq!(saved["expert_mode"], true);
        assert!(saved.get("wait_for_exit").is_none());
    }
}
