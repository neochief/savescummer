//! The Linux sign-in entry: an XDG autostart file,
//! `~/.config/autostart/SaveScummer.desktop`, which every mainstream desktop
//! runs at sign-in.
//!
//! Inside an AppImage the host runs from a mount that is gone after it
//! exits; the entry points at the AppImage file instead (`$APPIMAGE`), which
//! runs the host when given no program name. Each version is a new file,
//! so a running AppImage re-points an entry of ours at itself
//! ([`keep_current`]).
//!
//! Ours means it carries `X-SaveScummer-Target`: the program it starts. Only
//! an entry whose target is this program counts as on, and `off` removes
//! only that. The user can also turn it off in their desktop's settings,
//! which set `Hidden=true` or `X-GNOME-Autostart-enabled=false`: that
//! counts as off.

use std::path::{Path, PathBuf};

const FILE: &str = "SaveScummer.desktop";
const TARGET_KEY: &str = "X-SaveScummer-Target";

pub fn set(on: bool, host_exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    let path = entry_path().ok_or("no home folder for the autostart entry")?;
    set_at(&path, on, &target(host_exe), data_dir)
}

pub fn is_enabled(host_exe: &Path) -> bool {
    entry_path().is_some_and(|path| read(&path).is_some_and(|entry| entry.enabled && entry.target == target(host_exe)))
}

/// Re-points an enabled entry of ours at this AppImage, when a different
/// file (another version) wrote it. Outside an AppImage, never changes it.
pub fn keep_current(host_exe: &Path, data_dir: Option<&Path>) {
    let Some(path) = entry_path() else { return };
    let target = target(host_exe);
    if appimage(host_exe).is_none() {
        return;
    }
    if let Some(entry) = read(&path)
        && entry.enabled
        && entry.target != target
    {
        let _ = std::fs::write(&path, contents(&target, data_dir));
    }
}

fn set_at(path: &Path, on: bool, target: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    if on {
        let dir = path.parent().expect("the entry is in a folder");
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        return std::fs::write(path, contents(target, data_dir)).map_err(|e| format!("{}: {e}", path.display()));
    }
    match read(path) {
        Some(entry) if entry.target == target => {
            std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))
        }
        // Not ours, or another program's: nothing to remove.
        _ => Ok(()),
    }
}

/// `$XDG_CONFIG_HOME/autostart`, `~/.config/autostart` by default.
fn entry_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))?;
    Some(config.join("autostart").join(FILE))
}

/// The program the entry starts: the AppImage when the host runs from one.
fn target(host_exe: &Path) -> PathBuf {
    appimage(host_exe).unwrap_or_else(|| host_exe.to_path_buf())
}

/// The AppImage file this host runs from: the runtime sets `$APPIMAGE` and
/// `$APPDIR` (the mount), and the host must be inside that mount (a program
/// the AppImage merely started isn't).
fn appimage(host_exe: &Path) -> Option<PathBuf> {
    let appdir = std::env::var_os("APPDIR")?;
    let appimage = PathBuf::from(std::env::var_os("APPIMAGE")?);
    (host_exe.starts_with(&appdir) && appimage.is_absolute()).then_some(appimage)
}

fn contents(target: &Path, data_dir: Option<&Path>) -> String {
    let mut exec = format!("{} --minimized", exec_arg(&target.to_string_lossy()));
    if let Some(dir) = data_dir {
        exec.push_str(&format!(" --data-dir {}", exec_arg(&dir.to_string_lossy())));
    }
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=SaveScummer\n\
         Comment=Checkpoints for your game saves\n\
         Exec={exec}\n\
         Icon=savescummer\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         {TARGET_KEY}={}\n",
        escape_value(&target.to_string_lossy())
    )
}

/// One `Exec` argument: quoted, with `"`, `` ` ``, `$` and `\` escaped
/// inside the quotes and `%` doubled (the Desktop Entry spec), then escaped
/// again as a string value (its `\` is `\\`).
fn exec_arg(arg: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in arg.chars() {
        match ch {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(ch);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    escape_value(&quoted)
}

/// A Desktop Entry string value: `\`, and line breaks, escaped.
fn escape_value(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r")
}

fn unescape_value(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('s') => out.push(' '),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

struct Entry {
    target: PathBuf,
    enabled: bool,
}

/// Our entry at `path`; None when there's none or it isn't ours.
fn read(path: &Path) -> Option<Entry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_main = false;
    let mut target = None;
    let mut enabled = true;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_main) else { continue };
        match (key.trim(), value.trim()) {
            (TARGET_KEY, value) => target = Some(PathBuf::from(unescape_value(value))),
            ("Hidden", "true") | ("X-GNOME-Autostart-enabled", "false") => enabled = false,
            _ => {}
        }
    }
    Some(Entry { target: target?, enabled })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_writes_an_entry_off_removes_only_ours() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("autostart").join(FILE);
        let exe = Path::new("/opt/Save Scummer/SaveScummer");
        set_at(&path, true, exe, Some(Path::new("/data/$HOME's \"saves\" 100%"))).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(
                r#"Exec="/opt/Save Scummer/SaveScummer" --minimized --data-dir "/data/\\$HOME's \\"saves\\" 100%%""#
            ),
            "{text}"
        );
        let entry = read(&path).unwrap();
        assert_eq!(entry.target, exe);
        assert!(entry.enabled);

        set_at(&path, false, Path::new("/elsewhere/SaveScummer"), None).unwrap();
        assert!(path.exists(), "another program's off leaves ours");
        set_at(&path, false, exe, None).unwrap();
        assert!(!path.exists());
        set_at(&path, false, exe, None).unwrap();
    }

    #[test]
    fn an_entry_turned_off_in_settings_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let exe = Path::new("/opt/SaveScummer");
        set_at(&path, true, exe, None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap().replace("Autostart-enabled=true", "Autostart-enabled=false");
        std::fs::write(&path, text).unwrap();
        assert!(!read(&path).unwrap().enabled);
    }

    #[test]
    fn a_foreign_entry_is_not_ours() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, "[Desktop Entry]\nType=Application\nExec=/usr/bin/savescummer\n").unwrap();
        assert!(read(&path).is_none());
        set_at(&path, false, Path::new("/usr/bin/savescummer"), None).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn values_round_trip() {
        for text in [r"C:\odd\path", "tab\there", "plain"] {
            assert_eq!(unescape_value(&escape_value(text)), text);
        }
    }
}
