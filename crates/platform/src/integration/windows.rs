//! The Windows tray icon, its menu and the
//! global hotkeys, all on one dedicated UI thread.

use std::cell::Cell;
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW, HICON,
    IDI_APPLICATION, LR_DEFAULTCOLOR, LoadIconW, MENUITEMINFOW, MF_GRAYED, MF_SEPARATOR, MF_STRING, MIIM_BITMAP, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow,
    SetMenuDefaultItem, SetMenuItemInfoW, SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_HOTKEY, WM_LBUTTONDBLCLK, WM_NULL, WNDCLASSW, WS_OVERLAPPED,
};

use super::{
    HotkeyAction, Key, MenuSource, Shortcut, Shortcuts, Signal, TrayDialog, TrayGameAction, TrayIcon, icon_png,
};
use crate::win::{copy_wide, wide};

/// Tray callback message (see `uCallbackMessage`).
const WM_TRAY: u32 = WM_APP + 1;
const WM_REBIND: u32 = WM_APP + 3;
const TRAY_ID: u32 = 1;
const MENU_MAIN: usize = 1;
const MENU_EXIT: usize = 2;
const MENU_PLAY: usize = 3;
const MENU_SAVE: usize = 4;
const MENU_LOAD: usize = 5;
const MENU_ADD: usize = 6;
const MENU_SCAN: usize = 7;
const MENU_SETTINGS: usize = 8;
const MENU_ABOUT: usize = 9;
const MENU_STOP: usize = 10;
const TOOLTIP: &str = "SaveScummer";
const ICO: &[u8] = include_bytes!("../../../../assets/icon.ico");

const HOTKEYS: [(i32, HotkeyAction); 2] = [(1, HotkeyAction::Save), (2, HotkeyAction::Load)];

struct Rebind {
    shortcuts: Shortcuts,
    reply: mpsc::SyncSender<Result<(), String>>,
}

/// Lives on the UI thread for the window's lifetime; the window's
/// `GWLP_USERDATA` points at it. Only shared references are ever made:
/// the popup menu's modal loop re-enters the window procedure.
struct UiState {
    on_signal: Box<dyn Fn(Signal) + Send + 'static>,
    menu_source: MenuSource,
    /// The tray icon, reloaded when the display scale changes.
    icon: Cell<AppIcon>,
    taskbar_created: u32,
    shortcuts: Cell<Shortcuts>,
}

#[derive(Clone, Copy)]
struct AppIcon {
    handle: HICON,
    /// Pixel size it was loaded for.
    size: i32,
    /// Whether we must destroy it (the stock fallback is shared).
    owned: bool,
}

impl AppIcon {
    fn release(self) {
        if self.owned {
            // SAFETY: we created it and nothing uses it anymore.
            unsafe { DestroyIcon(self.handle) };
        }
    }
}

pub struct Integration {
    hwnd: isize,
    thread: Option<JoinHandle<()>>,
    hotkey_errors: Vec<String>,
}

pub fn start(
    on_signal: Box<dyn Fn(Signal) + Send + 'static>,
    menu_source: MenuSource,
    shortcuts: Shortcuts,
) -> Result<Integration, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("savescummer-integration".into())
        .spawn(move || ui_thread(on_signal, menu_source, shortcuts, tx))
        .map_err(|e| format!("could not start the integration thread: {e}"))?;
    match rx.recv() {
        Ok(Ok((hwnd, hotkey_errors))) => Ok(Integration { hwnd, thread: Some(thread), hotkey_errors }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            let _ = thread.join();
            Err("the integration thread ended during startup".into())
        }
    }
}

impl Integration {
    /// Native hotkeys need no permission: nothing to allow.
    pub fn shortcut_setup(&self, _wait: std::time::Duration) -> super::ShortcutSetup {
        super::ShortcutSetup::Ready
    }

    pub fn set_up_shortcuts(&self) -> std::sync::mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = tx.send(Err("the shortcuts need no setup on Windows".into()));
        rx
    }

    pub fn hotkey_errors(&self) -> Vec<String> {
        self.hotkey_errors.clone()
    }

    pub fn rebind(&self, shortcuts: Shortcuts) -> Result<(), String> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let request = Box::into_raw(Box::new(Rebind { shortcuts, reply }));
        // SAFETY: ownership passes to the window procedure only on success.
        if unsafe { PostMessageW(self.hwnd as HWND, WM_REBIND, 0, request as LPARAM) } == 0 {
            unsafe {
                drop(Box::from_raw(request));
            }
            return Err("the hotkey window is unavailable".into());
        }
        receiver.recv().map_err(|_| "the hotkey window closed".to_string())?
    }

    /// Removes the tray icon, unregisters hotkeys and ends the thread.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Integration {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        // SAFETY: posting to a window handle is safe even if it's gone.
        unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) };
        // Joining from the UI thread itself (dropped inside on_signal)
        // would deadlock; the thread ends on its own after WM_CLOSE.
        if thread.thread().id() != std::thread::current().id() {
            let _ = thread.join();
        }
    }
}

type Ready = Result<(isize, Vec<String>), String>;

fn ui_thread(
    on_signal: Box<dyn Fn(Signal) + Send + 'static>,
    menu_source: MenuSource,
    shortcuts: Shortcuts,
    ready: mpsc::SyncSender<Ready>,
) {
    let class = wide("SaveScummerIntegration");
    let taskbar = wide("TaskbarCreated");
    // SAFETY: plain Win32 calls with NUL-terminated strings that outlive
    // them. The state box is freed only after the message loop ends, when
    // the window (its only user) is destroyed.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        // Fails harmlessly when a previous start in this process already
        // registered the class.
        RegisterClassW(&wc);
        // A real (never shown) top-level window rather than a message-only
        // one: only top-level windows receive the TaskbarCreated broadcast.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            let code = GetLastError();
            let _ = ready.send(Err(format!("could not create the integration window (error {code})")));
            return;
        }

        let state = Box::into_raw(Box::new(UiState {
            on_signal,
            menu_source,
            icon: Cell::new(load_app_icon(small_icon_size(hwnd))),
            taskbar_created: RegisterWindowMessageW(taskbar.as_ptr()),
            shortcuts: Cell::new(shortcuts),
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);

        let mut errors = Vec::new();
        for (index, shortcut) in shortcuts.iter().enumerate() {
            let Some(shortcut) = shortcut else { continue };
            if RegisterHotKey(hwnd, HOTKEYS[index].0, modifiers(*shortcut), key_code(shortcut.key)) == 0 {
                let code = GetLastError();
                errors.push(format!(
                    "{} is unavailable: another app already uses it (error {code})",
                    shortcut.canonical()
                ));
            }
        }
        // If Explorer isn't running yet, TaskbarCreated adds it later.
        add_tray_icon(hwnd, &*state);

        let _ = ready.send(Ok((hwnd as isize, errors)));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Box::from_raw(state).icon.get().release();
    }
}

fn modifiers(shortcut: Shortcut) -> u32 {
    let mut flags = MOD_NOREPEAT;
    if shortcut.ctrl {
        flags |= MOD_CONTROL;
    }
    if shortcut.alt {
        flags |= MOD_ALT;
    }
    if shortcut.shift {
        flags |= MOD_SHIFT;
    }
    if shortcut.meta {
        flags |= MOD_WIN;
    }
    flags
}

fn key_code(key: Key) -> u32 {
    match key {
        Key::Function(number) => 0x70 + u32::from(number) - 1,
        Key::Letter(letter) | Key::Digit(letter) => letter as u32,
    }
}

fn register_pair(hwnd: HWND, shortcuts: Shortcuts) -> Result<(), String> {
    for (index, shortcut) in shortcuts.iter().enumerate() {
        let Some(shortcut) = shortcut else { continue };
        if unsafe { RegisterHotKey(hwnd, HOTKEYS[index].0, modifiers(*shortcut), key_code(shortcut.key)) } == 0 {
            let error = unsafe { GetLastError() };
            for (id, _) in HOTKEYS.iter().take(index) {
                unsafe { UnregisterHotKey(hwnd, *id) };
            }
            return Err(format!("{} is unavailable: another app may use it (error {error})", shortcut.canonical()));
        }
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the pointer is either null (before setup / after the state
    // is gone) or the UiState box, which outlives the window.
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const UiState).as_ref() };
    let Some(state) = state else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    match msg {
        WM_HOTKEY => {
            if let Some(&(_, action)) = HOTKEYS.iter().find(|(id, ..)| *id as usize == wparam) {
                emit(state, Signal::Hotkey(action));
            }
            0
        }
        WM_TRAY => {
            // With NOTIFYICON_VERSION_4 the event is in LOWORD(lParam).
            match (lparam as u32) & 0xFFFF {
                NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONDBLCLK => emit(state, Signal::OpenMainWindow),
                WM_CONTEXTMENU => show_menu(hwnd, state),
                _ => {}
            }
            0
        }
        WM_REBIND => {
            let request = unsafe { Box::from_raw(lparam as *mut Rebind) };
            let old = state.shortcuts.get();
            for (id, _) in HOTKEYS {
                unsafe { UnregisterHotKey(hwnd, id) };
            }
            let result = register_pair(hwnd, request.shortcuts);
            if result.is_ok() {
                state.shortcuts.set(request.shortcuts);
            } else {
                let _ = register_pair(hwnd, old);
            }
            let _ = request.reply.send(result);
            0
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            // The display scale changed: redraw the icon at the new size.
            if refresh_icon(hwnd, state) {
                let mut nid = tray_data(hwnd);
                nid.uFlags = NIF_ICON;
                nid.hIcon = state.icon.get().handle;
                // SAFETY: `nid` is fully initialized and sized.
                unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
            }
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            let nid = tray_data(hwnd);
            // SAFETY: plain Win32 calls on our own window.
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &nid);
                for (id, ..) in HOTKEYS {
                    UnregisterHotKey(hwnd, id);
                }
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                PostQuitMessage(0);
            }
            0
        }
        m if m == state.taskbar_created && m != 0 => {
            // Explorer restarted and forgot every tray icon.
            add_tray_icon(hwnd, state);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// NIN_SELECT | NINF_KEY: the icon was chosen with the keyboard.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// Calls the host's callback. A panic must not unwind into Windows.
fn emit(state: &UiState, signal: Signal) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.on_signal)(signal)));
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: TRAY_ID, ..Default::default() }
}

fn add_tray_icon(hwnd: HWND, state: &UiState) {
    // Explorer may have restarted because the scale changed.
    refresh_icon(hwnd, state);
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = state.icon.get().handle;
    copy_wide(&mut nid.szTip, TOOLTIP);
    nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    // SAFETY: `nid` is fully initialized and sized.
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &nid) == 0 {
            // Possibly still there (a duplicate TaskbarCreated): refresh.
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
        Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

fn show_menu(hwnd: HWND, state: &UiState) {
    let snapshot = (state.menu_source)();
    let game = snapshot.game.clone();
    // SAFETY: the menu is created, used and destroyed here; strings
    // outlive the calls.
    let chosen = unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let mut bitmaps: Vec<HBITMAP> = Vec::new();
        let mut append = |id: usize, label: &str, enabled: bool, icon: Option<TrayIcon>| {
            let label = wide(label);
            AppendMenuW(menu, MF_STRING | if enabled { 0 } else { MF_GRAYED }, id, label.as_ptr());
            if let Some(bitmap) = icon.and_then(|icon| menu_bitmap(icon, small_icon_size(hwnd))) {
                let info = MENUITEMINFOW {
                    cbSize: size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_BITMAP,
                    hbmpItem: bitmap,
                    ..Default::default()
                };
                SetMenuItemInfoW(menu, id as u32, 0, &info);
                bitmaps.push(bitmap);
            }
        };
        append(0, snapshot.name.as_deref().unwrap_or("No active game"), false, None);
        if snapshot.running {
            append(MENU_STOP, "Stop", snapshot.stop, Some(TrayIcon::Stop));
        } else {
            append(MENU_PLAY, "Play", snapshot.play, Some(TrayIcon::Play));
        }
        append(MENU_SAVE, "Save checkpoint", snapshot.save, Some(TrayIcon::Save));
        append(MENU_LOAD, "Load latest checkpoint", snapshot.load, Some(TrayIcon::Load));
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        append(MENU_MAIN, "Main window", true, Some(TrayIcon::Main));
        append(MENU_ADD, "Add custom game…", true, Some(TrayIcon::Add));
        append(MENU_SCAN, "Scan for games", true, Some(TrayIcon::Scan));
        append(MENU_SETTINGS, "Settings…", true, Some(TrayIcon::Settings));
        append(MENU_ABOUT, "About SaveScummer…", true, Some(TrayIcon::About));
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        append(MENU_EXIT, "Quit", true, None);
        SetMenuDefaultItem(menu, MENU_MAIN as u32, 0);
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Without the foreground switch and the WM_NULL afterwards, the
        // menu doesn't close when the user clicks elsewhere (documented
        // Shell quirk).
        SetForegroundWindow(hwnd);
        let chosen =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        for bitmap in bitmaps {
            DeleteObject(bitmap);
        }
        chosen as usize
    };
    match chosen {
        MENU_PLAY | MENU_STOP | MENU_SAVE | MENU_LOAD => {
            if let Some(game) = game {
                let action = match chosen {
                    MENU_PLAY => TrayGameAction::Play,
                    MENU_STOP => TrayGameAction::Stop,
                    MENU_SAVE => TrayGameAction::Save,
                    _ => TrayGameAction::Load,
                };
                emit(state, Signal::TrayGame { game, action });
            }
        }
        MENU_MAIN => emit(state, Signal::OpenMainWindow),
        MENU_ADD => emit(state, Signal::OpenDialog(TrayDialog::Add)),
        MENU_SCAN => emit(state, Signal::Scan),
        MENU_SETTINGS => emit(state, Signal::OpenDialog(TrayDialog::Settings)),
        MENU_ABOUT => emit(state, Signal::OpenDialog(TrayDialog::About)),
        MENU_EXIT => emit(state, Signal::Exit),
        _ => {}
    }
}

/// A top-down, premultiplied BGRA bitmap for a native menu item.
fn menu_bitmap(icon: TrayIcon, size: i32) -> Option<HBITMAP> {
    let png = image::load_from_memory(icon_png(icon)).ok()?.to_rgba8();
    let pixels = image::imageops::resize(&png, size as u32, size as u32, image::imageops::FilterType::Lanczos3);
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: size,
        biHeight: -size,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap =
        unsafe { CreateDIBSection(std::ptr::null_mut(), &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        return None;
    }
    let dest = unsafe { std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize) };
    for (source, dest) in pixels.pixels().zip(dest.as_chunks_mut::<4>().0) {
        let alpha = source[3] as u16;
        dest[0] = (source[2] as u16 * alpha / 255) as u8;
        dest[1] = (source[1] as u16 * alpha / 255) as u8;
        dest[2] = (source[0] as u16 * alpha / 255) as u8;
        dest[3] = source[3];
    }
    Some(bitmap)
}

/// The small-icon size at the window's DPI (the tray's monitor: the
/// window sits at the primary monitor's origin). Real pixels only because
/// the host's manifest makes it DPI-aware; otherwise always 16.
fn small_icon_size(hwnd: HWND) -> i32 {
    // SAFETY: plain Win32 queries on our own window.
    unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(hwnd)) }.max(16)
}

/// Reloads the icon if the small-icon size changed; returns whether it
/// did. The shell keeps its own copy, so the old one can go at once.
fn refresh_icon(hwnd: HWND, state: &UiState) -> bool {
    let size = small_icon_size(hwnd);
    if size == state.icon.get().size {
        return false;
    }
    state.icon.replace(load_app_icon(size)).release();
    true
}

/// The app icon from the embedded .ico at `size` pixels, else the stock
/// application icon.
fn load_app_icon(size: i32) -> AppIcon {
    if let Some(image) = ico_image(ICO, size as u32) {
        // SAFETY: `image` is one complete icon image (BMP or PNG) from the
        // .ico, as CreateIconFromResourceEx expects; 0x00030000 is the
        // required format version.
        let icon = unsafe {
            CreateIconFromResourceEx(image.as_ptr(), image.len() as u32, 1, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
        };
        if !icon.is_null() {
            return AppIcon { handle: icon, size, owned: true };
        }
    }
    // SAFETY: loads a shared system icon; it must not be destroyed.
    let handle = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
    AppIcon { handle, size, owned: false }
}

/// Picks the image in an .ico file that best fits `size` pixels: the
/// smallest one at least that big, else the largest.
pub(super) fn ico_image(ico: &[u8], size: u32) -> Option<&[u8]> {
    let u16_at = |i: usize| Some(u16::from_le_bytes(ico.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_le_bytes(ico.get(i..i + 4)?.try_into().ok()?));
    if u16_at(2)? != 1 {
        return None; // Not an icon (1) file.
    }
    let count = u16_at(4)? as usize;
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let at = 6 + 16 * i;
        let width = match *ico.get(at)? {
            0 => 256,
            w => w as u32,
        };
        let len = u32_at(at + 8)? as usize;
        let offset = u32_at(at + 12)? as usize;
        let image = ico.get(offset..offset.checked_add(len)?)?;
        entries.push((width, image));
    }
    let fitting = entries.iter().filter(|(w, _)| *w >= size).min_by_key(|(w, _)| *w);
    fitting.or_else(|| entries.iter().max_by_key(|(w, _)| *w)).map(|(_, image)| *image)
}

/// The tray and hotkeys have their own thread here, so the main thread only
/// waits.
pub fn run_main_loop(wait: impl FnOnce() + Send + 'static) {
    wait()
}
