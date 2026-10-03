//! No tray, hotkeys or notifications on this OS yet.

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
    pub fn notify(&self, _title: &str, _text: &str) {}

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
