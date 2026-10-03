//! Linux: the tray icon and global hotkeys (Ctrl+F5, Ctrl+F9).
//!
//! The tray is a StatusNotifierItem ([`tray`]).
//!
//! Hotkeys come two ways. In a Wayland session they go through the desktop
//! portal ([`portal`]) when the desktop has one (GNOME 48+, KDE Plasma 6):
//! they fire whatever window is in front, and the desktop asks the user
//! once to allow them. Starting never opens that dialog: it binds only
//! shortcuts the desktop already allowed, and otherwise waits for
//! [`Integration::set_up_shortcuts`] (first-launch setup) or a changed
//! shortcut. Otherwise they are X11 key grabs through `global-hotkey`, on
//! their own thread: in an X11 session they work everywhere; in a Wayland
//! session they go through XWayland, so they fire only while an X11 window
//! is focused (nearly every game, including all Wine and Proton ones, but
//! not native Wayland apps). Until the portal carries the shortcuts, and if
//! the user declines its dialog, the X11 grabs stand in. Without the portal
//! or any X display there are no hotkeys.

#[path = "linux/portal.rs"]
mod portal;
#[path = "linux/tray.rs"]
mod tray;

use std::sync::{Arc, Mutex, Once, Weak, mpsc};
use std::time::Duration;

use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};

use super::{HotkeyAction, Key, MenuSource, Shortcut, ShortcutSetup, Shortcuts, Signal, hotkeys};

type Handler = Arc<Mutex<Box<dyn Fn(Signal) + Send + 'static>>>;

/// Who hears signals; None when integrations are off.
static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);
/// Shortcut ids the host recorded as allowed through the portal before.
static ALLOWED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The shortcuts the X11 grabs stand for.
static ACTIVE_SHORTCUTS: Mutex<Option<Shortcuts>> = Mutex::new(None);

/// How long starting waits for the portal to look at what the desktop
/// remembers (and bind it again); later answers still arrive.
const PORTAL_START_WAIT: Duration = Duration::from_secs(2);

pub struct Integration {
    tray: Option<tray::Tray>,
    hotkeys: Arc<Mutex<Hotkeys>>,
    hotkey_errors: Vec<String>,
}

#[derive(Default)]
struct Hotkeys {
    /// The portal, when the desktop has one; `portal_bound` once it has
    /// bound, and from then on it alone carries the hotkeys.
    portal: Option<portal::Portal>,
    portal_bound: bool,
    /// Whether the portal answered what the desktop remembers.
    inspected: bool,
    /// X11 grabs, while the portal doesn't carry the hotkeys.
    x11: Option<(GlobalHotKeyManager, Shortcuts)>,
}

impl Hotkeys {
    fn setup(&self) -> ShortcutSetup {
        match (&self.portal, self.portal_bound, self.inspected) {
            (None, _, _) | (Some(_), true, _) => ShortcutSetup::Ready,
            (Some(_), false, true) => ShortcutSetup::Needed,
            (Some(_), false, false) => ShortcutSetup::Unknown,
        }
    }

    /// The portal bound the shortcuts: it alone carries them from now on.
    fn portal_carries(&mut self) {
        self.portal_bound = true;
        self.inspected = true;
        // The manager's thread closes its X connection, which drops the grabs.
        drop(self.x11.take());
    }
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
        let allowed = lock(&ALLOWED).clone();
        portal::start(
            shortcuts,
            allowed,
            |action| emit(Signal::Hotkey(action)),
            |ids| emit(Signal::ShortcutsAllowed(ids)),
        )
        .map_err(|e| hotkey_errors.push(e))
        .ok()
    } else {
        None
    };
    let mut x11 = true;
    if let Some((portal, first)) = started {
        let mut state = lock(&hotkeys);
        state.portal = Some(portal);
        match first.recv_timeout(PORTAL_START_WAIT) {
            Ok(Ok(true)) => {
                state.portal_bound = true;
                state.inspected = true;
                x11 = false;
            }
            Ok(Ok(false)) => state.inspected = true,
            Ok(Err(e)) => {
                state.portal = None;
                hotkey_errors.push(format!("{e}; using X11 key grabs"));
            }
            Err(_) => {
                hotkey_errors.push("the shortcuts portal is slow to answer; X11 key grabs until it does".into());
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

/// Takes the portal's late answer to starting's inspection: bound, it
/// carries the hotkeys; failed, the X11 grabs stay.
fn wait_for_portal(hotkeys: Weak<Mutex<Hotkeys>>, first: mpsc::Receiver<Result<bool, String>>) {
    std::thread::spawn(move || {
        let result = first.recv().unwrap_or_else(|_| Err("the shortcuts portal stopped".into()));
        let Some(hotkeys) = hotkeys.upgrade() else { return };
        let mut state = lock(&hotkeys);
        match result {
            Ok(true) => state.portal_carries(),
            Ok(false) => state.inspected = true,
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

    /// Whether the shortcuts wait for the desktop's dialog, waiting up to
    /// `wait` for the portal to tell.
    pub fn shortcut_setup(&self, wait: Duration) -> ShortcutSetup {
        let deadline = std::time::Instant::now() + wait;
        loop {
            let setup = lock(&self.hotkeys).setup();
            if setup != ShortcutSetup::Unknown || std::time::Instant::now() >= deadline {
                return setup;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Binds the current shortcuts through the portal, which may show the
    /// desktop's dialog. The receiver gets the answer, once the user gave
    /// it: bound only when the desktop bound every one of them; otherwise
    /// the X11 grabs stay.
    pub fn set_up_shortcuts(&self) -> mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        let shortcuts = lock(&ACTIVE_SHORTCUTS).unwrap_or_else(Shortcut::defaults);
        let hotkeys = self.hotkeys.clone();
        std::thread::spawn(move || {
            let _ = tx.send(bind_portal(&hotkeys, shortcuts));
        });
        rx
    }

    /// Through the portal, a changed shortcut waits for the user to allow
    /// it in the desktop's dialog. A portal that doesn't carry the hotkeys
    /// yet is asked too, since the user changed them: if the user declines,
    /// the X11 grabs keep the new ones.
    pub fn rebind(&self, shortcuts: Shortcuts) -> Result<(), String> {
        let mut state = lock(&self.hotkeys);
        if state.portal_bound {
            return state.portal.as_ref().ok_or("hotkeys are unavailable")?.bind(shortcuts);
        }
        let (manager, active) = state.x11.as_mut().ok_or("hotkeys are unavailable")?;
        rebind_x11(manager, active, shortcuts)?;
        let asks = state.portal.is_some() && state.inspected;
        drop(state);
        if asks && let Err(e) = bind_portal(&self.hotkeys, shortcuts) {
            eprintln!("hotkeys: {e}; keeping the X11 key grabs");
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

fn bind_portal(hotkeys: &Mutex<Hotkeys>, shortcuts: Shortcuts) -> Result<(), String> {
    let portal = {
        let state = lock(hotkeys);
        if state.portal_bound {
            return Ok(());
        }
        state.portal.clone().ok_or("the desktop has no shortcuts portal")?
    };
    // Not under the lock: the user may take a while.
    portal.bind(shortcuts)?;
    lock(hotkeys).portal_carries();
    Ok(())
}

fn rebind_x11(manager: &GlobalHotKeyManager, active: &mut Shortcuts, shortcuts: Shortcuts) -> Result<(), String> {
    hotkeys::rebind(manager, *active, shortcuts)?;
    *active = shortcuts;
    *lock(&ACTIVE_SHORTCUTS) = Some(shortcuts);
    Ok(())
}

/// The shortcut ids the host recorded after an earlier bind (see
/// [`Signal::ShortcutsAllowed`]); starting binds them again silently.
pub fn remember_allowed_shortcuts(ids: Vec<String>) {
    *lock(&ALLOWED) = ids;
}

pub fn portal_ids(shortcuts: Shortcuts) -> Vec<String> {
    portal::bound_ids(shortcuts)
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
    let errors = hotkeys::register(&manager, shortcuts);
    listen_to_hotkeys();
    (Some(manager), errors)
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
    hotkeys::action(event, lock(&ACTIVE_SHORTCUTS).unwrap_or_else(Shortcut::defaults))
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::HotKeyState;
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};

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
