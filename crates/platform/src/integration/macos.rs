//! The macOS menu-bar item, global hotkeys (⌥F5, ⌥F9), notifications,
//! reopen and quit, all on the main thread's `NSApplication` run loop.
//!
//! The host's main thread runs that loop for the whole run
//! ([`run_main_loop`]), also without integrations: `NSWorkspace` only tracks
//! the frontmost app while it runs (see the monitor's macOS source).

use std::collections::HashSet;
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
    NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSError, NSObjectProtocol, NSPoint, NSSize, NSString, NSUUID};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::{HotkeyAction, Signal};

/// ⌥F5 and ⌥F9: macOS reserves ⌃F5 (PLAN-MACOS.md, HOTKEYS).
const HOTKEYS: [(Code, HotkeyAction, &str); 2] =
    [(Code::F5, HotkeyAction::Save, "⌥F5 (Save)"), (Code::F9, HotkeyAction::Load, "⌥F9 (Load)")];

/// The menu-bar template, 22 × 22 points at 1x and 2x.
const ICON_1X: &[u8] = include_bytes!("../../../../assets/macos/SaveScummerTemplate.png");
const ICON_2X: &[u8] = include_bytes!("../../../../assets/macos/SaveScummerTemplate@2x.png");
const ICON_POINTS: f64 = 22.0;

type Handler = Arc<Mutex<Box<dyn Fn(Signal) + Send + 'static>>>;

/// Who hears signals; None when integrations are off.
static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);
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
pub fn start(on_signal: Box<dyn Fn(Signal) + Send + 'static>) -> Result<Integration, String> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err("the menu bar and hotkeys must start on the main thread".into());
    };
    application(mtm);
    *lock(&HANDLER) = Some(Arc::new(Mutex::new(on_signal)));
    let bar = status_item(mtm);
    let (hotkeys, hotkey_errors) = register_hotkeys();
    MAIN.with(|main| *main.borrow_mut() = Some(MainState { bar, hotkeys }));
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

    /// Removes the menu-bar item and unregisters the hotkeys.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Integration {
    fn drop(&mut self) {
        *lock(&HANDLER) = None;
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
        /// A click shows the UI; a right-click or ⌃-click opens the menu.
        #[unsafe(method(statusItemClicked:))]
        fn status_item_clicked(&self, _sender: Option<&AnyObject>) {
            let mtm = self.mtm();
            let event = NSApplication::sharedApplication(mtm).currentEvent();
            let wants_menu = event.is_some_and(|e| {
                e.r#type() == NSEventType::RightMouseUp || e.modifierFlags().contains(NSEventModifierFlags::Control)
            });
            if wants_menu {
                open_menu(self, mtm);
            } else {
                emit(Signal::OpenMainWindow);
            }
        }

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
        // SAFETY: the delegate lives for the process and has this action.
        unsafe {
            button.setTarget(Some(&delegate(mtm)));
            button.setAction(Some(sel!(statusItemClicked:)));
        }
        button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
    }
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

/// Opens the menu under the menu-bar item; returns once it closes. Nothing
/// stays borrowed meanwhile: the menu runs its own event loop, in which the
/// host may shut down.
fn open_menu(delegate: &Delegate, mtm: MainThreadMarker) {
    let bar = MAIN.with(|main| main.borrow().as_ref().map(|state| state.bar.clone()));
    if let Some(bar) = bar {
        show_menu(&bar, delegate, mtm);
    }
}

fn show_menu(bar: &NSStatusItem, delegate: &Delegate, mtm: MainThreadMarker) {
    let menu = NSMenu::new(mtm);
    for (title, action) in [("Main window", sel!(openMainWindow:)), ("Exit", sel!(exit:))] {
        // SAFETY: the delegate lives for the process and has both actions.
        let item = unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::new(),
            );
            item.setTarget(Some(delegate));
            item
        };
        menu.addItem(&item);
    }
    // Attached only while open, so a plain click stays a click.
    bar.setMenu(Some(&menu));
    if let Some(button) = bar.button(mtm) {
        // SAFETY: a click on our own button; it opens the menu and returns
        // once the menu closes.
        unsafe { button.performClick(None) };
    }
    bar.setMenu(None);
}

// --- Hotkeys: Carbon `RegisterEventHotKey` through `global-hotkey`. It needs
// no Accessibility permission and works over fullscreen games.

fn register_hotkeys() -> (Option<GlobalHotKeyManager>, Vec<String>) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(e) => return (None, vec![format!("hotkeys are unavailable: {e}")]),
    };
    let mut errors = Vec::new();
    for (code, _, name) in HOTKEYS {
        if let Err(e) = manager.register(HotKey::new(Some(Modifiers::ALT), code)) {
            errors.push(format!("{name} is unavailable: another app already uses it ({e})"));
        }
    }
    listen_to_hotkeys();
    (Some(manager), errors)
}

/// `global-hotkey` takes one handler per process: it forwards to whoever
/// listens now. Holding a key sends one signal, not one per repeat.
fn listen_to_hotkeys() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let held: Mutex<HashSet<u32>> = Mutex::new(HashSet::new());
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let Some(&(_, action, _)) =
                HOTKEYS.iter().find(|(code, ..)| HotKey::new(Some(Modifiers::ALT), *code).id() == event.id)
            else {
                return;
            };
            match event.state {
                HotKeyState::Pressed if lock(&held).insert(event.id) => emit(Signal::Hotkey(action)),
                HotKeyState::Pressed => {}
                HotKeyState::Released => {
                    lock(&held).remove(&event.id);
                }
            }
        }));
    });
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
