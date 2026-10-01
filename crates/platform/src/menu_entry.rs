//! The app's entry in the desktop's app menu, on Linux when the host runs
//! from an AppImage, which installs nothing itself. A distribution package
//! or an AppImage tool installs its own entry; this one is for everyone
//! else.
//!
//! The host writes `~/.local/share/applications/com.savescummer.SaveScummer.desktop`
//! and the icon (`~/.local/share/icons/hicolor/scalable/apps/savescummer.svg`)
//! at every start of a release build, pointing at the AppImage file. Besides
//! the menu, the desktop uses the entry to name the app: the dock groups the
//! window under it (`StartupWMClass`), notifications carry its name and
//! icon, and the shortcuts portal knows the host by its id (see
//! `integration::linux::portal`).
//!
//! Like the autostart entry (`autostart/linux.rs`), ours carries
//! `X-SaveScummer-Target`. An entry with the same name that isn't ours (a
//! distribution's) is left alone, and an entry of ours is rewritten only
//! when it differs: each AppImage version is a new file. Development builds
//! never write one.

use std::path::Path;

/// Writes or updates the menu entry and icon; see the module docs. Never
/// fails the host: a menu entry is a convenience.
pub fn keep_installed(host_exe: &Path) {
    #[cfg(target_os = "linux")]
    if crate::autostart::available()
        && let Some(appimage) = crate::autostart::imp::appimage(host_exe)
        && let Some(data) = linux::data_home()
    {
        linux::install(&data, &appimage);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = host_exe;
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    use crate::autostart::imp::{TARGET_KEY, escape_value, exec_arg, read};

    const FILE: &str = "com.savescummer.SaveScummer.desktop";
    const ICON: &str = "savescummer";
    const ICON_SVG: &[u8] = include_bytes!("../../../assets/icon.svg");
    /// The UI window's class: GTK's program name, which the UI sets to the
    /// app id (its `bridge::run`).
    const WINDOW_CLASS: &str = "com.savescummer.SaveScummer";

    /// `$XDG_DATA_HOME`, `~/.local/share` by default.
    pub fn data_home() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")))
    }

    pub fn install(data: &Path, appimage: &Path) {
        let entry = data.join("applications").join(FILE);
        if entry.exists() && read(&entry).is_none() {
            return; // Not ours.
        }
        write_if_changed(&data.join(format!("icons/hicolor/scalable/apps/{ICON}.svg")), ICON_SVG);
        write_if_changed(&entry, contents(appimage).as_bytes());
    }

    fn write_if_changed(path: &Path, contents: &[u8]) {
        if std::fs::read(path).is_ok_and(|old| old == contents) {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, contents);
    }

    fn contents(appimage: &Path) -> String {
        let target = appimage.to_string_lossy();
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=SaveScummer\n\
             Comment=Checkpoints for your game saves\n\
             Exec={}\n\
             Icon={ICON}\n\
             Terminal=false\n\
             Categories=Utility;\n\
             StartupWMClass={WINDOW_CLASS}\n\
             {TARGET_KEY}={}\n",
            exec_arg(&target),
            escape_value(&target)
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn installs_updates_and_leaves_foreign_entries_alone() {
            let dir = tempfile::tempdir().unwrap();
            let entry = dir.path().join("applications").join(FILE);
            install(dir.path(), Path::new("/apps/SaveScummer-1.AppImage"));
            let text = std::fs::read_to_string(&entry).unwrap();
            assert!(text.contains("Exec=\"/apps/SaveScummer-1.AppImage\"\n"), "{text}");
            assert!(text.contains("StartupWMClass=com.savescummer.SaveScummer\n"), "{text}");
            assert_eq!(read(&entry).unwrap().target, Path::new("/apps/SaveScummer-1.AppImage"));
            assert!(dir.path().join("icons/hicolor/scalable/apps/savescummer.svg").is_file());

            install(dir.path(), Path::new("/apps/SaveScummer-2.AppImage"));
            assert_eq!(read(&entry).unwrap().target, Path::new("/apps/SaveScummer-2.AppImage"));

            let foreign = "[Desktop Entry]\nType=Application\nExec=/usr/bin/savescummer\n";
            std::fs::write(&entry, foreign).unwrap();
            install(dir.path(), Path::new("/apps/SaveScummer-3.AppImage"));
            assert_eq!(std::fs::read_to_string(&entry).unwrap(), foreign);
        }
    }
}
