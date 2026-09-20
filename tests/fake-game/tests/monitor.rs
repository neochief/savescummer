#![cfg(windows)]
use savescummer_core::*;
use savescummer_monitor::{Monitor, ObservationSource};
use savescummer_platform::{NativeObserver, Paths};
use std::{
    collections::BTreeMap,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn start(path: &Path) -> Process {
    use std::os::windows::process::CommandExt;
    Process(
        Command::new(path)
            .args(["--headless", "--duration-ms", "60000"])
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn wait(
    monitor: &mut Monitor,
    games: &BTreeMap<Id, Game>,
    count: usize,
) -> savescummer_monitor::Activity {
    let start = Instant::now();
    loop {
        let activity = monitor.observe(games, NativeObserver.observe().unwrap());
        if activity.stack.len() == count {
            eprintln!(
                "process observation after {} ms",
                start.elapsed().as_millis()
            );
            return activity;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "process observation timeout: {activity:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn discovers_real_processes_by_full_path_and_retains_game_until_last_exit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Game.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_savescummer-fake-game"), &path).unwrap();
    let other_dir = temp.path().join("unrelated");
    std::fs::create_dir(&other_dir).unwrap();
    let other = other_dir.join("Game.exe");
    std::fs::copy(&path, &other).unwrap();
    let paths = Paths::new(vec![]);
    let game = Game {
        detected_locations: vec![],
        user_configured: true,
        id: "game".into(),
        name: "Game".into(),
        info: String::new(),
        data_dir: temp.path().join("saves"),
        executables: vec![paths.resolve(&path).unwrap()],
        installed: true,
        configuration_error: None,
    };
    let games = [(game.id.clone(), game)].into_iter().collect();
    let mut monitor = Monitor::default();
    wait(&mut monitor, &games, 0);
    let a = start(&path);
    let activity = wait(&mut monitor, &games, 1);
    assert_eq!(activity.started, ["game"]);
    let b = start(&path);
    let unrelated = start(&other);
    // Establish both actual PIDs in the OS snapshot before terminating the first.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let obs = NativeObserver.observe().unwrap();
        if obs.processes.iter().any(|p| p.pid == b.0.id())
            && obs.processes.iter().any(|p| p.pid == unrelated.0.id())
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(a);
    let activity = wait(&mut monitor, &games, 1);
    assert!(activity.closed.is_empty());
    assert!(activity.started.is_empty());
    drop(b);
    let activity = wait(&mut monitor, &games, 0);
    assert_eq!(activity.closed, ["game"]);
    drop(unrelated);
}
