//! Process helpers shared by the macOS and Linux modules.

#[cfg(not(target_os = "macos"))]
use std::ffi::OsString;
#[cfg(not(target_os = "macos"))]
use std::fs;
#[cfg(not(target_os = "macos"))]
use std::os::unix::process::CommandExt;
#[cfg(not(target_os = "macos"))]
use std::path::Path;
use std::process::{Command, Stdio};

#[cfg(not(target_os = "macos"))]
use anyhow::Context;

pub fn hide_window(_command: &mut Command) {}

/// Starts `exe` so it outlives xtask: its own process group (Ctrl+C in this
/// terminal doesn't reach it) and output going to `log`. std opens every file
/// close-on-exec, so it inherits nothing else.
#[cfg(not(target_os = "macos"))]
pub fn spawn_detached(exe: &Path, args: &[OsString], log: &fs::File) -> anyhow::Result<u32> {
    let child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    Ok(child.id())
}

pub fn ask_to_close(pid: u32) {
    let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
}
