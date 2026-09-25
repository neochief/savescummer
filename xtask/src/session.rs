//! The dev session (PLAN-BUILD.md XTASK): `run`, `host start` and `host stop`.
//!
//! The dev host runs from the dev package with `--data-dir .runtime/dev`, so
//! development never touches the real app's data. It's recorded in
//! `build/dev/session.json` and stopped only if its pid, start time and path
//! all still match, so a reused pid is never killed.

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::build::{self, Options};
use crate::naming::{self, DESKTOP, HOST};
use crate::paths::{self, Mode};
use crate::{cmd, platform, procs};

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

/// `host start`: a dev build, then the dev host.
pub fn host_start(demo: bool) -> anyhow::Result<()> {
    let package = dev_package()?;
    start_host(&package, demo)
}

/// `host stop`.
pub fn host_stop() -> anyhow::Result<()> {
    stop_recorded()
}

/// `run`: a dev build, the dev host, then a desktop connected to it.
pub fn run(demo: bool, stop_other_hosts: bool) -> anyhow::Result<()> {
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
    start_host(&package, demo)?;

    let desktop = package.join("bin").join(naming::exe(DESKTOP));
    if !desktop.is_file() {
        println!(
            "no desktop yet: the dev host keeps running. Drive it with\n  {} --data-dir {} status\nand stop it with `cargo xtask host stop`.",
            paths::show(&package.join("bin").join(naming::exe(naming::CLI))),
            paths::show(&paths::dev_data()),
        );
        return Ok(());
    }
    let mut command = Command::new(&desktop);
    command.arg("--data-dir").arg(paths::dev_data());
    if demo {
        command.arg("--demo");
    }
    cmd::run(&mut command)?;
    println!("the dev host keeps running; `cargo xtask host stop` stops it");
    Ok(())
}

fn dev_package() -> anyhow::Result<PathBuf> {
    let built = build::build(&Options { release: false, test: false, package: true })?;
    Ok(built.package.expect("packaged"))
}

fn start_host(package: &Path, demo: bool) -> anyhow::Result<()> {
    stop_recorded()?;
    let exe = package.join("bin").join(naming::exe(HOST));
    let data_dir = paths::dev_data();
    let logs = Mode::Dev.dir().join("logs");
    fs::create_dir_all(&logs)?;
    fs::create_dir_all(&data_dir)?;
    let log = logs.join("host.log");
    let out = fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;

    let mut args: Vec<OsString> = vec!["--data-dir".into(), data_dir.clone().into()];
    if demo {
        args.push("--demo".into());
    }
    println!("starting the dev host ({})", paths::show(&exe));
    let pid = platform::spawn_detached(&exe, &args, &out)?;

    wait_ready(pid, &log)?;
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

/// Waits for the host's `{"ready":true,…}` line in its log.
fn wait_ready(pid: u32, log: &Path) -> anyhow::Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let file = fs::File::open(log)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            match value.get("ready").and_then(|r| r.as_bool()) {
                Some(true) => return Ok(()),
                Some(false) => bail!(
                    "the dev host didn't start: {}",
                    value.get("error").and_then(|e| e.as_str()).unwrap_or("no reason given")
                ),
                None => {}
            }
        }
        if procs::start_time(pid).is_none() {
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
