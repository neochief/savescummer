//! Paths as users see and keep them. The user's own folders are written as
//! `~` (macOS, Linux) or `%LOCALAPPDATA%`, `%APPDATA%` and `%USERPROFILE%`
//! (Windows), so stored records and a shared configuration name no user and
//! work on another PC. Every file operation uses the expanded path.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::KnownFolders;

#[derive(Debug, Clone)]
pub struct Portable {
    /// Most specific folder first, so `%LOCALAPPDATA%` wins over `%USERPROFILE%`.
    anchors: Vec<(&'static str, PathBuf)>,
    ci: bool,
}

impl Portable {
    pub fn new(folders: &KnownFolders, ci: bool) -> Portable {
        let candidates = if cfg!(windows) {
            vec![
                ("%LOCALAPPDATA%", &folders.localappdata),
                ("%APPDATA%", &folders.appdata),
                ("%USERPROFILE%", &folders.home),
            ]
        } else {
            vec![("~", &folders.home)]
        };
        let mut anchors: Vec<(&'static str, PathBuf)> =
            candidates.into_iter().filter_map(|(token, dir)| Some((token, dir.clone()?))).collect();
        anchors.sort_by_key(|(_, dir)| std::cmp::Reverse(dir.components().count()));
        Portable { anchors, ci }
    }

    /// `path` with the user's folder written as its token.
    pub fn contract(&self, path: &Path) -> String {
        for (token, dir) in &self.anchors {
            if let Some(rest) = inside(path, dir, self.ci) {
                if rest.as_os_str().is_empty() {
                    return token.to_string();
                }
                return format!("{token}{}{}", std::path::MAIN_SEPARATOR, rest.display());
            }
        }
        path.to_string_lossy().into_owned()
    }

    /// `text` with a leading token replaced by the user's folder.
    pub fn expand(&self, text: &str) -> PathBuf {
        for (token, dir) in &self.anchors {
            let Some(head) = text.get(..token.len()) else { continue };
            let same = if cfg!(windows) { head.eq_ignore_ascii_case(token) } else { head == *token };
            let rest = &text[token.len()..];
            if same && (rest.is_empty() || rest.starts_with(std::path::is_separator)) {
                return dir.join(rest.trim_start_matches(std::path::is_separator));
            }
        }
        PathBuf::from(text)
    }
}

/// What's left of `path` inside `dir`.
fn inside(path: &Path, dir: &Path, ci: bool) -> Option<PathBuf> {
    let mut rest = path.components();
    for want in dir.components() {
        let have = rest.next()?;
        let same = match (want, have) {
            (Component::Normal(a), Component::Normal(b)) if ci => {
                a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
            }
            (Component::Prefix(a), Component::Prefix(b)) if ci => {
                a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
            }
            _ => want == have,
        };
        if !same {
            return None;
        }
    }
    Some(rest.as_path().to_path_buf())
}

static CURRENT: RwLock<Option<Arc<Portable>>> = RwLock::new(None);

/// Sets this process's user folders for records serialized with
/// [`serde_path`]. Until then paths are kept as they are.
pub fn install(portable: Portable) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(portable));
}

fn current() -> Option<Arc<Portable>> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `#[serde(with = "...")]` for a stored path: written portably, read back
/// expanded for this user.
pub mod serde_path {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        match super::current() {
            Some(portable) => serializer.serialize_str(&portable.contract(path)),
            None => serializer.serialize_str(&path.to_string_lossy()),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        let text = String::deserialize(deserializer)?;
        Ok(match super::current() {
            Some(portable) => portable.expand(&text),
            None => PathBuf::from(text),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn portable(home: &str) -> Portable {
        Portable::new(&KnownFolders { home: Some(home.into()), ..Default::default() }, false)
    }

    #[cfg(unix)]
    #[test]
    fn the_home_folder_is_written_as_a_tilde_and_expanded_back() {
        let p = portable("/Users/alex");
        assert_eq!(p.contract(Path::new("/Users/alex/Library/Saves/*.sav")), "~/Library/Saves/*.sav");
        assert_eq!(p.contract(Path::new("/Users/alex")), "~");
        assert_eq!(
            p.contract(Path::new("/Users/alexandra/Saves")),
            "/Users/alexandra/Saves",
            "a folder that only starts alike"
        );
        assert_eq!(p.expand("~/Library/Saves/*.sav"), Path::new("/Users/alex/Library/Saves/*.sav"));
        assert_eq!(p.expand("~"), Path::new("/Users/alex"));
        assert_eq!(p.expand("~bob/Saves"), Path::new("~bob/Saves"), "another user's home is not ours");
        assert_eq!(p.expand("/Applications/Game.app"), Path::new("/Applications/Game.app"));
    }

    #[cfg(windows)]
    #[test]
    fn the_most_specific_user_folder_names_a_path() {
        let p = Portable::new(
            &KnownFolders {
                home: Some(r"C:\Users\Alex".into()),
                appdata: Some(r"C:\Users\Alex\AppData\Roaming".into()),
                localappdata: Some(r"C:\Users\Alex\AppData\Local".into()),
                ..Default::default()
            },
            true,
        );
        assert_eq!(p.contract(Path::new(r"c:\users\alex\AppData\Roaming\Game")), r"%APPDATA%\Game");
        assert_eq!(
            p.contract(Path::new(r"C:\Users\Alex\AppData\LocalLow\Game")),
            r"%USERPROFILE%\AppData\LocalLow\Game"
        );
        assert_eq!(p.expand(r"%appdata%\Game"), Path::new(r"C:\Users\Alex\AppData\Roaming\Game"));
    }
}
