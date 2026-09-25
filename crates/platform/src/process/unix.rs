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
