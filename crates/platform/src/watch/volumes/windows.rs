//! Letting go of a drive (Windows). A folder watch keeps its folder open, and
//! an open handle stops the user from safely removing a USB drive that holds
//! a Steam library. So every volume with watches gets a device notification;
//! when Windows asks to remove the volume, its watches are dropped before
//! the answer, and when the volume returns they are opened again.
//!
//! A hidden top-level window on its own thread receives the notifications:
//! removal requests arrive for the handle registered per volume, arrivals
//! as a broadcast to top-level windows.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetVolumePathNameW, OPEN_EXISTING,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DBT_DEVICEARRIVAL, DBT_DEVICEQUERYREMOVE, DBT_DEVICEQUERYREMOVEFAILED, DBT_DEVICEREMOVECOMPLETE,
    DBT_DEVICEREMOVEPENDING, DBT_DEVTYP_HANDLE, DBT_DEVTYP_VOLUME, DEV_BROADCAST_HANDLE, DEV_BROADCAST_HDR,
    DEV_BROADCAST_VOLUME, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetMessageW, GetWindowLongPtrW, HDEVNOTIFY, MSG, PostMessageW, RegisterClassW, RegisterDeviceNotificationW,
    SendMessageW, SetWindowLongPtrW, UnregisterDeviceNotification, WM_APP, WM_CLOSE, WM_DESTROY, WM_DEVICECHANGE,
    WNDCLASSW, WS_OVERLAPPED,
};

use super::Msg;
use crate::win::wide;

/// Replaces the set of volumes that have watches; `lparam` is a leaked
/// `Box<Vec<String>>`.
const WM_SET_VOLUMES: u32 = WM_APP + 1;
/// Test only: acts as if Windows sent the event in `wparam` for the volume
/// in `lparam` (a `*const String`).
const WM_SIMULATE: u32 = WM_APP + 2;

/// How long a removal request waits for the watches to close. Windows asks
/// with a timeout of its own; answering late only means "busy".
const RELEASE_TIMEOUT: Duration = Duration::from_secs(5);

/// The volume a path lives on, as Windows names its mount point (`D:\`, or
/// a folder for a volume mounted in one), upper-cased for comparing.
pub fn volume_of(path: &Path) -> Option<String> {
    let wide_path = wide(path);
    let mut buf = [0u16; 1024];
    // SAFETY: the input is NUL-terminated and the buffer length is passed.
    if unsafe { GetVolumePathNameW(wide_path.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).to_uppercase())
}

/// The window that receives device notifications; closed on drop.
pub struct Volumes {
    remote: Remote,
}

/// Talks to the window from other threads.
#[derive(Clone, Copy)]
pub struct Remote {
    hwnd: isize,
}

impl Volumes {
    pub fn remote(&self) -> Remote {
        self.remote
    }

    /// `watch` receives [`Msg::Release`] and [`Msg::Restore`].
    pub fn start(watch: Sender<Msg>) -> Option<Volumes> {
        let (ready, hwnd) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("savescummer-watch-drives".into())
            .spawn(move || window_thread(watch, ready))
            .ok()?;
        let hwnd = hwnd.recv().ok().flatten()?;
        Some(Volumes { remote: Remote { hwnd } })
    }
}

impl Remote {
    /// Registers the volumes that have watches now, and forgets the others.
    pub fn set_volumes(&self, volumes: Vec<String>) {
        let boxed = Box::into_raw(Box::new(volumes));
        // SAFETY: the window thread takes ownership of the box; if the post
        // fails, it never sees it and we free it here.
        unsafe {
            if PostMessageW(self.hwnd as HWND, WM_SET_VOLUMES, 0, boxed as LPARAM) == 0 {
                drop(Box::from_raw(boxed));
            }
        }
    }

    /// Acts as if Windows sent `event` (a `DBT_*` code) for `volume`.
    pub fn simulate(&self, event: u32, volume: &str) {
        let volume = volume.to_uppercase();
        // SAFETY: SendMessage returns after the window handled it, so the
        // string outlives its use.
        unsafe { SendMessageW(self.hwnd as HWND, WM_SIMULATE, event as WPARAM, &volume as *const String as LPARAM) };
    }
}

impl Drop for Volumes {
    fn drop(&mut self) {
        // SAFETY: the window thread ends its loop when the window is gone.
        unsafe { PostMessageW(self.remote.hwnd as HWND, WM_CLOSE, 0, 0) };
    }
}

/// One volume's device notification and the handle it was registered for.
struct Registration {
    handle: HANDLE,
    notify: HDEVNOTIFY,
}

impl Registration {
    fn open(hwnd: HWND, volume: &str) -> Option<Registration> {
        let path = wide(volume);
        // SAFETY: a plain open of the volume's root folder; closed in drop.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        // SAFETY: a zeroed header with its size and type set, as the API wants.
        let notify = unsafe {
            let mut filter: DEV_BROADCAST_HANDLE = std::mem::zeroed();
            filter.dbch_size = size_of::<DEV_BROADCAST_HANDLE>() as u32;
            filter.dbch_devicetype = DBT_DEVTYP_HANDLE;
            filter.dbch_handle = handle;
            RegisterDeviceNotificationW(hwnd, &filter as *const _ as *const _, DEVICE_NOTIFY_WINDOW_HANDLE)
        };
        if notify.is_null() {
            // SAFETY: opened above.
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Registration { handle, notify })
    }

    /// Closes the handle but keeps the notification: Windows still reports
    /// on it whether the removal went ahead or failed.
    fn close_handle(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: opened in `open`, closed once.
            unsafe { CloseHandle(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.close_handle();
        // SAFETY: registered in `open`, unregistered once.
        unsafe { UnregisterDeviceNotification(self.notify) };
    }
}

struct State {
    watch: Sender<Msg>,
    registered: HashMap<String, Registration>,
    /// Volumes Windows asked to remove, waiting to return.
    released: HashSet<String>,
    wanted: HashSet<String>,
}

impl State {
    fn release(&mut self, volume: &str, wait: bool) {
        // Our own handle is one of those holding the volume.
        if let Some(registration) = self.registered.get_mut(volume) {
            registration.close_handle();
        }
        self.released.insert(volume.to_string());
        let (ack, acked) = mpsc::channel();
        if self.watch.send(Msg::Release(volume.to_string(), ack)).is_ok() && wait {
            let _ = acked.recv_timeout(RELEASE_TIMEOUT);
        }
    }

    fn restore(&mut self, hwnd: HWND, volume: &str) {
        if !self.released.remove(volume) {
            return;
        }
        self.registered.remove(volume);
        if self.wanted.contains(volume)
            && let Some(registration) = Registration::open(hwnd, volume)
        {
            self.registered.insert(volume.to_string(), registration);
        }
        let _ = self.watch.send(Msg::Restore(volume.to_string()));
    }

    fn volume_for(&self, notify: HDEVNOTIFY) -> Option<String> {
        self.registered.iter().find(|(_, r)| r.notify == notify).map(|(v, _)| v.clone())
    }

    fn device_change(&mut self, hwnd: HWND, event: u32, header: *const DEV_BROADCAST_HDR) {
        if header.is_null() {
            return;
        }
        // SAFETY: Windows (or `simulate`) passes a valid header whose type
        // says which structure it starts.
        unsafe {
            match ((*header).dbch_devicetype, event) {
                (DBT_DEVTYP_HANDLE, DBT_DEVICEQUERYREMOVE) => {
                    let notify = (*(header as *const DEV_BROADCAST_HANDLE)).dbch_hdevnotify;
                    if let Some(volume) = self.volume_for(notify) {
                        self.release(&volume, true);
                    }
                }
                (DBT_DEVTYP_HANDLE, DBT_DEVICEREMOVEPENDING | DBT_DEVICEREMOVECOMPLETE) => {
                    // Gone, asked or not (a drive pulled without asking gets
                    // no query first). Its arrival brings the watches back.
                    let notify = (*(header as *const DEV_BROADCAST_HANDLE)).dbch_hdevnotify;
                    if let Some(volume) = self.volume_for(notify) {
                        if !self.released.contains(&volume) {
                            self.release(&volume, false);
                        }
                        self.registered.remove(&volume);
                    }
                }
                (DBT_DEVTYP_HANDLE, DBT_DEVICEQUERYREMOVEFAILED) => {
                    // Something else refused; the drive stays, and so do we.
                    let notify = (*(header as *const DEV_BROADCAST_HANDLE)).dbch_hdevnotify;
                    if let Some(volume) = self.volume_for(notify) {
                        self.restore(hwnd, &volume);
                    }
                }
                (DBT_DEVTYP_VOLUME, DBT_DEVICEARRIVAL) => {
                    let mask = (*(header as *const DEV_BROADCAST_VOLUME)).dbcv_unitmask;
                    for bit in 0..26u32 {
                        if mask & (1 << bit) != 0 {
                            let volume = format!("{}:\\", (b'A' + bit as u8) as char);
                            self.restore(hwnd, &volume);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn set_volumes(&mut self, hwnd: HWND, volumes: Vec<String>) {
        self.wanted = volumes.into_iter().collect();
        self.registered.retain(|v, _| self.wanted.contains(v));
        self.released.retain(|v| self.wanted.contains(v));
        for volume in &self.wanted {
            if !self.registered.contains_key(volume)
                && !self.released.contains(volume)
                && let Some(registration) = Registration::open(hwnd, volume)
            {
                self.registered.insert(volume.clone(), registration);
            }
        }
    }

    /// What `simulate` sends: the structure Windows would pass for `volume`.
    fn simulate(&mut self, hwnd: HWND, event: u32, volume: &str) {
        // SAFETY: zeroed structures with their size and type set.
        unsafe {
            if event == DBT_DEVICEARRIVAL {
                let mut vol: DEV_BROADCAST_VOLUME = std::mem::zeroed();
                vol.dbcv_size = size_of::<DEV_BROADCAST_VOLUME>() as u32;
                vol.dbcv_devicetype = DBT_DEVTYP_VOLUME;
                let letter = volume.bytes().next().unwrap_or(b'A').to_ascii_uppercase();
                vol.dbcv_unitmask = 1 << (letter.saturating_sub(b'A') as u32).min(25);
                self.device_change(hwnd, event, &vol as *const _ as *const DEV_BROADCAST_HDR);
            } else {
                let mut handle: DEV_BROADCAST_HANDLE = std::mem::zeroed();
                handle.dbch_size = size_of::<DEV_BROADCAST_HANDLE>() as u32;
                handle.dbch_devicetype = DBT_DEVTYP_HANDLE;
                if let Some(registration) = self.registered.get(volume) {
                    handle.dbch_handle = registration.handle;
                    handle.dbch_hdevnotify = registration.notify;
                }
                self.device_change(hwnd, event, &handle as *const _ as *const DEV_BROADCAST_HDR);
            }
        }
    }
}

fn window_thread(watch: Sender<Msg>, ready: mpsc::SyncSender<Option<isize>>) {
    let class = wide("SaveScummerDrives");
    // SAFETY: plain Win32 calls with NUL-terminated strings that outlive
    // them. The state box is freed after the loop ends, when the window (its
    // only user) is gone.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        // Fails harmlessly when this process already registered it.
        RegisterClassW(&wc);
        // Top-level, never shown: only top-level windows get the volume
        // arrival broadcast.
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
            let _ = GetLastError();
            let _ = ready.send(None);
            return;
        }
        let state = Box::into_raw(Box::new(State {
            watch,
            registered: HashMap::new(),
            released: HashSet::new(),
            wanted: HashSet::new(),
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        let _ = ready.send(Some(hwnd as isize));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
        drop(Box::from_raw(state));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds the state box while the window lives; it
    // is cleared in WM_DESTROY before the box is freed.
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State).as_mut() };
    let Some(state) = state else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    match msg {
        WM_DEVICECHANGE => {
            state.device_change(hwnd, wparam as u32, lparam as *const DEV_BROADCAST_HDR);
            // TRUE: we never refuse a removal.
            1
        }
        WM_SET_VOLUMES => {
            // SAFETY: `set_volumes` leaked this box for us.
            let volumes = unsafe { Box::from_raw(lparam as *mut Vec<String>) };
            state.set_volumes(hwnd, *volumes);
            0
        }
        WM_SIMULATE => {
            // SAFETY: `simulate` passes a string that outlives SendMessage.
            let volume = unsafe { &*(lparam as *const String) };
            state.simulate(hwnd, wparam as u32, volume);
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            state.registered.clear();
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
