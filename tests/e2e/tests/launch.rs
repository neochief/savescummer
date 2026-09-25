//! Starting the app (PLAN-HOST, PROCESSES): a user launch shows the UI,
//! `--minimized` doesn't, a second launch reaches the running host, and
//! showing the UI brings an open one forward instead of starting another.
//! The UI is a stand-in (`fake-ui`) that logs what happens to it.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use common::*;

struct Launch {
    world: World,
    log: PathBuf,
}

impl Launch {
    fn new() -> Launch {
        let world = World::new();
        let log = world.root.join("ui.log");
        Launch { world, log }
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        vec![("SAVESCUMMER_UI_EXE", FAKE_UI.to_string()), ("FAKE_UI_LOG", self.log.to_string_lossy().into_owned())]
    }

    fn launched(&self) -> HostProcess {
        let env = self.env();
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.world.host_launched(&env)
    }

    fn minimized(&self) -> HostProcess {
        let env = self.env();
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.world.host_with(&[], &env)
    }

    fn lines(&self) -> Vec<String> {
        read_lines(&self.log)
    }

    fn wait_lines(&self, what: &str, check: impl Fn(&[String]) -> bool) -> Vec<String> {
        wait_for(what, Duration::from_secs(20), || {
            let lines = self.lines();
            check(&lines).then_some(lines)
        })
    }

    /// Starts a stand-in UI directly, the way the host would. It exits
    /// with the host.
    fn open_ui(&self) -> std::process::Child {
        let ui = Command::new(FAKE_UI)
            .arg("--data-dir")
            .arg(&self.world.data)
            .env("FAKE_UI_LOG", &self.log)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the stand-in UI");
        self.wait_lines("the UI connects", |l| l.iter().any(|l| l == "connected"));
        ui
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).unwrap_or_default().lines().map(str::to_string).collect()
}

fn count(lines: &[String], what: &str) -> usize {
    lines.iter().filter(|l| *l == what).count()
}

#[test]
fn a_user_launch_shows_the_ui() {
    let launch = Launch::new();
    let _host = launch.launched();
    launch.wait_lines("the host starts the UI", |l| count(l, "connected") == 1);
    assert_eq!(count(&launch.lines(), "started"), 1);
}

#[test]
fn a_minimized_start_shows_nothing() {
    let launch = Launch::new();
    let _host = launch.minimized();
    // Long enough for a UI to have started and logged.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(launch.lines().is_empty(), "{:?}", launch.lines());
}

#[test]
fn a_second_launch_reaches_the_running_host() {
    let launch = Launch::new();
    let _host = launch.minimized();
    let mut ui = launch.open_ui();

    let second = Command::new(HOST)
        .args(launch.world.launch_args())
        .envs(launch.env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("launch again");
    let out = output_within(second, CLI_LIMIT, "the second launch");
    assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(String::from_utf8_lossy(&out.stdout).contains("another host is already running"));

    let lines = launch.wait_lines("the open UI is asked to come forward", |l| count(l, "show") == 1);
    assert_eq!(count(&lines, "started"), 1, "no second UI: {lines:?}");
    // The stand-in exits with the host.
    launch.world.ok(&["shutdown"]);
    ui.wait().unwrap();
}

#[test]
fn showing_the_ui_starts_one_only_when_none_is_open() {
    let launch = Launch::new();
    let _host = launch.minimized();

    assert_eq!(launch.world.ok(&["show-ui"])["ui"], "started");
    launch.wait_lines("the UI connects", |l| count(l, "connected") == 1);
    assert_eq!(launch.world.ok(&["show-ui"])["ui"], "front");
    let lines = launch.wait_lines("the open UI comes forward", |l| count(l, "show") == 1);
    assert_eq!(count(&lines, "started"), 1, "{lines:?}");
}

#[test]
fn a_host_the_cli_starts_shows_no_ui() {
    let launch = Launch::new();
    let world = &launch.world;
    let mut command = Command::new(CLI);
    command.arg("--data-dir").arg(&world.data).arg("--json");
    // The test host's arguments, minus the data folder the CLI passes itself.
    let args = world.host_args();
    for arg in args.iter().skip(2) {
        command.arg(format!("--host-arg={arg}"));
    }
    command.arg("status").env("SAVESCUMMER_HOST_EXE", HOST).envs(launch.env());
    let out = output_within(
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap(),
        CLI_LIMIT,
        "the CLI starting a host",
    );
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(launch.lines().is_empty(), "{:?}", launch.lines());
    world.ok(&["shutdown"]);
}

#[test]
fn an_install_without_a_ui_says_so() {
    let world = World::new();
    let _host = world.host();
    assert_eq!(world.ok(&["show-ui"])["ui"], "unavailable");
}

#[test]
fn a_one_off_focus_report_outlives_its_connection_but_the_uis_does_not() {
    let launch = Launch::new();
    let world = &launch.world;
    world.steam_install(1001, "Rogue One", "RogueOne.exe");
    let _host = launch.minimized();
    let target = || world.ok(&["hotkey-target"]);

    // The CLI standing in for the UI: its report stays after it exits.
    world.ok(&["ui-report", "--focused", "--visible", "--selected", "steam-1001"]);
    assert_eq!(target()["source"], "window");

    // A real UI (it watches) going away takes its focus with it.
    let mut ui = launch.open_ui();
    world.ok(&["ui-report", "--focused", "--visible", "--selected", "steam-1001"]);
    assert_eq!(target()["source"], "window");
    ui.kill().unwrap();
    ui.wait().unwrap();
    wait_for("the closed UI's focus is dropped", Duration::from_secs(20), || {
        (target()["source"] != "window").then_some(())
    });
    // And showing the UI starts a new one rather than calling the closed one.
    assert_eq!(world.ok(&["show-ui"])["ui"], "started");
}

#[cfg(unix)]
#[test]
fn a_data_folder_too_long_for_a_socket_is_refused_with_the_reason() {
    let world = World::new();
    let data = world.root.join("x".repeat(120));
    let host = Command::new(HOST)
        .args(["--minimized", "--no-integrations", "--no-catalog-update", "--data-dir"])
        .arg(&data)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = output_within(host, CLI_LIMIT, "a host with a long data folder");
    let line = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(2), "{line}");
    assert!(line.contains("path is too long"), "{line}");

    let cli = Command::new(CLI)
        .args(["--no-start", "status", "--data-dir"])
        .arg(&data)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = output_within(cli, CLI_LIMIT, "the CLI with a long data folder");
    assert!(String::from_utf8_lossy(&out.stderr).contains("path is too long"), "{out:?}");
}

/// Found in review: logout or `launchctl bootout` while the host starts.
#[cfg(unix)]
#[test]
fn a_quit_signal_during_startup_still_exits_safely() {
    let world = World::new();
    let mut child = Command::new(HOST)
        .args(world.host_args())
        .env("SAVESCUMMER_TEST_DELAY_AT", "scan.discover:1:3000")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let log = || std::fs::read_to_string(world.data.join("host.log")).unwrap_or_default();
    wait_for("the first scan", Duration::from_secs(20), || log().contains("first scan").then_some(()));
    Command::new("kill").args(["-TERM", &child.id().to_string()]).status().unwrap();
    let exited = wait_for("the host to exit", Duration::from_secs(30), || child.try_wait().unwrap());
    let log = log();
    assert!(log.contains("host stopped"), "exited ({exited:?}) without the safe exit:\n{log}");
}
