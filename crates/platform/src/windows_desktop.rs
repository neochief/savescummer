use crate::desktop::{DesktopCommand, DesktopEvent};
use std::{
    cell::RefCell,
    io,
    sync::mpsc,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

thread_local! { static EVENTS: RefCell<Option<mpsc::Sender<DesktopEvent>>> = const { RefCell::new(None) }; }
const TRAY: u32 = WM_APP + 1;
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn emit(event: DesktopEvent) {
    EVENTS.with(|tx| {
        if let Some(tx) = tx.borrow().as_ref() {
            let _ = tx.send(event);
        }
    });
}
// This window property/message pair is shared with MainWindow's native handler.
// The foreground desktop owns selection and admission feedback. Never fall back
// to a different running game when its UI is disabled or has a dialog open.
fn forward_to_desktop(foreground: HWND, action: WPARAM) -> bool {
    unsafe {
        let target = GetAncestor(foreground, GA_ROOTOWNER);
        if target.is_null()
            || GetPropW(target, wide("SaveScummer.ShortcutTarget.v1").as_ptr()) as usize != 1
        {
            return false;
        }
        let message = RegisterWindowMessageW(wide("SaveScummer.DesktopShortcut.v1").as_ptr());
        if message != 0 {
            PostMessageW(target, message, action, 0);
        }
        true
    }
}
unsafe extern "system" fn window(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            if matches!(wp, 1 | 2) && forward_to_desktop(unsafe { GetForegroundWindow() }, wp) {
                return 0;
            }
            match wp {
                1 => emit(DesktopEvent::Save),
                2 => emit(DesktopEvent::Load),
                _ => (),
            };
            0
        }
        TRAY => {
            match lp as u32 {
                WM_LBUTTONUP | NIN_BALLOONUSERCLICK => emit(DesktopEvent::Open),
                WM_RBUTTONUP => unsafe {
                    let menu = CreatePopupMenu();
                    if !menu.is_null() {
                        AppendMenuW(menu, MF_STRING, 1, wide("Main window").as_ptr());
                        AppendMenuW(menu, MF_STRING, 2, wide("Exit").as_ptr());
                        let mut point = POINT::default();
                        GetCursorPos(&mut point);
                        SetForegroundWindow(hwnd);
                        let selected = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON,
                            point.x,
                            point.y,
                            0,
                            hwnd,
                            std::ptr::null(),
                        );
                        DestroyMenu(menu);
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                        match selected {
                            1 => emit(DesktopEvent::Open),
                            2 => emit(DesktopEvent::Exit),
                            _ => (),
                        }
                    }
                },
                _ => (),
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
fn text<const N: usize>(dest: &mut [u16; N], value: &str) {
    for (out, ch) in dest.iter_mut().take(N - 1).zip(value.encode_utf16()) {
        *out = ch;
    }
}
pub fn run(
    events: mpsc::Sender<DesktopEvent>,
    commands: mpsc::Receiver<DesktopCommand>,
    ready: mpsc::SyncSender<io::Result<Vec<String>>>,
) {
    EVENTS.with(|tx| *tx.borrow_mut() = Some(events));
    unsafe {
        let class_name = wide("SaveScummerHostIntegration");
        let instance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSW {
            lpfnWndProc: Some(window),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };
        let atom = RegisterClassW(&class);
        if atom == 0 {
            let _ = ready.send(Err(io::Error::last_os_error()));
            return;
        }
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
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
            let _ = ready.send(Err(io::Error::last_os_error()));
            UnregisterClassW(class_name.as_ptr(), instance);
            return;
        }
        let mut icon: NOTIFYICONDATAW = std::mem::zeroed();
        icon.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        icon.hWnd = hwnd;
        icon.uID = 1;
        icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        icon.uCallbackMessage = TRAY;
        icon.hIcon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        text(&mut icon.szTip, "Save Scummer");
        let save = RegisterHotKey(hwnd, 1, MOD_CONTROL | MOD_NOREPEAT, VK_F5 as u32) != 0;
        let load = RegisterHotKey(hwnd, 2, MOD_CONTROL | MOD_NOREPEAT, VK_F9 as u32) != 0;
        let tray = Shell_NotifyIconW(NIM_ADD, &icon) != 0;
        {
            let mut warnings = vec![];
            if !save {
                warnings.push(
                    "Ctrl+F5 is unavailable; another application may own the shortcut".into(),
                );
            }
            if !load {
                warnings.push(
                    "Ctrl+F9 is unavailable; another application may own the shortcut".into(),
                );
            }
            if !tray {
                warnings.push("The notification-area icon could not be added".into());
            }
            let _ = ready.send(Ok(warnings));
            let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
            let mut last_notification = Instant::now() - Duration::from_secs(10);
            'messages: loop {
                let mut message: MSG = std::mem::zeroed();
                while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if message.message == taskbar_created {
                        Shell_NotifyIconW(NIM_ADD, &icon);
                    }
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                match commands.recv_timeout(Duration::from_millis(20)) {
                    Ok(DesktopCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break 'messages;
                    }
                    Ok(DesktopCommand::Failure(value)) => {
                        if last_notification.elapsed() >= Duration::from_secs(2) {
                            let mut notice = icon;
                            notice.uFlags = NIF_INFO;
                            notice.dwInfoFlags = NIIF_ERROR | NIIF_NOSOUND;
                            text(&mut notice.szInfoTitle, "Save Scummer");
                            text(&mut notice.szInfo, &value);
                            Shell_NotifyIconW(NIM_MODIFY, &notice);
                            last_notification = Instant::now();
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
            }
        }
        if save {
            UnregisterHotKey(hwnd, 1);
        }
        if load {
            UnregisterHotKey(hwnd, 2);
        }
        if tray {
            Shell_NotifyIconW(NIM_DELETE, &icon);
        }
        DestroyWindow(hwnd);
        UnregisterClassW(class_name.as_ptr(), instance);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWindow(HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe { DestroyWindow(self.0) };
        }
    }
    fn test_window(owner: HWND) -> TestWindow {
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("Shortcut routing test").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                owner,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        TestWindow(hwnd)
    }

    #[test]
    fn shortcuts_forward_only_to_marked_desktop_and_its_owned_windows() {
        let desktop = test_window(std::ptr::null_mut());
        let unrelated = test_window(std::ptr::null_mut());
        let dialog = test_window(desktop.0);
        assert!(!forward_to_desktop(desktop.0, 1));
        assert!(!forward_to_desktop(std::ptr::null_mut(), 1));
        unsafe {
            assert_ne!(
                SetPropW(
                    desktop.0,
                    wide("SaveScummer.ShortcutTarget.v1").as_ptr(),
                    1_usize as HANDLE
                ),
                0
            );
            let message = RegisterWindowMessageW(wide("SaveScummer.DesktopShortcut.v1").as_ptr());
            assert_ne!(message, 0);
            assert!(!forward_to_desktop(unrelated.0, 1));
            for (source, action) in [(desktop.0, 1), (dialog.0, 2)] {
                assert!(forward_to_desktop(source, action));
                let mut received: MSG = std::mem::zeroed();
                assert_ne!(
                    PeekMessageW(&mut received, desktop.0, message, message, PM_REMOVE),
                    0
                );
                assert_eq!(received.hwnd, desktop.0);
                assert_eq!(received.wParam, action);
                assert_eq!(
                    PeekMessageW(&mut received, desktop.0, message, message, PM_REMOVE),
                    0
                );
            }
        }
    }
}
