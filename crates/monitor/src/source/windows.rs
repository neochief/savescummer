//! The Windows process list: a Toolhelp snapshot, image paths, and the
//! foreground window's process.

use std::collections::{HashMap, HashSet};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
    WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::{Proc, ProcessSource};

/// Caches each process's path by pid and parent, so a poll opens only
/// processes it hasn't seen. Between full looks it answers from the ids
/// of the last snapshot, the foreground window and handles on the
/// running games' processes, which are all cheap.
#[derive(Default)]
pub struct WindowsSource {
    paths: HashMap<(u32, u32), Option<PathBuf>>,
    pids: HashSet<u32>,
    exits: HashMap<u32, Handle>,
}

/// A process handle that can only be waited on; closed on drop.
struct Handle(HANDLE);

// SAFETY: a process handle may be used from any thread.
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: opened by us, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn image_path(pid: u32) -> Option<PathBuf> {
    // SAFETY: plain Win32 calls with a buffer we own; the handle is closed.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len as usize])))
    }
}

impl ProcessSource for WindowsSource {
    fn list(&mut self) -> Vec<Proc> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        // SAFETY: the snapshot handle is closed below; the entry struct is
        // initialized with its size as the API requires.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Vec::new();
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snapshot, &mut entry);
            while ok != 0 {
                out.push((entry.th32ProcessID, entry.th32ParentProcessID));
                ok = Process32NextW(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
        let mut fresh = HashMap::new();
        let procs = out
            .into_iter()
            .filter(|(pid, _)| *pid != 0 && *pid != 4)
            .map(|(pid, parent)| {
                let exe = match self.paths.get(&(pid, parent)) {
                    Some(path) => path.clone(),
                    None => image_path(pid),
                };
                fresh.insert((pid, parent), exe.clone());
                Proc { pid, parent, exe }
            })
            .collect::<Vec<Proc>>();
        self.paths = fresh;
        self.pids = procs.iter().map(|p| p.pid).collect();
        procs
    }

    fn may_have_changed(&mut self, watched: &[u32]) -> bool {
        // A process we haven't listed is in front: likely a game starting.
        if self.foreground().is_some_and(|pid| !self.pids.contains(&pid)) {
            return true;
        }
        self.exits.retain(|pid, _| watched.contains(pid));
        for pid in watched {
            let handle = self.exits.entry(*pid).or_insert_with(|| {
                // SAFETY: a wait-only handle, closed on drop. Null if the
                // process is already gone, which counts as an exit.
                Handle(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, *pid) })
            });
            // SAFETY: a zero-timeout wait on our own handle.
            if handle.0.is_null() || unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 {
                return true;
            }
        }
        false
    }

    fn foreground(&mut self) -> Option<u32> {
        // SAFETY: reading the foreground window's owner has no preconditions.
        unsafe {
            let window = GetForegroundWindow();
            if window.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, &mut pid);
            (pid != 0).then_some(pid)
        }
    }
}

pub fn system_source() -> Box<dyn ProcessSource> {
    Box::new(WindowsSource::default())
}
