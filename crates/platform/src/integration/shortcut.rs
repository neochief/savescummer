//! The platform-neutral part of a global shortcut. The host stores the
//! canonical text; platform adapters register the same parsed combination.

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
                        if ch.is_ascii_uppercase() { Some(Key::Letter(ch)) }
                        else if ch.is_ascii_digit() { Some(Key::Digit(ch)) }
                        else { None }
                    } else { None };
                    if key.is_none() { return Err("use F1–F12, A–Z, or 0–9".into()); }
                }
                _ => return Err("use one key and each modifier at most once".into()),
            }
        }
        shortcut.key = key.ok_or("press a key with the shortcut")?;
        if !matches!(shortcut.key, Key::Function(_)) && !shortcut.ctrl && !shortcut.alt && !shortcut.meta {
            return Err("letters and numbers need Ctrl, Alt, or Meta".into());
        }
        #[cfg(target_os = "macos")]
        if (shortcut.ctrl && !shortcut.alt && !shortcut.shift && !shortcut.meta
            && shortcut.key == Key::Function(5))
            || (shortcut.meta && !shortcut.ctrl && !shortcut.alt
                && matches!(shortcut.key, Key::Letter('Q') | Key::Function(3)))
        {
            return Err("this shortcut is reserved by macOS".into());
        }
        #[cfg(windows)]
        if (shortcut.alt && !shortcut.ctrl && !shortcut.shift && !shortcut.meta
            && shortcut.key == Key::Function(4))
            || (shortcut.meta && !shortcut.ctrl && !shortcut.alt && !shortcut.shift
                && matches!(shortcut.key, Key::Letter('L') | Key::Letter('D')))
        {
            return Err("this shortcut is reserved by Windows".into());
        }
        Ok(shortcut)
    }

    pub fn canonical(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl { parts.push("Ctrl".to_string()); }
        if self.alt { parts.push("Alt".to_string()); }
        if self.shift { parts.push("Shift".to_string()); }
        if self.meta { parts.push("Meta".to_string()); }
        parts.push(match self.key {
            Key::Function(number) => format!("F{number}"),
            Key::Letter(letter) | Key::Digit(letter) => letter.to_string(),
        });
        parts.join("+")
    }

    pub fn defaults() -> [Self; 2] {
        let modifier = if cfg!(target_os = "macos") { "Alt" } else { "Ctrl" };
        [Self::parse(&format!("{modifier}+F5")).unwrap(), Self::parse(&format!("{modifier}+F9")).unwrap()]
    }
}

pub fn validate_shortcuts(save: &str, load: &str) -> Result<[Shortcut; 2], String> {
    let save = Shortcut::parse(save).map_err(|error| format!("Save shortcut: {error}"))?;
    let load = Shortcut::parse(load).map_err(|error| format!("Load shortcut: {error}"))?;
    if save == load { return Err("Load shortcut: choose a different shortcut from Save".into()); }
    Ok([save, load])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_and_rejects_ambiguous_shortcuts() {
        let keys = validate_shortcuts("shift + alt + f6", "Ctrl+A").unwrap();
        assert_eq!(keys[0].canonical(), "Alt+Shift+F6");
        assert_eq!(keys[1].canonical(), "Ctrl+A");
        assert!(validate_shortcuts("A", "Ctrl+F9").is_err());
        assert!(validate_shortcuts("Alt+F6", "F6+Alt").is_err());
        assert!(validate_shortcuts("F13", "Ctrl+F9").is_err());
    }
}
