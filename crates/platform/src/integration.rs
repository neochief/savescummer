//! Global hotkeys and the tray icon, owned by one dedicated UI thread so they
//! work with no window open.
//!
//! - Ctrl+F5 → Save, Ctrl+F9 → Load. Holding a key triggers once.
//! - Tray: left-click or double-click opens the main window; right-click shows
//!   a menu with "Main window" and "Exit".
//! - [`Integration::notify`] shows an OS notification (a tray balloon).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Save,
    Load,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Hotkey(HotkeyAction),
    OpenMainWindow,
    Exit,
}

pub use imp::{Integration, start};

#[cfg(windows)]
mod imp {
    use std::cell::Cell;
    use std::sync::mpsc;
    use std::thread::JoinHandle;

    use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_F5, VK_F9,
    };
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
        DestroyMenu, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW,
        HICON, IDI_APPLICATION, LR_DEFAULTCOLOR, LoadIconW, MF_STRING, MSG, PostMessageW, PostQuitMessage,
        RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, SetMenuDefaultItem,
        SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP,
        WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_NULL,
        WNDCLASSW, WS_OVERLAPPED,
    };

    use super::{HotkeyAction, Signal};
    use crate::win::{copy_wide, wide};

    /// Tray callback message (see `uCallbackMessage`).
    const WM_TRAY: u32 = WM_APP + 1;
    /// Posted by `notify`; `lParam` owns a `Box<Balloon>`.
    const WM_BALLOON: u32 = WM_APP + 2;
    const TRAY_ID: u32 = 1;
    const MENU_MAIN: usize = 1;
    const MENU_EXIT: usize = 2;
    const TOOLTIP: &str = "SaveScummer";
    const ICO: &[u8] = include_bytes!("../../../assets/icon.ico");

    const HOTKEYS: [(i32, HotkeyAction, u16, &str); 2] =
        [(1, HotkeyAction::Save, VK_F5, "Ctrl+F5 (Save)"), (2, HotkeyAction::Load, VK_F9, "Ctrl+F9 (Load)")];

    struct Balloon {
        title: String,
        text: String,
    }

    /// Lives on the UI thread for the window's lifetime; the window's
    /// `GWLP_USERDATA` points at it. Only shared references are ever made:
    /// the popup menu's modal loop re-enters the window procedure.
    struct UiState {
        on_signal: Box<dyn Fn(Signal) + Send + 'static>,
        /// The tray icon, reloaded when the display scale changes.
        icon: Cell<AppIcon>,
        taskbar_created: u32,
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

    pub fn start(on_signal: Box<dyn Fn(Signal) + Send + 'static>) -> Result<Integration, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("savescummer-integration".into())
            .spawn(move || ui_thread(on_signal, tx))
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
        /// Shows an OS notification (a tray balloon). Never blocks.
        pub fn notify(&self, title: &str, text: &str) {
            let balloon = Box::into_raw(Box::new(Balloon { title: title.into(), text: text.into() }));
            // SAFETY: on success the UI thread takes ownership of the box back;
            // on failure we still own it and free it here.
            unsafe {
                if PostMessageW(self.hwnd as HWND, WM_BALLOON, 0, balloon as LPARAM) == 0 {
                    drop(Box::from_raw(balloon));
                }
            }
        }

        pub fn hotkey_errors(&self) -> Vec<String> {
            self.hotkey_errors.clone()
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

    fn ui_thread(on_signal: Box<dyn Fn(Signal) + Send + 'static>, ready: mpsc::SyncSender<Ready>) {
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
                icon: Cell::new(load_app_icon(small_icon_size(hwnd))),
                taskbar_created: RegisterWindowMessageW(taskbar.as_ptr()),
            }));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);

            let mut errors = Vec::new();
            for (id, _, vk, name) in HOTKEYS {
                if RegisterHotKey(hwnd, id, MOD_CONTROL | MOD_NOREPEAT, vk as u32) == 0 {
                    let code = GetLastError();
                    errors.push(format!("{name} is unavailable: another app already uses it (error {code})"));
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

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: the pointer is either null (before setup / after the state
        // is gone) or the UiState box, which outlives the window.
        let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const UiState).as_ref() };
        let Some(state) = state else {
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        };
        match msg {
            WM_HOTKEY => {
                if let Some(&(_, action, _, _)) = HOTKEYS.iter().find(|(id, ..)| *id as usize == wparam) {
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
            WM_BALLOON => {
                // SAFETY: `notify` posted a Box<Balloon> and gave up ownership.
                let balloon = unsafe { Box::from_raw(lparam as *mut Balloon) };
                show_balloon(hwnd, &balloon);
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

    fn show_balloon(hwnd: HWND, balloon: &Balloon) {
        let mut nid = tray_data(hwnd);
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut nid.szInfoTitle, &balloon.title);
        copy_wide(&mut nid.szInfo, &balloon.text);
        // SAFETY: `nid` is fully initialized and sized.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    }

    fn show_menu(hwnd: HWND, state: &UiState) {
        let main = wide("Main window");
        let exit = wide("Exit");
        // SAFETY: the menu is created, used and destroyed here; strings
        // outlive the calls.
        let chosen = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            AppendMenuW(menu, MF_STRING, MENU_MAIN, main.as_ptr());
            AppendMenuW(menu, MF_STRING, MENU_EXIT, exit.as_ptr());
            SetMenuDefaultItem(menu, MENU_MAIN as u32, 0);
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            // Without the foreground switch and the WM_NULL afterwards, the
            // menu doesn't close when the user clicks elsewhere (documented
            // Shell quirk).
            SetForegroundWindow(hwnd);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                0,
                hwnd,
                std::ptr::null(),
            );
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            chosen as usize
        };
        match chosen {
            MENU_MAIN => emit(state, Signal::OpenMainWindow),
            MENU_EXIT => emit(state, Signal::Exit),
            _ => {}
        }
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
                CreateIconFromResourceEx(
                    image.as_ptr(),
                    image.len() as u32,
                    1,
                    0x0003_0000,
                    size,
                    size,
                    LR_DEFAULTCOLOR,
                )
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
}

#[cfg(not(windows))]
mod imp {
    use super::Signal;

    pub struct Integration {
        _private: (),
    }

    pub fn start(on_signal: Box<dyn Fn(Signal) + Send + 'static>) -> Result<Integration, String> {
        let _ = on_signal;
        Err("not supported on this platform yet".into())
    }

    impl Integration {
        pub fn notify(&self, _title: &str, _text: &str) {}

        pub fn hotkey_errors(&self) -> Vec<String> {
            Vec::new()
        }

        pub fn stop(self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn the_embedded_icon_has_a_small_image() {
        let ico = include_bytes!("../../../assets/icon.ico");
        let image = imp::ico_image(ico, 16).expect("an image");
        assert!(!image.is_empty());
        assert!(imp::ico_image(b"not an icon", 16).is_none());
    }

    /// Needs a desktop session: adds a real tray icon and registers the
    /// global hotkeys, then removes both.
    #[test]
    #[ignore]
    fn starts_notifies_and_stops() {
        let integration = start(Box::new(|_| {})).expect("started");
        eprintln!("hotkey errors: {:?}", integration.hotkey_errors());
        integration.notify("SaveScummer test", "Integration test notification");
        std::thread::sleep(std::time::Duration::from_millis(500));
        integration.stop();

        // Dropping without stop() must clean up too, and a second start in
        // the same process must work (class already registered).
        let again = start(Box::new(|_| {})).expect("started again");
        drop(again);
    }
}
