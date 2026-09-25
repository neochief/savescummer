//! Registry change notifications (Windows): the uninstall keys and the GOG
//! games key, where GOG and standalone installers record their installs.
//!
//! One thread owns every key handle, because Windows cancels a key's
//! notification when the thread that asked for it exits. Keys are watched
//! with their subkeys; a key that doesn't exist is skipped until the next
//! [`KeyWatcher::set_keys`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};

use super::{Hive, Msg, RegistryKey};

/// An event handle shared with the worker thread.
struct Event(HANDLE);

// SAFETY: event handles may be signalled and waited on from any thread.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

struct Shared {
    pending: Mutex<Option<Vec<RegistryKey>>>,
    stop: AtomicBool,
    wake: Event,
}

impl Drop for Shared {
    fn drop(&mut self) {
        // SAFETY: the last owner closes the event; nobody signals it after.
        unsafe { CloseHandle(self.wake.0) };
    }
}

pub struct KeyWatcher {
    shared: Arc<Shared>,
}

impl KeyWatcher {
    pub fn new(changed: Sender<Msg>) -> Option<KeyWatcher> {
        // SAFETY: an unnamed auto-reset event, closed when `Shared` drops.
        let wake = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if wake.is_null() {
            return None;
        }
        let shared = Arc::new(Shared { pending: Mutex::new(None), stop: AtomicBool::new(false), wake: Event(wake) });
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("savescummer-watch-registry".into())
            .spawn(move || run(worker, changed))
            .ok()?;
        Some(KeyWatcher { shared })
    }

    pub fn set_keys(&self, keys: Vec<RegistryKey>) {
        *self.shared.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(keys);
        // SAFETY: the event lives as long as `Shared`.
        unsafe { SetEvent(self.shared.wake.0) };
    }
}

impl Drop for KeyWatcher {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // SAFETY: see set_keys.
        unsafe { SetEvent(self.shared.wake.0) };
    }
}

/// One watched key: its handle and the event its notification signals.
struct Watched {
    key: HKEY,
    event: HANDLE,
}

impl Watched {
    fn open(spec: &RegistryKey) -> Option<Watched> {
        let root = match spec.hive {
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::CurrentUser => HKEY_CURRENT_USER,
        };
        let path: Vec<u16> = spec.path.encode_utf16().chain(Some(0)).collect();
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: the key is closed in Drop.
        if unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_NOTIFY, &mut key) } != 0 {
            return None;
        }
        // SAFETY: an unnamed auto-reset event, closed in Drop.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        let watched = Watched { key, event };
        (!event.is_null() && watched.arm()).then_some(watched)
    }

    /// Asks for the next change. Each notification fires once.
    fn arm(&self) -> bool {
        // SAFETY: both handles are open while `self` lives.
        unsafe {
            RegNotifyChangeKeyValue(self.key, 1, REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET, self.event, 1)
                == 0
        }
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        // SAFETY: opened in `open`; closing the key cancels its notification.
        unsafe {
            RegCloseKey(self.key);
            if !self.event.is_null() {
                CloseHandle(self.event);
            }
        }
    }
}

fn run(shared: Arc<Shared>, changed: Sender<Msg>) {
    let mut watched: Vec<Watched> = Vec::new();
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let pending = shared.pending.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(keys) = pending {
            watched.clear();
            watched.extend(keys.iter().filter_map(Watched::open));
        }
        let handles: Vec<HANDLE> = std::iter::once(shared.wake.0).chain(watched.iter().map(|w| w.event)).collect();
        // SAFETY: every handle stays open during the wait (at most 64; we
        // watch a handful).
        let result = unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE) };
        let index = result.wrapping_sub(WAIT_OBJECT_0) as usize;
        if index == 0 {
            continue; // new keys or stop
        }
        if index > watched.len() {
            break; // the wait failed; periodic and focus scans remain
        }
        if changed.send(Msg::Changed).is_err() {
            break;
        }
        // A deleted key can't be armed again; drop it until the next set_keys.
        if !watched[index - 1].arm() {
            watched.remove(index - 1);
        }
    }
}

#[cfg(test)]
pub(crate) mod scratch {
    //! A throwaway key under `HKCU\Software\SaveScummerTests`, deleted on drop.

    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteKeyW, RegDeleteTreeW, RegSetValueExW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub struct ScratchKey {
        pub path: String,
    }

    impl ScratchKey {
        pub fn new(name: &str) -> ScratchKey {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = format!(r"Software\SaveScummerTests\{name}-{}-{nanos}", std::process::id());
            set_value(&path, "created", "1");
            ScratchKey { path }
        }
    }

    impl Drop for ScratchKey {
        fn drop(&mut self) {
            // SAFETY: deletes only this test's own key, then the shared parent
            // if no other test still uses it (deleting a key with subkeys fails).
            unsafe {
                RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.path).as_ptr());
                // RegDeleteTreeW leaves the key itself.
                RegDeleteKeyW(HKEY_CURRENT_USER, wide(&self.path).as_ptr());
                RegDeleteKeyW(HKEY_CURRENT_USER, wide(r"Software\SaveScummerTests").as_ptr());
            }
        }
    }

    /// Creates `path` (and parents) under HKCU and sets a string value.
    pub fn set_value(path: &str, name: &str, value: &str) {
        let mut key: HKEY = std::ptr::null_mut();
        let data = wide(value);
        // SAFETY: plain registry calls on a key this test owns.
        unsafe {
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            );
            assert_eq!(status, 0, "create {path}");
            RegSetValueExW(key, wide(name).as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, (data.len() * 2) as u32);
            RegCloseKey(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_machine_wide_uninstall_keys_can_be_watched_without_admin_rights() {
        for path in [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let key = RegistryKey { hive: Hive::LocalMachine, path: path.into() };
            assert!(Watched::open(&key).is_some(), "{path}");
        }
        let missing = RegistryKey { hive: Hive::CurrentUser, path: r"Software\SaveScummer no such key".into() };
        assert!(Watched::open(&missing).is_none());
    }
}
