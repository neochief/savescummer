//! The macOS process list: `proc_listallpids`, then each process's path and
//! BSD info from `libproc`, and the frontmost app from `NSWorkspace`.
//!
//! `NSWorkspace` only learns about app switches while the main thread runs
//! its run loop, which the host always does on macOS
//! (`platform::integration::run_main_loop`). Why not `CGWindowList`: it
//! needs Screen Recording permission on current macOS.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidActivateApplicationNotification};
use objc2_foundation::NSNotification;

use super::{Proc, ProcessSource};

/// Caches each process's path and parent by pid and start time, so a poll
/// asks only about processes it hasn't seen. The parent is the one first
/// seen: when a launcher exits, macOS hands its child to launchd, and the
/// child must still count as the launcher's game (as on Windows, which
/// keeps the original parent).
#[derive(Default)]
pub struct MacSource {
    known: HashMap<(u32, u64), (u32, Option<PathBuf>)>,
    pids: HashSet<u32>,
}

/// Set on the main thread whenever another app comes to the front.
static SWITCHED: AtomicBool = AtomicBool::new(false);

impl ProcessSource for MacSource {
    fn list(&mut self) -> Vec<Proc> {
        let mut fresh = HashMap::new();
        let mut procs = Vec::new();
        for pid in all_pids() {
            let Some(info) = bsd_info(pid) else { continue };
            // A zombie has exited; only its parent hasn't noticed yet.
            if info.pbi_status == libc::SZOMB {
                continue;
            }
            let key = (pid, info.pbi_start_tvsec);
            let (parent, exe) = match self.known.get(&key) {
                Some(known) => known.clone(),
                None => (info.pbi_ppid, pid_path(pid)),
            };
            fresh.insert(key, (parent, exe.clone()));
            procs.push(Proc { pid, parent, exe });
        }
        self.known = fresh;
        self.pids = procs.iter().map(|p| p.pid).collect();
        procs
    }

    fn may_have_changed(&mut self, watched: &[u32]) -> bool {
        if SWITCHED.swap(false, Ordering::SeqCst) {
            return true;
        }
        if self.foreground().is_some_and(|pid| !self.pids.contains(&pid)) {
            return true;
        }
        // A game starting another program (a launcher starting the real
        // game): look now, while its parent is still known.
        if watched.iter().any(|&pid| children(pid).iter().any(|child| !self.pids.contains(child))) {
            return true;
        }
        watched.iter().any(|&pid| bsd_info(pid).is_none_or(|info| info.pbi_status == libc::SZOMB))
    }

    fn foreground(&mut self) -> Option<u32> {
        observe_switches();
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        u32::try_from(app.processIdentifier()).ok().filter(|&pid| pid != 0)
    }
}

/// Registers, once per process, for app switches. The observer also makes
/// `NSWorkspace` keep `frontmostApplication` current.
fn observe_switches() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let block = RcBlock::new(|_: NonNull<NSNotification>| SWITCHED.store(true, Ordering::SeqCst));
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: the block takes the notification and touches only an atomic.
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
                None,
                &block,
            )
        };
        // Observed for the process's lifetime.
        std::mem::forget(observer);
    });
}

fn all_pids() -> Vec<u32> {
    // SAFETY: a null buffer asks for the count only.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Vec::new();
    }
    // Room for processes started since.
    let mut pids = vec![0 as libc::pid_t; count as usize + 64];
    let bytes = (pids.len() * size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: the buffer holds `bytes` bytes; the call returns how many
    // pids it wrote.
    let got = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(got.max(0) as usize);
    pids.into_iter().filter_map(|pid| u32::try_from(pid).ok()).filter(|&pid| pid != 0).collect()
}

/// The processes `pid` started that are still its children.
fn children(pid: u32) -> Vec<u32> {
    let mut pids = [0 as libc::pid_t; 64];
    let bytes = size_of_val(&pids) as libc::c_int;
    // SAFETY: the buffer holds `bytes` bytes; the call returns how many
    // pids it wrote.
    let got = unsafe { libc::proc_listchildpids(pid as libc::c_int, pids.as_mut_ptr().cast(), bytes) };
    pids[..got.clamp(0, 64) as usize].iter().filter_map(|&p| u32::try_from(p).ok()).collect()
}

/// A process's parent, status and start time; None once it's gone (or
/// isn't ours to ask about, which never happens for this call).
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: a zeroed plain-data struct the kernel fills in, with its size.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let got =
        unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (got == size).then_some(info)
}

/// The executable's path; None for another user's or a protected process.
fn pid_path(pid: u32) -> Option<PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer has the size we pass.
    let len = unsafe { libc::proc_pidpath(pid as libc::c_int, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if len <= 0 {
        return None;
    }
    buffer.truncate(len as usize);
    Some(PathBuf::from(OsString::from_vec(buffer)))
}

pub fn system_source() -> Box<dyn ProcessSource> {
    Box::new(MacSource::default())
}
