//! Global hotkeys and the tray icon, so they work with no
//! window open. Each OS has its own file; this one holds what they share.
//!
//! - Ctrl+F5 → Save, Ctrl+F9 → Load; ⌥F5 and ⌥F9 on macOS, which reserves
//!   ⌃F5. Each OS file holds its own table. Holding a key triggers once.
//! - Tray: a click opens the main window; its menu shows the active game and
//!   the main window's common actions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Save,
    Load,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Hotkey(HotkeyAction),
    TrayGame { game: String, action: TrayGameAction },
    OpenDialog(TrayDialog),
    Scan,
    OpenMainWindow,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayGameAction {
    Play,
    Stop,
    Save,
    Load,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayDialog {
    Add,
    Settings,
    About,
}

/// A fresh snapshot for a menu as it opens. The game id is kept with the
/// action so a focus change while the menu is open cannot retarget a click.
#[derive(Debug, Clone, Default)]
pub struct TrayMenu {
    pub game: Option<String>,
    pub name: Option<String>,
    pub play: bool,
    pub running: bool,
    pub stop: bool,
    pub save: bool,
    pub load: bool,
}

pub type MenuSource = Box<dyn Fn() -> TrayMenu + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy)]
pub enum TrayIcon {
    Play,
    Stop,
    Save,
    Load,
    Main,
    Add,
    Scan,
    Settings,
    About,
}

pub fn icon_png(icon: TrayIcon) -> &'static [u8] {
    match icon {
        TrayIcon::Play => include_bytes!("../../../../assets/tray/triangle.png"),
        TrayIcon::Stop => include_bytes!("../../../../assets/tray/circle-stop.png"),
        TrayIcon::Save => include_bytes!("../../../../assets/tray/flag.png"),
        TrayIcon::Load => include_bytes!("../../../../assets/tray/rotate-left.png"),
        TrayIcon::Main => include_bytes!("../../../../assets/tray/window.png"),
        TrayIcon::Add => include_bytes!("../../../../assets/tray/plus.png"),
        TrayIcon::Scan => include_bytes!("../../../../assets/tray/arrows-rotate.png"),
        TrayIcon::Settings => include_bytes!("../../../../assets/tray/gear.png"),
        TrayIcon::About => include_bytes!("../../../../assets/tray/circle-info.png"),
    }
}

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), path = "unsupported.rs")]
mod imp;
mod shortcut;

pub use imp::{Integration, start};
pub use shortcut::{Key, Shortcut, Shortcuts, shortcut_text, validate_shortcuts};

/// Runs the OS's event loop on the calling thread, which must be the main
/// thread, until `wait` returns (the host waits for shutdown and shuts down
/// in it). Where the tray and hotkeys run on their own thread (Windows),
/// this just calls `wait`; where the OS insists on the main thread (macOS),
/// the loop runs here and `wait` on another thread. On macOS, when the OS
/// asked the app to quit (logout), the process ends once `wait` returns.
pub fn run_main_loop(wait: impl FnOnce() + Send + 'static) {
    imp::run_main_loop(wait)
}

// Windows only: on macOS the tray and hotkeys belong to the main thread,
// which tests don't run on.
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_icon_has_a_small_image() {
        let ico = include_bytes!("../../../../assets/icon.ico");
        let image = imp::ico_image(ico, 16).expect("an image");
        assert!(!image.is_empty());
        assert!(imp::ico_image(b"not an icon", 16).is_none());
    }

    /// Needs a desktop session: adds a real tray icon and registers the
    /// global hotkeys, then removes both.
    #[test]
    #[ignore = "needs a desktop session; run by hand"]
    fn starts_and_stops() {
        let integration = start(Box::new(|_| {}), Box::new(TrayMenu::default), Shortcut::defaults()).expect("started");
        eprintln!("hotkey errors: {:?}", integration.hotkey_errors());
        std::thread::sleep(std::time::Duration::from_millis(500));
        integration.stop();

        // Dropping without stop() must clean up too, and a second start in
        // the same process must work (class already registered).
        let again = start(Box::new(|_| {}), Box::new(TrayMenu::default), Shortcut::defaults()).expect("started again");
        drop(again);
    }
}
