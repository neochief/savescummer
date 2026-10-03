//! Hotkeys, the tray and sounds: adapters the host owns, so they work with
//! no window open. And showing the UI, which the tray, a second launch and
//! a user launch all ask for.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use savescummer_core::{ErrorKind, Failure};
use savescummer_ipc::{EventBody, HotkeyAction, Operation, TrayDialogRequest};
use savescummer_platform::integration::{self, Signal, TrayDialog, TrayGameAction, TrayMenu};

use crate::host::{Host, hotkey_target, new_id};
use crate::ops;

/// Runs what a hotkey press runs: Save or Load on the hotkeys' target.
/// With no target it does nothing.
/// `request_id` makes a protocol request safe to repeat; a real key press
/// passes a fresh one.
pub fn hotkey(host: &Arc<Host>, request_id: &str, action: HotkeyAction) -> Result<Operation, Failure> {
    if host.lock().ui.capturing_shortcut {
        return Err(Failure::new(ErrorKind::InvalidRequest, "a shortcut is being edited"));
    }
    let target = hotkey_target(&host.lock()).map(|(g, _)| g);
    let Some(game) = target else {
        return Err(Failure::new(ErrorKind::NotFound, "no game to act on: no window focus and no active game"));
    };
    let request = match action {
        HotkeyAction::Save => ops::Request::Save { label: None },
        HotkeyAction::Load => ops::Request::Load { checkpoint: None },
    };
    ops::submit(host, request_id, &game, request)
}

/// Starts hotkeys and the tray.
pub fn start(host: &Arc<Host>) {
    let weak = Arc::downgrade(host);
    let menu_host = Arc::downgrade(host);
    let shortcuts = host.lock().shortcuts;
    let result = integration::start(
        Box::new(move |signal| {
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
                Signal::TrayGame { game, action } => {
                    std::thread::spawn(move || {
                        let result = match action {
                            TrayGameAction::Play => crate::lifecycle::play(&host, &game).map(|_| ()),
                            TrayGameAction::Stop => crate::lifecycle::close(&host, &game).map(|_| ()),
                            TrayGameAction::Save => {
                                ops::submit(&host, &new_id("tray"), &game, ops::Request::Save { label: None })
                                    .map(|_| ())
                            }
                            TrayGameAction::Load => {
                                ops::submit(&host, &new_id("tray"), &game, ops::Request::Load { checkpoint: None })
                                    .map(|_| ())
                            }
                        };
                        if let Err(error) = result {
                            crate::trace(&format!("tray action failed: {error}"));
                        }
                    });
                }
                Signal::OpenDialog(dialog) => {
                    let kind = match dialog {
                        TrayDialog::Add => "add",
                        TrayDialog::Settings => "settings",
                        TrayDialog::About => "about",
                    };
                    let mut inner = host.lock();
                    inner.tray_dialog = Some(TrayDialogRequest { id: new_id("tray-dialog"), kind: kind.into() });
                    host.publish(&mut inner);
                    drop(inner);
                    show_ui(&host);
                }
                Signal::Scan => {
                    host.scans.request(false, true, "tray menu");
                }
                Signal::OpenMainWindow => {
                    show_ui(&host);
                }
                Signal::Exit => host.request_shutdown(),
            }
        }),
        Box::new(move || menu_host.upgrade().map_or_else(TrayMenu::default, |host| tray_menu(&host))),
        shortcuts,
    );
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

fn tray_menu(host: &Arc<Host>) -> TrayMenu {
    let id = host.lock().stack.active().map(str::to_string);
    let state = host.current_state();
    let Some(game) = id.as_deref().and_then(|id| state.game(id)) else { return TrayMenu::default() };
    TrayMenu {
        game: id,
        name: Some(game.name.clone()),
        play: game.can_play && !game.running && game.busy.is_none(),
        running: game.running,
        stop: game.expert_mode && game.can_close && game.busy.is_none(),
        save: game.save.available,
        load: game.load.available,
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
    // A UI that restarts a development/demo host must keep its simulated
    // machine. Otherwise it could scan the user's actual game folders.
    if let Some(env) = &host.opts.env {
        command.arg("--restart-env").arg(env);
    }
    if host.opts.demo || host.opts.no_integrations {
        command.arg("--restart-no-integrations");
    }
    if host.opts.no_catalog_update {
        command.arg("--restart-no-catalog-update");
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
