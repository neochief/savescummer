//! No tray or hotkeys on this OS yet.

use super::{MenuSource, Shortcuts, Signal};

pub struct Integration {
    _private: (),
}

pub fn start(
    on_signal: Box<dyn Fn(Signal) + Send + 'static>,
    menu_source: MenuSource,
    _shortcuts: Shortcuts,
) -> Result<Integration, String> {
    let _ = (on_signal, menu_source);
    Err("not supported on this platform yet".into())
}

impl Integration {
    /// Native hotkeys need no permission: nothing to allow.
    pub fn shortcut_setup(&self, _wait: std::time::Duration) -> super::ShortcutSetup {
        super::ShortcutSetup::Ready
    }

    pub fn set_up_shortcuts(&self) -> std::sync::mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = tx.send(Err("the shortcuts need no setup on this platform".into()));
        rx
    }

    pub fn hotkey_errors(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn rebind(&self, _shortcuts: Shortcuts) -> Result<(), String> {
        Err("hotkeys are not supported on this platform yet".into())
    }

    pub fn stop(self) {}
}

pub fn run_main_loop(wait: impl FnOnce() + Send + 'static) {
    wait()
}
