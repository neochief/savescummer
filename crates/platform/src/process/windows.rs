use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, GetCurrentProcess, TerminateProcess,
};

/// Makes `command` start a program that outlives us: no console of ours, its
/// own process group, and none of our standard handles.
pub fn detach(command: &mut Command) {
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    // Windows passes every inheritable handle to a child, so a program
    // started by a caller whose output is captured would hold the capturing
    // pipe open for its whole life.
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: clearing a flag on our own standard handles.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Ends the process at once, the way a crash or a kill would: no cleanup.
pub fn hard_exit(code: i32) -> ! {
    // SAFETY: terminating our own process.
    unsafe {
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    std::process::exit(code)
}

pub fn request_game_close(pid: u32) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE};

    struct Target {
        pid: u32,
        sent: bool,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> i32 {
        // SAFETY: EnumWindows calls this synchronously while Target is alive.
        let target = unsafe { &mut *(data as *mut Target) };
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut owner);
        }
        if owner == target.pid && unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } != 0 {
            target.sent = true;
        }
        1
    }
    let mut target = Target { pid, sent: false };
    // SAFETY: the callback only uses target during this synchronous call.
    unsafe {
        EnumWindows(Some(visit), &mut target as *mut Target as LPARAM);
    }
    if target.sent { Ok(()) } else { Err(std::io::ErrorKind::NotFound.into()) }
}
