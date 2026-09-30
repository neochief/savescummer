//! Linux: global hotkeys (Ctrl+F5, Ctrl+F9) and notifications. No tray
//! icon yet (PLAN-BUILD.md Linux): opening the app again shows the window.
//!
//! Hotkeys are X11 key grabs through `global-hotkey`, on their own thread.
//! In an X11 session they work everywhere. In a Wayland session they go
//! through XWayland, so they fire while an X11 window is focused: nearly
//! every game, including all Wine and Proton ones, but not native Wayland
//! apps. Without any X display there are no hotkeys.
//!
//! Notifications go through `notify-send` (libnotify), which every
//! mainstream desktop has; without it nothing shows.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, Once};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::{HotkeyAction, Key, Shortcut, Shortcuts, Signal};

type Handler = Arc<Mutex<Box<dyn Fn(Signal) + Send + 'static>>>;

/// Who hears signals; None when integrations are off.
static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);
static ACTIVE_SHORTCUTS: Mutex<Option<Shortcuts>> = Mutex::new(None);

pub struct Integration {
    hotkeys: Mutex<Option<(GlobalHotKeyManager, Shortcuts)>>,
    hotkey_errors: Vec<String>,
}

pub fn start(on_signal: Box<dyn Fn(Signal) + Send + 'static>, shortcuts: Shortcuts) -> Result<Integration, String> {
    *lock(&HANDLER) = Some(Arc::new(Mutex::new(on_signal)));
    let (manager, hotkey_errors) = register_hotkeys(shortcuts);
    *lock(&ACTIVE_SHORTCUTS) = Some(shortcuts);
    Ok(Integration { hotkeys: Mutex::new(manager.map(|m| (m, shortcuts))), hotkey_errors })
}

impl Integration {
    /// Shows a notification. Never blocks.
    pub fn notify(&self, title: &str, text: &str) {
        let child = Command::new("notify-send")
            .args(["--app-name=SaveScummer", "--icon=savescummer", "--", title, text])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = child {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    pub fn hotkey_errors(&self) -> Vec<String> {
        self.hotkey_errors.clone()
    }

    pub fn rebind(&self, shortcuts: Shortcuts) -> Result<(), String> {
        let mut hotkeys = lock(&self.hotkeys);
        let (manager, active) = hotkeys.as_mut().ok_or("hotkeys are unavailable")?;
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

    /// Unregisters the hotkeys.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Integration {
    fn drop(&mut self) {
        *lock(&HANDLER) = None;
        *lock(&ACTIVE_SHORTCUTS) = None;
        // The manager's thread closes its X connection, which drops the grabs.
        drop(lock(&self.hotkeys).take());
    }
}

/// Nothing needs the main thread on Linux.
pub fn run_main_loop(wait: impl FnOnce() + Send + 'static) {
    wait()
}

/// Calls the host's callback. A panic must not end the hotkey thread.
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
