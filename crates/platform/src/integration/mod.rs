//! Global hotkeys, the tray icon and notifications, so they work with no
//! window open. Each OS has its own file; this one holds what they share.
//!
//! - Ctrl+F5 → Save, Ctrl+F9 → Load; ⌥F5 and ⌥F9 on macOS, which reserves
//!   ⌃F5. Each OS file holds its own table. Holding a key triggers once.
//! - Tray: a click opens the main window; its menu has "Main window" and
//!   "Exit".
//! - [`Integration::notify`] shows an OS notification.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Save,
    Load,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Hotkey(HotkeyAction),
    OpenMainWindow,
    Exit,
}

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(any(windows, target_os = "macos")), path = "unsupported.rs")]
mod imp;

pub use imp::{Integration, start};

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
    fn starts_notifies_and_stops() {
        let integration = start(Box::new(|_| {})).expect("started");
        eprintln!("hotkey errors: {:?}", integration.hotkey_errors());
        integration.notify("SaveScummer test", "Integration test notification");
        std::thread::sleep(std::time::Duration::from_millis(500));
        integration.stop();

        // Dropping without stop() must clean up too, and a second start in
        // the same process must work (class already registered).
        let again = start(Box::new(|_| {})).expect("started again");
        drop(again);
    }
}
