//! Linux: the tray icon and global hotkeys (Ctrl+F5, Ctrl+F9).
//!
//! The tray is a StatusNotifierItem ([`tray`]).
//!
//! Hotkeys come two ways. In a Wayland session they go through the desktop
//! portal ([`portal`]) when the desktop has one (GNOME 48+, KDE Plasma 6):
//! they fire whatever window is in front, and the desktop asks the user
//! once to allow them. Otherwise they are X11 key grabs through
//! `global-hotkey`, on their own thread: in an X11 session they work
//! everywhere; in a Wayland session they go through XWayland, so they fire
//! only while an X11 window is focused (nearly every game, including all
//! Wine and Proton ones, but not native Wayland apps). While the desktop's
//! dialog waits for the user, and if the user declines it, the X11 grabs
//! stand in. Without the portal or any X display there are no hotkeys.

#[path = "linux/portal.rs"]
mod portal;
#[path = "linux/tray.rs"]
mod tray;

use std::sync::{Arc, Mutex, Once, Weak};
use std::time::Duration;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::{HotkeyAction, Key, MenuSource, Shortcut, Shortcuts, Signal};

type Handler = Arc<Mutex<Box<dyn Fn(Signal) + Send + 'static>>>;

/// Who hears signals; None when integrations are off.
static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);
/// The shortcuts the X11 grabs stand for.
static ACTIVE_SHORTCUTS: Mutex<Option<Shortcuts>> = Mutex::new(None);

/// How long starting waits for the portal: enough when the user already
/// allowed the shortcuts, not for the user to answer the dialog.
const PORTAL_START_WAIT: Duration = Duration::from_secs(2);

pub struct Integration {
    tray: Option<tray::Tray>,
    hotkeys: Arc<Mutex<Hotkeys>>,
    hotkey_errors: Vec<String>,
}

#[derive(Default)]
struct Hotkeys {
    /// The portal, once it was asked to bind; `portal_bound` once it has
    /// bound, and from then on it alone carries the hotkeys.
    portal: Option<portal::Portal>,
    portal_bound: bool,
    /// X11 grabs, while the portal doesn't carry the hotkeys.
    x11: Option<(GlobalHotKeyManager, Shortcuts)>,
}

pub fn start(
    on_signal: Box<dyn Fn(Signal) + Send + 'static>,
    menu_source: MenuSource,
    shortcuts: Shortcuts,
) -> Result<Integration, String> {
    *lock(&HANDLER) = Some(Arc::new(Mutex::new(on_signal)));
    *lock(&ACTIVE_SHORTCUTS) = Some(shortcuts);
    let mut hotkey_errors = Vec::new();
    let tray = tray::Tray::start(menu_source).map_err(|e| hotkey_errors.push(e)).ok();
    let hotkeys = Arc::new(Mutex::new(Hotkeys::default()));
    let started = if portal::wanted() {
        portal::start(shortcuts, |action| emit(Signal::Hotkey(action))).map_err(|e| hotkey_errors.push(e)).ok()
    } else {
        None
    };
    let mut x11 = true;
    if let Some((portal, first)) = started {
        let mut state = lock(&hotkeys);
        match first.recv_timeout(PORTAL_START_WAIT) {
            Ok(Ok(())) => {
                state.portal = Some(portal);
                state.portal_bound = true;
                x11 = false;
            }
            Ok(Err(e)) => hotkey_errors.push(format!("{e}; using X11 key grabs")),
            Err(_) => {
                hotkey_errors.push("waiting for the desktop to allow the shortcuts; X11 key grabs until then".into());
                state.portal = Some(portal);
                wait_for_portal(Arc::downgrade(&hotkeys), first);
            }
        }
    }
    if x11 {
        let (manager, errors) = register_hotkeys(shortcuts);
        hotkey_errors.extend(errors);
        lock(&hotkeys).x11 = manager.map(|m| (m, shortcuts));
    }
    Ok(Integration { tray, hotkeys, hotkey_errors })
}

/// Hands the hotkeys to the portal once the user allows them, or drops it
/// if they don't.
fn wait_for_portal(hotkeys: Weak<Mutex<Hotkeys>>, first: std::sync::mpsc::Receiver<Result<(), String>>) {
    std::thread::spawn(move || {
        let result = first.recv().unwrap_or_else(|_| Err("the shortcuts portal stopped".into()));
        let Some(hotkeys) = hotkeys.upgrade() else { return };
        let mut state = lock(&hotkeys);
        match result {
            Ok(()) => {
                state.portal_bound = true;
                // The manager's thread closes its X connection, which drops the grabs.
                drop(state.x11.take());
            }
            Err(e) => {
                state.portal = None;
                eprintln!("hotkeys: {e}; keeping the X11 key grabs");
            }
        }
    });
}

impl Integration {
    pub fn hotkey_errors(&self) -> Vec<String> {
        self.hotkey_errors.clone()
    }

    /// Through the portal, a changed shortcut waits for the user to allow
    /// it in the desktop's dialog.
    pub fn rebind(&self, shortcuts: Shortcuts) -> Result<(), String> {
        let mut state = lock(&self.hotkeys);
        if state.portal_bound {
            return state.portal.as_ref().ok_or("hotkeys are unavailable")?.bind(shortcuts);
        }
        let (manager, active) = state.x11.as_mut().ok_or("hotkeys are unavailable")?;
        rebind_x11(manager, active, shortcuts)?;
        // Still waiting for the user: the portal binds these next.
        if let Some(portal) = &state.portal {
            portal.bind_later(shortcuts);
        }
        Ok(())
    }

    /// Removes the tray icon and unregisters the hotkeys.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Integration {
    fn drop(&mut self) {
        *lock(&HANDLER) = None;
        *lock(&ACTIVE_SHORTCUTS) = None;
        drop(self.tray.take());
        // The portal's thread closes its session, and the manager's its X
        // connection, which releases the shortcuts.
        drop(std::mem::take(&mut *lock(&self.hotkeys)));
    }
}

fn rebind_x11(manager: &GlobalHotKeyManager, active: &mut Shortcuts, shortcuts: Shortcuts) -> Result<(), String> {
    let old = *active;
    for shortcut in old.into_iter().flatten() {
        let _ = manager.unregister(native_shortcut(shortcut));
    }
    let mut registered = Vec::new();
    for shortcut in shortcuts.into_iter().flatten() {
        let hotkey = native_shortcut(shortcut);
        if let Err(error) = manager.register(hotkey) {
            for hotkey in registered {
                let _ = manager.unregister(hotkey);
            }
            for shortcut in old.into_iter().flatten() {
                let _ = manager.register(native_shortcut(shortcut));
            }
            return Err(format!("{} is unavailable: {error}", shortcut.canonical()));
        }
        registered.push(hotkey);
    }
    *active = shortcuts;
    *lock(&ACTIVE_SHORTCUTS) = Some(shortcuts);
    Ok(())
}

/// Nothing needs the main thread on Linux.
pub fn run_main_loop(wait: impl FnOnce() + Send + 'static) {
    wait()
}

/// Calls the host's callback. A panic must not end the hotkey, portal or
/// tray thread.
fn emit(signal: Signal) {
    let Some(handler) = lock(&HANDLER).clone() else { return };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (lock(&handler))(signal)));
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn register_hotkeys(shortcuts: Shortcuts) -> (Option<GlobalHotKeyManager>, Vec<String>) {
    // `global-hotkey` reports a missing display only on its own thread, as
    // registrations that silently succeed: ask first.
    if let Err(e) = x11rb::connect(None) {
        return (None, vec![format!("hotkeys need an X11 display (an X11 session, or XWayland): {e}")]);
    }
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(e) => return (None, vec![format!("hotkeys are unavailable: {e}")]),
    };
    let mut errors = Vec::new();
    for shortcut in shortcuts.into_iter().flatten() {
        if let Err(e) = manager.register(native_shortcut(shortcut)) {
            errors.push(format!("{} is unavailable: another app already uses it ({e})", shortcut.canonical()));
        }
    }
    listen_to_hotkeys();
    (Some(manager), errors)
}

fn native_shortcut(shortcut: Shortcut) -> HotKey {
    let mut modifiers = Modifiers::empty();
    if shortcut.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if shortcut.alt {
        modifiers |= Modifiers::ALT;
    }
    if shortcut.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if shortcut.meta {
        modifiers |= Modifiers::SUPER;
    }
    let name = match shortcut.key {
        Key::Function(number) => format!("F{number}"),
        Key::Letter(letter) => format!("Key{letter}"),
        Key::Digit(digit) => format!("Digit{digit}"),
    };
    let code: Code = name.parse().expect("validated shortcut key");
    HotKey::new((!modifiers.is_empty()).then_some(modifiers), code)
}

/// `global-hotkey` takes one handler per process: it forwards to whoever
/// listens now.
fn listen_to_hotkeys() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
            if let Some(action) = hotkey_action(event) {
                emit(Signal::Hotkey(action));
            }
        }));
    });
}

/// Each press is one action. The X11 backend asks for detectable
/// auto-repeat and reports a held key once, until it's released.
fn hotkey_action(event: GlobalHotKeyEvent) -> Option<HotkeyAction> {
    if event.state != HotKeyState::Pressed {
        return None;
    }
    let shortcuts = lock(&ACTIVE_SHORTCUTS).unwrap_or_else(Shortcut::defaults);
    shortcuts
        .iter()
        .position(|shortcut| shortcut.is_some_and(|shortcut| native_shortcut(shortcut).id() == event.id))
        .map(|index| if index == 0 { HotkeyAction::Save } else { HotkeyAction::Load })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_presses_map_to_their_actions() {
        let save = HotKey::new(Some(Modifiers::CONTROL), Code::F5).id();
        let load = HotKey::new(Some(Modifiers::CONTROL), Code::F9).id();
        assert_eq!(
            hotkey_action(GlobalHotKeyEvent { id: save, state: HotKeyState::Pressed }),
            Some(HotkeyAction::Save)
        );
        assert_eq!(
            hotkey_action(GlobalHotKeyEvent { id: load, state: HotKeyState::Pressed }),
            Some(HotkeyAction::Load)
        );
        assert_eq!(hotkey_action(GlobalHotKeyEvent { id: save, state: HotKeyState::Released }), None);
    }
}
