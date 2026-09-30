//! The platform-neutral part of a global shortcut. The host stores the
//! canonical text; platform adapters register the same parsed combination.

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Function(u8),
    Letter(char),
    Digit(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: Key,
}

impl Shortcut {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut shortcut = Self { ctrl: false, alt: false, shift: false, meta: false, key: Key::Function(0) };
        let mut key = None;
        for part in text.split('+') {
            let part = part.trim().to_ascii_uppercase();
            match part.as_str() {
                "CTRL" | "CONTROL" if !shortcut.ctrl => shortcut.ctrl = true,
                "ALT" | "OPTION" if !shortcut.alt => shortcut.alt = true,
                "SHIFT" if !shortcut.shift => shortcut.shift = true,
                "META" | "CMD" | "COMMAND" | "WIN" if !shortcut.meta => shortcut.meta = true,
                _ if key.is_none() => {
                    key = if let Some(number) = part.strip_prefix('F').and_then(|s| s.parse::<u8>().ok()) {
                        (1..=12).contains(&number).then_some(Key::Function(number))
                    } else if part.len() == 1 {
                        let ch = part.chars().next().unwrap();
                        if ch.is_ascii_uppercase() {
                            Some(Key::Letter(ch))
                        } else if ch.is_ascii_digit() {
                            Some(Key::Digit(ch))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if key.is_none() {
                        return Err("use F1–F12, A–Z, or 0–9".into());
                    }
                }
                _ => return Err("use one key and each modifier at most once".into()),
            }
        }
        shortcut.key = key.ok_or("press a key with the shortcut")?;
        if !matches!(shortcut.key, Key::Function(_)) && !shortcut.ctrl && !shortcut.alt && !shortcut.meta {
            return Err("letters and numbers need Ctrl, Alt, or Meta".into());
        }
        Ok(shortcut)
    }

    pub fn canonical(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.meta {
            parts.push("Meta".to_string());
        }
        parts.push(match self.key {
            Key::Function(number) => format!("F{number}"),
            Key::Letter(letter) | Key::Digit(letter) => letter.to_string(),
        });
        parts.join("+")
    }

    pub fn defaults() -> Shortcuts {
        let modifier = if cfg!(target_os = "macos") { "Alt" } else { "Ctrl" };
        [Self::parse(&format!("{modifier}+F5")).ok(), Self::parse(&format!("{modifier}+F9")).ok()]
    }
}

/// Save and Load, in that order; None when the user removed one.
pub type Shortcuts = [Option<Shortcut>; 2];

/// The stored text of a shortcut: empty when it's unset.
pub fn shortcut_text(shortcut: Option<Shortcut>) -> String {
    shortcut.map(Shortcut::canonical).unwrap_or_default()
}

pub fn validate_shortcuts(save: &str, load: &str) -> Result<Shortcuts, String> {
    validate_shortcuts_for_platform(save, load, current_platform())
}

fn validate_shortcuts_for_platform(save: &str, load: &str, platform: &str) -> Result<Shortcuts, String> {
    let parse = |text: &str, label: &str| {
        (!text.trim().is_empty())
            .then(|| Shortcut::parse(text).map_err(|error| format!("{label} shortcut: {error}")))
            .transpose()
    };
    let save = parse(save, "Save")?;
    let load = parse(load, "Load")?;
    if save.is_some() && save == load {
        return Err("Load shortcut: choose a different shortcut from Save".into());
    }
    for (label, shortcut) in [("Save", save), ("Load", load)] {
        let Some(shortcut) = shortcut else { continue };
        if let Some(conflict) = shortcut_conflict(platform, shortcut)
            && conflict.severity == "major"
        {
            return Err(format!(
                "{label} shortcut: {} is reserved for {} on {}",
                shortcut.canonical(),
                conflict.description,
                if platform == "macos" { "macOS" } else { "Windows" }
            ));
        }
    }
    Ok([save, load])
}

#[derive(Deserialize)]
struct ShortcutConflict {
    key: String,
    severity: String,
    description: String,
}

#[derive(Deserialize)]
struct ShortcutConflicts {
    windows: Vec<ShortcutConflict>,
    macos: Vec<ShortcutConflict>,
}

fn shortcut_conflict(platform: &str, shortcut: Shortcut) -> Option<&'static ShortcutConflict> {
    let conflicts = conflicts();
    let entries = match platform {
        "windows" => &conflicts.windows,
        "macos" => &conflicts.macos,
        _ => return None,
    };
    let canonical = shortcut.canonical();
    entries.iter().find(|entry| entry.key == canonical)
}

fn conflicts() -> &'static ShortcutConflicts {
    static CONFLICTS: OnceLock<ShortcutConflicts> = OnceLock::new();
    CONFLICTS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../apps/ui/src/shortcuts/shortcut-conflicts.json"))
            .expect("shortcut conflict list must be valid JSON")
    })
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_and_rejects_ambiguous_shortcuts() {
        let keys = validate_shortcuts("shift + alt + f6", "Ctrl+Alt+A").unwrap();
        assert_eq!(shortcut_text(keys[0]), "Alt+Shift+F6");
        assert_eq!(shortcut_text(keys[1]), "Ctrl+Alt+A");
        assert_eq!(validate_shortcuts("", "").unwrap(), [None, None]);
        assert!(validate_shortcuts("A", "Ctrl+F9").is_err());
        assert!(validate_shortcuts("Alt+F6", "F6+Alt").is_err());
        assert!(validate_shortcuts("F13", "Ctrl+F9").is_err());
    }

    #[test]
    fn blocks_major_conflicts_and_allows_minor_conflicts_on_both_platforms() {
        assert!(validate_shortcuts_for_platform("Ctrl+C", "Ctrl+F9", "windows").unwrap_err().contains("Copy"));
        assert!(validate_shortcuts_for_platform("Ctrl+F5", "Meta+L", "windows").unwrap_err().contains("Lock"));
        assert!(validate_shortcuts_for_platform("Meta+Q", "Alt+F9", "macos").unwrap_err().contains("Quit"));
        assert!(validate_shortcuts_for_platform("Alt+F5", "Meta+Shift+4", "macos").unwrap_err().contains("Capture"));
        assert!(validate_shortcuts_for_platform("Meta+G", "Ctrl+F9", "windows").is_ok());
        assert!(validate_shortcuts_for_platform("Meta+F", "Alt+F9", "macos").is_ok());
    }

    #[test]
    fn conflict_list_has_supported_unique_canonical_keys() {
        use std::collections::HashSet;
        let conflicts = conflicts();
        for platform in ["windows", "macos"] {
            let entries = if platform == "windows" { &conflicts.windows } else { &conflicts.macos };
            let mut seen = HashSet::new();
            for entry in entries {
                assert_eq!(Shortcut::parse(&entry.key).unwrap().canonical(), entry.key);
                assert!(matches!(entry.severity.as_str(), "major" | "minor"));
                assert!(seen.insert(&entry.key), "duplicate {platform} shortcut: {}", entry.key);
            }
        }
    }
}
