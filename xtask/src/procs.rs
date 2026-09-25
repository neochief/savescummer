//! Stopping processes politely (PLAN-BUILD.md STOPPING PROCESSES).
//!
//! A running host may be in the middle of a save, and Windows can't replace
//! a running executable, so every stop goes through the same routine:
//!
//! 1. ask UIs to close
//! 2. ask hosts to shut down through their sibling CLI, so an accepted
//!    operation finishes
//! 3. terminate whatever is still running
//!
//! Every build, `check`, `dist` and `clean` starts by stopping everything
//! running from the output folders (`target/`, `build/`, `dist/`): hosts,
//! CLIs, UIs, test binaries. A rebuild is then always clean: nothing
//! half-replaced, no stale host serving old code, no "access denied" on a
//! locked executable. `.runtime/` and installed copies are never touched.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::naming::{self, CARGO_CLI, CARGO_HOST, CLI, HOST, UI};
use crate::{paths, platform};

const UI_GRACE: Duration = Duration::from_secs(10);
const HOST_GRACE: Duration = Duration::from_secs(30);
const KILL_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: u32,
    pub exe: PathBuf,
    pub cmd: Vec<OsString>,
    /// Seconds since the epoch; with the pid and path, identifies the process.
    pub start_time: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum Role {
    Ui,
    Host,
    Other,
}

impl Proc {
    fn role(&self) -> Role {
        let name = naming::program_name(&self.exe);
        let is = |program: &str| name.eq_ignore_ascii_case(program);
        if is(UI) {
            Role::Ui
        } else if is(HOST) || is(CARGO_HOST) {
            Role::Host
        } else {
            Role::Other
        }
    }

    /// The host's `--data-dir`, if it was given one.
    fn data_dir(&self) -> Option<PathBuf> {
        let args: Vec<String> = self.cmd.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        args.iter().enumerate().find_map(|(i, arg)| {
            if arg == "--data-dir" {
                args.get(i + 1).map(PathBuf::from)
            } else {
                arg.strip_prefix("--data-dir=").map(PathBuf::from)
            }
        })
    }

    fn describe(&self) -> String {
        format!("{} (pid {})", paths::show(&self.exe), self.pid)
    }
}

/// Every process of this user that sysinfo can see the executable of.
pub fn all() -> Vec<Proc> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always).with_cmd(UpdateKind::Always),
    );
    let own = std::process::id();
    system
        .processes()
        .values()
        .filter(|p| p.pid().as_u32() != own)
        .filter_map(|p| {
            Some(Proc {
                pid: p.pid().as_u32(),
                exe: p.exe()?.to_path_buf(),
                cmd: p.cmd().to_vec(),
                start_time: p.start_time(),
            })
        })
        .collect()
}

/// Processes whose executable lives under `dir`.
pub fn under(dir: &Path) -> Vec<Proc> {
    all().into_iter().filter(|p| is_under(&p.exe, dir)).collect()
}

/// Everything running from this checkout's output folders, except xtask
/// itself (any instance) and Cargo's build scripts, which belong to a build
/// in progress rather than to the app.
pub fn outputs() -> Vec<Proc> {
    let roots = [paths::target(), paths::build(), paths::dist()];
    all()
        .into_iter()
        .filter(|p| roots.iter().any(|root| is_under(&p.exe, root)))
        .filter(|p| {
            let stem = p.exe.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            stem != "xtask" && !stem.starts_with("xtask-") && !stem.starts_with("build-script-")
        })
        .collect()
}

/// Stops everything running from the output folders, before anything in
/// them is rebuilt or removed.
pub fn stop_outputs() -> anyhow::Result<()> {
    stop(outputs())
}

/// Running hosts that aren't built output (typically the installed app).
pub fn other_hosts() -> Vec<Proc> {
    let roots = [paths::target(), paths::build(), paths::dist()];
    all()
        .into_iter()
        .filter(|p| p.role() == Role::Host && is_named(&p.exe, HOST))
        .filter(|p| !roots.iter().any(|root| is_under(&p.exe, root)))
        .collect()
}

/// The process with this identity, if it's still running.
pub fn find(pid: u32, start_time: u64, exe: &Path) -> Option<Proc> {
    all().into_iter().find(|p| p.pid == pid && p.start_time == start_time && same_path(&p.exe, exe))
}

/// Start time of a process we just spawned, for recording its identity.
pub fn start_time(pid: u32) -> Option<u64> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(Pid::from_u32(pid)).map(|p| p.start_time())
}

/// Stops everything running from `dir`, an output folder.
pub fn stop_under(dir: &Path) -> anyhow::Result<()> {
    stop(under(dir))
}

/// The polite routine: UIs close, hosts shut down, the rest is terminated.
pub fn stop(procs: Vec<Proc>) -> anyhow::Result<()> {
    if procs.is_empty() {
        return Ok(());
    }
    let (uis, rest): (Vec<_>, Vec<_>) = procs.into_iter().partition(|p| p.role() == Role::Ui);
    let (hosts, others): (Vec<_>, Vec<_>) = rest.into_iter().partition(|p| p.role() == Role::Host);

    for ui in &uis {
        println!("asking {} to close", ui.describe());
        platform::ask_to_close(ui.pid);
    }
    wait_gone(&uis, UI_GRACE);

    for host in &hosts {
        shut_down_host(host);
    }

    let leftovers: Vec<Proc> = uis.into_iter().chain(hosts).chain(others).filter(alive).collect();
    for proc in &leftovers {
        println!("terminating {}", proc.describe());
        terminate(proc.pid);
    }
    if !wait_gone(&leftovers, KILL_GRACE) {
        let names: Vec<String> = leftovers.iter().filter(|p| alive(p)).map(Proc::describe).collect();
        anyhow::bail!("couldn't stop {}; close it and try again", names.join(", "));
    }
    Ok(())
}

/// Asks the host to shut down through the CLI next to it, then waits for it
/// to exit. Nothing is forced here: leftovers are terminated by the caller.
fn shut_down_host(host: &Proc) {
    let Some(dir) = host.exe.parent() else { return };
    let Some(cli) = [CLI, CARGO_CLI].iter().map(|name| dir.join(naming::exe(name))).find(|p| p.is_file()) else {
        println!("no CLI next to {}; it will be terminated", host.describe());
        return;
    };
    println!("asking {} to shut down", host.describe());
    let mut command = Command::new(&cli);
    command.arg("--no-start");
    if let Some(data_dir) = host.data_dir() {
        command.arg("--data-dir").arg(data_dir);
    }
    command.arg("shutdown").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    platform::hide_window(&mut command);
    let child = command.spawn();
    let exited = wait_gone(std::slice::from_ref(host), HOST_GRACE);
    if let Ok(mut child) = child {
        if !exited {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

fn alive(proc: &Proc) -> bool {
    find(proc.pid, proc.start_time, &proc.exe).is_some()
}

fn terminate(pid: u32) {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing(),
    );
    if let Some(process) = system.process(Pid::from_u32(pid)) {
        process.kill();
    }
}

/// Waits until none of `procs` is running; `false` if some still are.
fn wait_gone(procs: &[Proc], timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !procs.iter().any(alive) {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn is_named(exe: &Path, name: &str) -> bool {
    naming::program_name(exe).eq_ignore_ascii_case(name)
}

fn normalized(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).trim_end_matches('/').to_string();
    if cfg!(windows) { text.to_lowercase() } else { text }
}

fn same_path(a: &Path, b: &Path) -> bool {
    normalized(a) == normalized(b)
}

pub fn is_under(path: &Path, dir: &Path) -> bool {
    let (path, dir) = (normalized(path), normalized(dir));
    path.len() > dir.len() && path.starts_with(&dir) && path.as_bytes()[dir.len()] == b'/'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_matches_whole_components() {
        let root = Path::new("/repo/build");
        assert!(is_under(Path::new("/repo/build/dev/x"), root));
        assert!(!is_under(Path::new("/repo/builder/x"), root));
        assert!(!is_under(Path::new("/repo/build"), root));
    }

    #[test]
    fn reads_the_data_dir_argument() {
        let proc = |args: &[&str]| Proc {
            pid: 1,
            exe: PathBuf::from("SaveScummer"),
            cmd: args.iter().map(OsString::from).collect(),
            start_time: 0,
        };
        assert_eq!(proc(&["host", "--data-dir", "d"]).data_dir(), Some(PathBuf::from("d")));
        assert_eq!(proc(&["host", "--data-dir=e"]).data_dir(), Some(PathBuf::from("e")));
        assert_eq!(proc(&["host", "--minimized"]).data_dir(), None);
    }
}
