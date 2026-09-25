//! macOS: `proc_pidinfo(PROC_PIDLISTFDS)` lists a process's descriptors and
//! `proc_pidfdinfo(PROC_PIDFDVNODEINFO)` gives each file's device and inode.
//! `libproc` is what `lsof` and Activity Monitor use, but Apple documents it
//! as private and subject to change: any unexpected answer means "couldn't
//! inspect", never a crash. Only the user's own processes can be read, and
//! protected ones may refuse.

use std::io;
use std::mem::size_of;

use super::{FileId, Unreadable};

pub const SUPPORTED: bool = true;

/// `PROC_PIDFDVNODEINFO` from `sys/proc_info.h`.
const PROC_PIDFDVNODEINFO: libc::c_int = 1;

/// `struct proc_fileinfo` from `sys/proc_info.h`.
#[repr(C)]
struct ProcFileInfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: libc::off_t,
    fi_type: i32,
    fi_guardflags: u32,
}

/// `struct vnode_fdinfo` from `sys/proc_info.h`.
#[repr(C)]
struct VnodeFdInfo {
    pfi: ProcFileInfo,
    pvi: libc::vnode_info,
}

/// (device, inode) as the kernel reports them: the device is 32 bits.
pub type Open = (u32, u64);

pub fn open_files(pid: u32) -> Result<Vec<Open>, Unreadable> {
    let pid = pid as libc::c_int;
    let fds = list_fds(pid)?;
    let mut open = Vec::new();
    for fd in fds {
        if fd.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            continue;
        }
        // SAFETY: a zeroed plain-data struct the kernel fills in, with its size.
        let mut info: VnodeFdInfo = unsafe { std::mem::zeroed() };
        let size = size_of::<VnodeFdInfo>() as libc::c_int;
        let got = unsafe { libc::proc_pidfdinfo(pid, fd.proc_fd, PROC_PIDFDVNODEINFO, (&raw mut info).cast(), size) };
        if got == size {
            open.push((info.pvi.vi_stat.vst_dev, info.pvi.vi_stat.vst_ino));
            continue;
        }
        match io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => return Err(Unreadable::Exited),
            // Closed between listing and looking.
            Some(libc::EBADF) => {}
            _ => return Err(refused(pid, "proc_pidfdinfo")),
        }
    }
    Ok(open)
}

fn list_fds(pid: libc::c_int) -> Result<Vec<libc::proc_fdinfo>, Unreadable> {
    let mut capacity = 64usize;
    loop {
        let mut fds: Vec<libc::proc_fdinfo> = Vec::with_capacity(capacity);
        let bytes = (capacity * size_of::<libc::proc_fdinfo>()) as libc::c_int;
        // SAFETY: the buffer holds `capacity` entries; the kernel writes at
        // most `bytes` and returns how many it wrote.
        let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, fds.as_mut_ptr().cast(), bytes) };
        if got <= 0 {
            return match io::Error::last_os_error().raw_os_error() {
                Some(libc::ESRCH) => Err(Unreadable::Exited),
                _ => Err(refused(pid, "proc_pidinfo")),
            };
        }
        let count = got as usize / size_of::<libc::proc_fdinfo>();
        if count < capacity {
            // SAFETY: the kernel initialized `count` entries.
            unsafe { fds.set_len(count) };
            return Ok(fds);
        }
        // The buffer was full: there may be more.
        capacity *= 4;
    }
}

fn refused(pid: libc::c_int, call: &str) -> Unreadable {
    Unreadable::Refused(format!("{call} for pid {pid}: {}", io::Error::last_os_error()))
}

pub fn same(open: &Open, file: &FileId) -> bool {
    open.0 == file.dev as u32 && open.1 == file.ino
}
