//! Hotkeys, the tray and sounds: adapters the host owns, so they work with
//! no window open.

use std::sync::Arc;

use savescummer_core::{ErrorKind, Failure};
use savescummer_ipc::{HotkeyAction, Operation};
use savescummer_platform::integration::{self, Signal};

use crate::host::{Host, hotkey_target, new_id};
use crate::ops;

/// Runs what a hotkey press runs: Save or Load on the hotkeys' target, with
/// sounds. With no target it does nothing.
/// `request_id` makes a protocol request safe to repeat; a real key press
/// passes a fresh one.
pub fn hotkey(host: &Arc<Host>, request_id: &str, action: HotkeyAction) -> Result<Operation, Failure> {
    let target = hotkey_target(&host.lock()).map(|(g, _)| g);
    let Some(game) = target else {
        return Err(Failure::new(ErrorKind::NotFound, "no game to act on: no window focus and no running game"));
    };
    let request = match action {
        HotkeyAction::Save => ops::Request::Save { label: None },
        HotkeyAction::Load => ops::Request::Load { checkpoint: None },
    };
    ops::submit(host, request_id, &game, request, true)
}

/// Starts hotkeys and the tray.
pub fn start(host: &Arc<Host>) {
    let weak = Arc::downgrade(host);
    let result = integration::start(Box::new(move |signal| {
        let Some(host) = weak.upgrade() else { return };
        match signal {
            Signal::Hotkey(action) => {
                let action = match action {
                    integration::HotkeyAction::Save => HotkeyAction::Save,
                    integration::HotkeyAction::Load => HotkeyAction::Load,
                };
                // Never block the UI thread with file work.
                std::thread::spawn(move || {
                    let _ = hotkey(&host, &new_id("hotkey"), action);
                });
            }
            Signal::OpenMainWindow => open_main_window(),
            Signal::Exit => host.request_shutdown(),
        }
    }));
    match result {
        Ok(integration) => {
            for error in integration.hotkey_errors() {
                eprintln!("hotkey: {error}");
            }
            *host.integration.lock().unwrap_or_else(|e| e.into_inner()) = Some(integration);
        }
        Err(e) => eprintln!("tray and hotkeys unavailable: {e}"),
    }
}

/// Opens or focuses the desktop UI from the same install folder.
fn open_main_window() {
    let Ok(exe) = std::env::current_exe() else { return };
    let name = if cfg!(windows) { "SaveScummer.exe" } else { "SaveScummer" };
    if let Some(ui) = exe.parent().map(|dir| dir.join(name))
        && ui.exists()
    {
        let _ = std::process::Command::new(ui).spawn();
    }
}

pub fn stop(host: &Host) {
    if let Some(integration) = host.integration.lock().unwrap_or_else(|e| e.into_inner()).take() {
        integration.stop();
    }
}
