use std::os::unix::process::CommandExt;
use std::process::Command;

/// Makes `command` start a program that outlives us: its own process group,
/// so Ctrl+C or closing the terminal that started us doesn't reach it. The
/// standard library opens files close-on-exec, so it inherits nothing else.
pub fn detach(command: &mut Command) {
    command.process_group(0);
}

/// Ends the process at once, the way a crash or a kill would: no atexit
/// handlers, no flushing.
pub fn hard_exit(code: i32) -> ! {
    // SAFETY: `_exit` ends the process without touching its state.
    unsafe { libc::_exit(code) }
}

pub fn request_game_close(pid: u32) -> std::io::Result<()> {
    let pid = i32::try_from(pid).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    if pid <= 0 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    #[cfg(target_os = "macos")]
    if let Some(app) = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        return if app.terminate() { Ok(()) } else { Err(std::io::ErrorKind::NotFound.into()) };
    }
    // Non-app executables use the conventional graceful termination signal.
    // SAFETY: a positive, observed PID; SIGTERM gives the process time to clean up.
    if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}
