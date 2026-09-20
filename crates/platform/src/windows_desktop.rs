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
unsafe extern "system" fn window(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
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
