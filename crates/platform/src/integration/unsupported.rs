//! No tray, hotkeys or notifications on this OS yet (PLAN-MACOS.md).

use super::{Shortcut, Signal};

pub struct Integration {
    _private: (),
}

pub fn start(on_signal: Box<dyn Fn(Signal) + Send + 'static>, _shortcuts: [Shortcut; 2]) -> Result<Integration, String> {
    let _ = on_signal;
    Err("not supported on this platform yet".into())
}

impl Integration {
    pub fn notify(&self, _title: &str, _text: &str) {}

    pub fn hotkey_errors(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn rebind(&self, _shortcuts: [Shortcut; 2]) -> Result<(), String> {
        Err("hotkeys are not supported on this platform yet".into())
    }

    pub fn stop(self) {}
}

pub fn run_main_loop(wait: impl FnOnce() + Send + 'static) {
    wait()
}
