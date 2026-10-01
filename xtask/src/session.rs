//! The dev session (PLAN-BUILD.md XTASK): `run`, `host start` and `host stop`.
//!
//! The dev host runs from the dev package with data in `.runtime/dev` or
//! `.runtime/dev-demo`, so demo resets cannot touch normal dev saves. It's recorded in
//! `build/dev/session.json` and stopped only if its pid, start time and path
//! all still match, so a reused pid is never killed.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::build::{self, Options};
use crate::naming::{CLI, HOST, UI};
use crate::paths::{self, Mode};
use crate::{platform, procs};

const READY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize)]
struct Session {
    pid: u32,
    start_time: u64,
    exe: PathBuf,
    data_dir: PathBuf,
    log: PathBuf,
}

fn session_file() -> PathBuf {
    Mode::Dev.dir().join("session.json")
}

/// `host start`: a dev build, then the dev host in the tray, without the UI.
pub fn host_start(demo: bool) -> anyhow::Result<()> {
    let package = dev_package()?;
    start_host(&package, demo, true, false)
}

/// `host stop`.
pub fn host_stop() -> anyhow::Result<()> {
    stop_recorded()
}

/// `run`: a dev build, then the dev host, started the way a user starts the
/// app, so the host shows the UI itself.
pub fn run(demo: bool, no_integrations: bool, stop_other_hosts: bool) -> anyhow::Result<()> {
    let package = dev_package()?;
    let others = procs::other_hosts();
    if stop_other_hosts {
        procs::stop(others)?;
    } else if !others.is_empty() {
        println!(
            "warning: another SaveScummer host is running ({}); it keeps the tray icon and global shortcuts. \
             `--stop-other-hosts` stops it.",
            others.iter().map(|p| p.exe.display().to_string()).collect::<Vec<_>>().join(", ")
        );
    }
    start_host(&package, demo, false, no_integrations)?;

    if !platform::program(&package, UI).is_file() {
        println!(
            "no UI yet, so the dev host has no window to show. Drive it with\n  {} --data-dir {} status",
            paths::show(&platform::program(&package, CLI)),
            paths::show(&if demo { paths::demo_data() } else { paths::dev_data() }),
        );
    }
    println!("the dev host keeps running; `cargo xtask host stop` stops it");
    Ok(())
}

fn dev_package() -> anyhow::Result<PathBuf> {
    let built = build::build(&Options { release: false, test: false, package: true })?;
    Ok(built.package.expect("packaged"))
}

fn start_host(package: &Path, demo: bool, minimized: bool, no_integrations: bool) -> anyhow::Result<()> {
    stop_recorded()?;
    let exe = platform::program(package, HOST);
    let data_dir = if demo { paths::demo_data() } else { paths::dev_data() };
    fs::create_dir_all(&data_dir)?;

    // The dev host keeps the catalog it was built with: a download from the
    // release branch would hide catalog changes not released yet.
    let mut args: Vec<OsString> = vec!["--data-dir".into(), data_dir.clone().into(), "--no-catalog-update".into()];
    if demo {
        args.push("--demo".into());
    }
    if no_integrations {
        args.push("--no-integrations".into());
    }
    if minimized {
        args.push("--minimized".into());
    }
    println!("starting the dev host ({})", paths::show(&exe));
    #[cfg(target_os = "macos")]
    let (pid, log) = {
        let log = if demo { data_dir.join("data/host.log") } else { data_dir.join("host.log") };
        let offset = if demo { 0 } else { fs::metadata(&log).map(|meta| meta.len() as usize).unwrap_or(0) };
        platform::launch_app(package, &args, minimized)?;
        (wait_ready(None, &log, offset)?, log)
    };
    #[cfg(not(target_os = "macos"))]
    let (pid, log) = {
        let logs = Mode::Dev.dir().join("logs");
        fs::create_dir_all(&logs)?;
        let log = logs.join("host.log");
        let out = fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
        let pid = platform::spawn_detached(&exe, &args, &out)?;
        (wait_ready(Some(pid), &log, 0)?, log)
    };
    let start_time = procs::start_time(pid).context("the dev host exited right after starting")?;
    let session = Session { pid, start_time, exe, data_dir, log };
    fs::write(session_file(), serde_json::to_string_pretty(&session)? + "\n")?;
    println!(
        "dev host ready (pid {pid}), data in {}, log in {}",
        paths::show(&session.data_dir),
        paths::show(&session.log)
    );
    Ok(())
}

/// Waits for the host's ready line. On macOS Launch Services owns the new
/// process, so its own log supplies the pid instead of `Command::spawn`.
fn wait_ready(spawned_pid: Option<u32>, log: &Path, offset: usize) -> anyhow::Result<u32> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let text = fs::read_to_string(log).unwrap_or_default();
        let fresh = if text.len() >= offset { &text[offset..] } else { &text };
        for line in fresh.lines() {
            let json = line.split_once("ready line: ").map_or(line, |(_, json)| json);
            let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else { continue };
            match value.get("ready").and_then(|r| r.as_bool()) {
                Some(true) => {
                    let pid = value.get("pid").and_then(|pid| pid.as_u64()).map(|pid| pid as u32).or(spawned_pid);
                    return pid.context("the host's ready line had no pid");
                }
                Some(false) => bail!(
                    "the dev host didn't start: {}",
                    value.get("error").and_then(|e| e.as_str()).unwrap_or("no reason given")
                ),
                None => {}
            }
        }
        if spawned_pid.is_some_and(|pid| procs::start_time(pid).is_none()) {
            let text = fs::read_to_string(log).unwrap_or_default();
            let tail: Vec<&str> = text.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
            bail!("the dev host exited before it was ready ({}):\n{}", paths::show(log), tail.join("\n"));
        }
        if Instant::now() > deadline {
            bail!("the dev host wasn't ready after {}s; see {}", READY_TIMEOUT.as_secs(), paths::show(log));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Stops the recorded dev host if it's still the same process; the session
/// file is deleted last.
fn stop_recorded() -> anyhow::Result<()> {
    let path = session_file();
    let Ok(text) = fs::read_to_string(&path) else { return Ok(()) };
    if let Ok(session) = serde_json::from_str::<Session>(&text)
        && let Some(proc) = procs::find(session.pid, session.start_time, &session.exe)
    {
        procs::stop(vec![proc])?;
    }
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))
}

/// For `clean`: the recorded host, then everything else running from output.
pub fn stop_all_output() -> anyhow::Result<()> {
    stop_recorded()?;
    procs::stop_outputs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_services_reads_the_new_ready_line_from_the_host_log() {
        let path = std::env::temp_dir().join(format!("savescummer-ready-{}", uuid::Uuid::new_v4()));
        let old = "[earlier] ready line: {\"ready\":true,\"pid\":11}\n";
        let new = "[now] ready line: {\"ready\":true,\"pid\":42}\n";
        fs::write(&path, format!("{old}{new}")).unwrap();
        assert_eq!(wait_ready(None, &path, old.len()).unwrap(), 42);
        fs::remove_file(path).unwrap();
    }
}
