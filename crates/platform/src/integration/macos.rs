//! The macOS menu-bar item, global hotkeys (⌥F5, ⌥F9), notifications,
//! reopen and quit, all on the main thread's `NSApplication` run loop.
//!
//! The host's main thread runs that loop for the whole run
//! ([`run_main_loop`]), also without integrations: `NSWorkspace` only tracks
//! the frontmost app while it runs (see the monitor's macOS source).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};

use block2::RcBlock;
use dispatch2::{DispatchQueue, run_on_main};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSApplicationTerminateReply, NSBitmapImageRep,
    NSEvent, NSEventModifierFlags, NSEventType, NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSError, NSObjectProtocol, NSPoint, NSSize, NSString, NSUUID};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::{
    HotkeyAction, Key, MenuSource, Shortcut, Shortcuts, Signal, TrayDialog, TrayGameAction, TrayIcon, icon_png,
};

/// The menu-bar template, 22 × 22 points at 1x and 2x.
const ICON_1X: &[u8] = include_bytes!("../../../../assets/macos/SaveScummerTemplate.png");
const ICON_2X: &[u8] = include_bytes!("../../../../assets/macos/SaveScummerTemplate@2x.png");
const ICON_POINTS: f64 = 22.0;

type Handler = Arc<Mutex<Box<dyn Fn(Signal) + Send + 'static>>>;

/// Who hears signals; None when integrations are off.
static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);
static MENU_SOURCE: Mutex<Option<MenuSource>> = Mutex::new(None);
static MENU_GAME: Mutex<Option<String>> = Mutex::new(None);
static ACTIVE_SHORTCUTS: Mutex<Option<Shortcuts>> = Mutex::new(None);
/// AppKit asked to quit (logout, or Quit from the Dock) and waits for the
/// answer, which is given once the host has shut down.
static TERMINATING: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// What lives on the main thread while integrations are on.
    static MAIN: std::cell::RefCell<Option<MainState>> = const { std::cell::RefCell::new(None) };
}

struct MainState {
    bar: Retained<NSStatusItem>,
    hotkeys: Option<GlobalHotKeyManager>,
    shortcuts: Shortcuts,
}

pub struct Integration {
    hotkey_errors: Vec<String>,
}

/// Runs `NSApplication` on the main thread until `wait` returns on another.
/// If AppKit is waiting to quit, answering it ends the process.
pub fn run_main_loop(wait: impl FnOnce() + Send + 'static) {
    let Some(mtm) = MainThreadMarker::new() else {
        return wait();
    };
    let app = application(mtm);
    std::thread::spawn(move || {
        wait();
        DispatchQueue::main().exec_async(|| {
            let mtm = MainThreadMarker::new().expect("on the main queue");
            let app = NSApplication::sharedApplication(mtm);
            if TERMINATING.load(Ordering::SeqCst) {
                app.replyToApplicationShouldTerminate(true);
                return;
            }
            app.stop(None);
            // `stop` takes effect after the next event.
            if let Some(event) = wake_event() {
                app.postEvent_atStart(&event, true);
            }
        });
    });
    app.run();
}

/// The shared application, set up once: AppKit needs it before any status
/// item or image.
fn application(mtm: MainThreadMarker) -> Retained<NSApplication> {
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon and no menu bar of its own (the bundle is LSUIElement too).
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate(mtm))));
    app
}

fn wake_event() -> Option<Retained<NSEvent>> {
    NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        0,
        0,
    )
}

/// Adds the menu-bar item and registers the hotkeys. Must be called on the
/// main thread, before [`run_main_loop`].
pub fn start(
    on_signal: Box<dyn Fn(Signal) + Send + 'static>,
    menu_source: MenuSource,
    shortcuts: Shortcuts,
) -> Result<Integration, String> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err("the menu bar and hotkeys must start on the main thread".into());
    };
    application(mtm);
    *lock(&HANDLER) = Some(Arc::new(Mutex::new(on_signal)));
    *lock(&MENU_SOURCE) = Some(menu_source);
    let bar = status_item(mtm);
    let (hotkeys, hotkey_errors) = register_hotkeys(shortcuts);
    *lock(&ACTIVE_SHORTCUTS) = Some(shortcuts);
    MAIN.with(|main| *main.borrow_mut() = Some(MainState { bar, hotkeys, shortcuts }));
    Ok(Integration { hotkey_errors })
}

impl Integration {
    /// Shows a notification. It asks for permission the first time; without
    /// it, or outside an app bundle, nothing shows. Never blocks.
    pub fn notify(&self, title: &str, text: &str) {
        let (title, text) = (title.to_string(), text.to_string());
        DispatchQueue::main().exec_async(move || notify_now(&title, &text));
    }

    pub fn hotkey_errors(&self) -> Vec<String> {
        self.hotkey_errors.clone()
    }

    pub fn rebind(&self, shortcuts: Shortcuts) -> Result<(), String> {
        run_on_main(move |_| {
            MAIN.with(|main| {
                let mut main = main.borrow_mut();
                let state = main.as_mut().ok_or("hotkeys are not running")?;
                let manager = state.hotkeys.as_ref().ok_or("hotkeys are unavailable")?;
                let old = state.shortcuts;
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
                state.shortcuts = shortcuts;
                *lock(&ACTIVE_SHORTCUTS) = Some(shortcuts);
                Ok(())
            })
        })
    }

    /// Removes the menu-bar item and unregisters the hotkeys.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Integration {
    fn drop(&mut self) {
        *lock(&HANDLER) = None;
        *lock(&MENU_SOURCE) = None;
        *lock(&MENU_GAME) = None;
        *lock(&ACTIVE_SHORTCUTS) = None;
        run_on_main(|mtm| {
            if let Some(state) = MAIN.with(|main| main.borrow_mut().take()) {
                // A menu open right now closes first.
                if let Some(menu) = state.bar.menu(mtm) {
                    menu.cancelTracking();
                }
                NSStatusBar::systemStatusBar().removeStatusItem(&state.bar);
                drop(state.hotkeys);
            }
        });
    }
}

/// Calls the host's callback. A panic must not unwind into AppKit.
fn emit(signal: Signal) {
    let Some(handler) = lock(&HANDLER).clone() else { return };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (lock(&handler))(signal)));
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// --- The app delegate: reopen, quit, and the menu-bar item's actions.

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SaveScummerAppDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSMenuDelegate for Delegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            rebuild_menu(menu);
        }
    }

    unsafe impl NSApplicationDelegate for Delegate {
        /// Opening the app again while it runs (Finder, Spotlight, the Dock).
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            emit(Signal::OpenMainWindow);
            false
        }

        /// Logout or Quit: shut down safely first, answer AppKit after.
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            if lock(&HANDLER).is_none() {
                return NSApplicationTerminateReply::TerminateNow;
            }
            TERMINATING.store(true, Ordering::SeqCst);
            emit(Signal::Exit);
            NSApplicationTerminateReply::TerminateLater
        }
    }

    impl Delegate {
        #[unsafe(method(play:))]
        fn play(&self, _sender: Option<&AnyObject>) { emit_game(TrayGameAction::Play); }

        #[unsafe(method(stopGame:))]
        fn stop_game(&self, _sender: Option<&AnyObject>) { emit_game(TrayGameAction::Stop); }

        #[unsafe(method(save:))]
        fn save(&self, _sender: Option<&AnyObject>) { emit_game(TrayGameAction::Save); }

        #[unsafe(method(load:))]
        fn load(&self, _sender: Option<&AnyObject>) { emit_game(TrayGameAction::Load); }

        #[unsafe(method(addGame:))]
        fn add_game(&self, _sender: Option<&AnyObject>) { emit(Signal::OpenDialog(TrayDialog::Add)); }

        #[unsafe(method(scan:))]
        fn scan(&self, _sender: Option<&AnyObject>) { emit(Signal::Scan); }

        #[unsafe(method(settings:))]
        fn settings(&self, _sender: Option<&AnyObject>) { emit(Signal::OpenDialog(TrayDialog::Settings)); }

        #[unsafe(method(about:))]
        fn about(&self, _sender: Option<&AnyObject>) { emit(Signal::OpenDialog(TrayDialog::About)); }

        #[unsafe(method(openMainWindow:))]
        fn open_main_window(&self, _sender: Option<&AnyObject>) {
            emit(Signal::OpenMainWindow);
        }

        #[unsafe(method(exit:))]
        fn exit(&self, _sender: Option<&AnyObject>) {
            emit(Signal::Exit);
        }
    }
);

/// The one delegate, alive for the process's lifetime (AppKit holds it
/// weakly).
fn delegate(mtm: MainThreadMarker) -> Retained<Delegate> {
    thread_local! {
        static DELEGATE: std::cell::OnceCell<Retained<Delegate>> = const { std::cell::OnceCell::new() };
    }
    DELEGATE.with(|d| {
        d.get_or_init(|| {
            let this = Delegate::alloc(mtm).set_ivars(());
            // SAFETY: NSObject's plain initializer.
            unsafe { msg_send![super(this), init] }
        })
        .clone()
    })
}

// --- The menu-bar item.

fn status_item(mtm: MainThreadMarker) -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        button.setImage(Some(&template_icon()));
        button.setToolTip(Some(&NSString::from_str("SaveScummer")));
    }
    // Any click, left or right, opens it.
    item.setMenu(Some(&menu(mtm)));
    item
}

/// One image with both PNGs as representations, 22 points, marked as a
/// template: AppKit tints it for the menu bar and picks the Retina one.
fn template_icon() -> Retained<NSImage> {
    let size = NSSize::new(ICON_POINTS, ICON_POINTS);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    for png in [ICON_1X, ICON_2X] {
        if let Some(rep) = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(png)) {
            // The PNGs' DPI would make them tiny: their size is in points.
            rep.setSize(size);
            image.addRepresentation(&rep);
        }
    }
    image.setTemplate(true);
    image
}

fn menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let delegate = delegate(mtm);
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    rebuild_menu(&menu);
    menu
}

fn emit_game(action: TrayGameAction) {
    if let Some(game) = lock(&MENU_GAME).clone() {
        emit(Signal::TrayGame { game, action });
    }
}

fn rebuild_menu(menu: &NSMenu) {
    let mtm = MainThreadMarker::new().expect("menu on main thread");
    let delegate = delegate(mtm);
    let snapshot = lock(&MENU_SOURCE).as_ref().map(|source| source()).unwrap_or_default();
    *lock(&MENU_GAME) = snapshot.game;
    menu.removeAllItems();
    let add = |title: &str, action, enabled: bool, icon: Option<TrayIcon>| {
        // SAFETY: each selector is implemented by the process-lifetime delegate.
        let item = unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                &NSString::new(),
            );
            item.setTarget(Some(&delegate));
            item.setEnabled(enabled);
            if let Some(icon) = icon
                && let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(icon_png(icon)))
            {
                image.setSize(NSSize::new(16.0, 16.0));
                image.setTemplate(true);
                item.setImage(Some(&image));
            }
            item
        };
        menu.addItem(&item);
    };
    add(snapshot.name.as_deref().unwrap_or("No active game"), None, false, None);
    if snapshot.running {
        add("Stop", Some(sel!(stopGame:)), snapshot.stop, Some(TrayIcon::Stop));
    } else {
        add("Play", Some(sel!(play:)), snapshot.play, Some(TrayIcon::Play));
    }
    add("Save checkpoint", Some(sel!(save:)), snapshot.save, Some(TrayIcon::Save));
    add("Load latest checkpoint", Some(sel!(load:)), snapshot.load, Some(TrayIcon::Load));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add("Main window", Some(sel!(openMainWindow:)), true, Some(TrayIcon::Main));
    add("Add custom game…", Some(sel!(addGame:)), true, Some(TrayIcon::Add));
    add("Scan for games", Some(sel!(scan:)), true, Some(TrayIcon::Scan));
    add("Settings…", Some(sel!(settings:)), true, Some(TrayIcon::Settings));
    add("About SaveScummer", Some(sel!(about:)), true, Some(TrayIcon::About));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add("Exit", Some(sel!(exit:)), true, Some(TrayIcon::Exit));
}

// --- Hotkeys: Carbon `RegisterEventHotKey` through `global-hotkey`. It needs
// no Accessibility permission and works over fullscreen games.

fn register_hotkeys(shortcuts: Shortcuts) -> (Option<GlobalHotKeyManager>, Vec<String>) {
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

/// Each press is one action. Carbon hotkeys never auto-repeat while held
/// (apps wanting repeats time them themselves), so there's nothing to
/// filter, and no key-up to wait for: one macOS loses (the screen locking
/// while the key is down) can't leave a key dead.
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
    fn a_press_after_a_lost_release_still_counts() {
        let id = HotKey::new(Some(Modifiers::ALT), Code::F5).id();
        let press = GlobalHotKeyEvent { id, state: HotKeyState::Pressed };
        assert_eq!(hotkey_action(press), Some(HotkeyAction::Save));
        // The release never arrives (the screen locked while ⌥F5 was down).
        assert_eq!(hotkey_action(press), Some(HotkeyAction::Save), "the next press saves");
        assert_eq!(hotkey_action(GlobalHotKeyEvent { id, state: HotKeyState::Released }), None);
    }
}

// --- Notifications: `UNUserNotificationCenter`, which only works inside an
// app bundle.

fn notify_now(title: &str, text: &str) {
    if !objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_some_and(|id| !id.is_empty()) {
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    listen_to_notifications(&center);
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(text));
    let id = NSUUID::UUID().UUIDString();
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
    let request = Mutex::new(Some(request));
    // Asks once; later calls answer at once with the user's choice.
    let then = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if let Some(request) = lock(&request).take().filter(|_| granted.as_bool()) {
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, None);
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &then,
    );
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop.
    #[unsafe(super(NSObject))]
    #[name = "SaveScummerNotificationDelegate"]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        /// Show it even when the host counts as the active app.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
        }

        /// Clicking a notification opens the UI.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            _response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            emit(Signal::OpenMainWindow);
            completion.call(());
        }
    }
);

fn listen_to_notifications(center: &UNUserNotificationCenter) {
    static DELEGATE: Mutex<Option<Retained<NotificationDelegate>>> = Mutex::new(None);
    let mut slot = lock(&DELEGATE);
    if slot.is_some() {
        return;
    }
    let this = NotificationDelegate::alloc().set_ivars(());
    // SAFETY: NSObject's plain initializer.
    let delegate: Retained<NotificationDelegate> = unsafe { msg_send![super(this), init] };
    // The center holds it weakly: kept here for the process's lifetime.
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    *slot = Some(delegate);
}
