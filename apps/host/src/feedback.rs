//! Hotkeys, the tray and sounds: adapters the host owns, so they work with
//! no window open. And showing the UI, which the tray, a second launch and
//! a user launch all ask for.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use savescummer_core::{ErrorKind, Failure};
use savescummer_ipc::{EventBody, HotkeyAction, Operation};
use savescummer_platform::integration::{self, Signal};
use savescummer_platform::sounds::Cue;

use crate::host::{Host, hotkey_target, new_id};
use crate::ops;

/// Runs what a hotkey press runs: Save or Load on the hotkeys' target, with
/// sounds. With no target it does nothing.
/// `request_id` makes a protocol request safe to repeat; a real key press
/// passes a fresh one.
pub fn hotkey(host: &Arc<Host>, request_id: &str, action: HotkeyAction) -> Result<Operation, Failure> {
    // A game in front that waits for macOS's permission: fail, and never
    // fall through to another game on the stack.
    if let Some(refusal) = crate::privacy::hotkey_refusal(host) {
        ops::cue(host, Cue::Failed);
        return Err(refusal);
    }
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
            Signal::OpenMainWindow => {
                show_ui(&host);
            }
            Signal::Exit => host.request_shutdown(),
        }
    }));
    match result {
        Ok(integration) => {
            for error in integration.hotkey_errors() {
                crate::trace(&format!("hotkey: {error}"));
            }
            *host.integration.lock().unwrap_or_else(|e| e.into_inner()) = Some(integration);
        }
        Err(e) => crate::trace(&format!("tray and hotkeys unavailable: {e}")),
    }
}

/// How long a started UI has to connect before another show request may
/// start a second one.
const UI_START_GRACE: Duration = Duration::from_secs(15);

/// What showing the UI did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// The connected UI was told to come to the front.
    Front,
    /// A UI was started.
    Started,
    /// One was started moments ago and is still connecting.
    Starting,
    /// There is no UI in this install (or it couldn't start).
    Unavailable,
}

impl Shown {
    pub fn as_str(self) -> &'static str {
        match self {
            Shown::Front => "front",
            Shown::Started => "started",
            Shown::Starting => "starting",
            Shown::Unavailable => "unavailable",
        }
    }
}

/// Shows the UI (PLAN-HOST, PROCESSES): a connected UI comes to the front;
/// otherwise one starts.
pub fn show_ui(host: &Arc<Host>) -> Shown {
    {
        let mut inner = host.lock();
        if inner.ui_connections > 0 {
            drop(inner);
            let _ = host.events_tx.send(EventBody::ShowWindow);
            return Shown::Front;
        }
        if inner.ui_started.is_some_and(|t| t.elapsed() < UI_START_GRACE) {
            return Shown::Starting;
        }
        inner.ui_started = Some(Instant::now());
    }
    let Some(mut command) = ui_command() else {
        crate::trace("no UI to show next to the host");
        host.lock().ui_started = None;
        return Shown::Unavailable;
    };
    if let Some(dir) = &host.opts.data_dir {
        command.arg("--data-dir").arg(dir);
    }
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    savescummer_platform::process::detach(&mut command);
    match command.spawn() {
        Ok(mut child) => {
            // Reaped in the background, so it never lingers as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Shown::Started
        }
        Err(e) => {
            crate::trace(&format!("can't start the UI: {e}"));
            host.lock().ui_started = None;
            Shown::Unavailable
        }
    }
}

/// The UI from the same install: `SaveScummer.UI` next to the host, or the
/// AppImage's `ui` mode. Tests name a stand-in with `SAVESCUMMER_UI_EXE`.
fn ui_command() -> Option<Command> {
    if let Some(exe) = std::env::var_os("SAVESCUMMER_UI_EXE") {
        return Some(Command::new(exe));
    }
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let mut command = Command::new(appimage);
        command.arg("ui");
        return Some(command);
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let ui: PathBuf = dir.join(format!("SaveScummer.UI{}", std::env::consts::EXE_SUFFIX));
    ui.is_file().then(|| Command::new(ui))
}

pub fn stop(host: &Host) {
    if let Some(integration) = host.integration.lock().unwrap_or_else(|e| e.into_inner()).take() {
        integration.stop();
    }
}
