//! Game monitoring through the real OS process list: the ACTIVE STACK,
//! session markers, launchers, games already running at host start, and
//! which game the hotkeys act on.

mod common;

use std::time::Duration;

use common::*;

fn two_games(world: &World) -> ((String, std::path::PathBuf), (String, std::path::PathBuf)) {
    let a_saves = world.home.join("Saves").join("Alpha");
    let b_saves = world.home.join("Saves").join("Beta");
    write(&a_saves.join("a.sav"), "a");
    write(&b_saves.join("b.sav"), "b");
    (world.custom_game("Alpha", &a_saves), world.custom_game("Beta", &b_saves))
}

fn stack(world: &World) -> Vec<String> {
    world.state()["active_stack"].as_array().unwrap().iter().map(s).collect()
}

#[test]
fn the_active_stack_follows_starts_and_exits() {
    let world = World::new();
    let _host = world.host();
    let ((a, a_exe), (b, b_exe)) = two_games(&world);
    assert!(world.ok(&["hotkey-target"])["game"].is_null(), "nothing running: hotkeys do nothing");
    assert_eq!(world.cli(&["hotkey", "save"]).error_kind(), "not_found");

    let mut alpha = launch(&a_exe, &[]);
    world.wait_state("Alpha runs", |st| st["active_stack"].as_array().unwrap().len() == 1);
    let mut beta = launch(&b_exe, &[]);
    world.wait_state("Beta runs", |st| st["active_stack"].as_array().unwrap().len() == 2);
    // A game that starts doesn't jump ahead of one started earlier.
    assert_eq!(stack(&world), vec![a.clone(), b.clone()]);
    assert_eq!(s(&world.ok(&["hotkey-target"])["game"]), a);

    // The active game closes: it stays the hotkeys' target, so it can be
    // loaded before it's relaunched.
    alpha.quit();
    world.wait_state("Alpha left", |st| st["active_stack"].as_array().unwrap().len() == 1);
    assert_eq!(stack(&world), vec![b.clone()]);
    assert_eq!(s(&world.ok(&["hotkey-target"])["game"]), a);
    assert_eq!(s(&world.ok(&["hotkey", "save"])["game"]), a);
    // Relaunched, it's back on top.
    let _alpha = launch(&a_exe, &[]);
    world.wait_state("Alpha runs again", |st| st["active_stack"].as_array().unwrap().len() == 2);
    assert_eq!(stack(&world), vec![a.clone(), b.clone()]);
    beta.quit();
    world.wait_state("Beta left", |st| st["active_stack"].as_array().unwrap().len() == 1);
    assert_eq!(s(&world.ok(&["hotkey-target"])["game"]), a);
}

#[test]
fn several_processes_are_one_game_until_the_last_exits() {
    let world = World::new();
    let _host = world.host();
    let ((a, exe), _) = two_games(&world);
    let mut first = launch(&exe, &[]);
    let mut second = launch(&exe, &[]);
    world.wait_game(&a, "running", |g| g["running"] == true);
    assert_eq!(stack(&world), vec![a.clone()], "one entry for several processes");
    world.ok(&["save", &a]);
    first.quit();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(world.game(&a)["running"], true, "still running while one process lives");
    second.quit();
    world.wait_game(&a, "closed", |g| g["running"] == false);
    assert_eq!(world.kinds(&a), vec!["game_closed", "saved", "game_started"], "exactly one start and one close marker");
}

#[test]
fn a_launcher_that_starts_the_game_and_exits() {
    let world = World::new();
    let _host = world.host();
    let ((a, launcher), _) = two_games(&world);
    // The real game lives somewhere the catalog doesn't name.
    let engine = world.root.join("elsewhere").join("engine.exe");
    copy_game(&engine);
    let quit_engine = world.root.join("quit-engine");
    let mut launched = launch(
        &launcher,
        &["--launch", engine.to_str().unwrap(), "--quit-file", quit_engine.to_str().unwrap(), "--", "--run-ms", "600"],
    );
    world.wait_game(&a, "running", |g| g["running"] == true);
    launched.wait_exit();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(world.game(&a)["running"], true, "the launched child keeps the game running");
    std::fs::write(&quit_engine, "").unwrap();
    world.wait_game(&a, "the child exited", |g| g["running"] == false);
}

#[test]
fn the_same_file_name_elsewhere_does_not_count() {
    let world = World::new();
    let _host = world.host();
    let ((a, _), _) = two_games(&world);
    let impostor = world.root.join("other").join("Alpha.exe");
    copy_game(&impostor);
    let _running = launch(&impostor, &[]);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(world.game(&a)["running"], false);
}

#[test]
fn a_crash_is_an_exit() {
    let world = World::new();
    let _host = world.host();
    let ((a, exe), _) = two_games(&world);
    let _crashing = launch(&exe, &["--crash-after", "2500"]);
    world.wait_game(&a, "running", |g| g["running"] == true);
    world.ok(&["save", &a]);
    world.wait_game(&a, "the crash ended the session", |g| g["running"] == false);
    assert_eq!(world.kinds(&a), vec!["game_closed", "saved", "game_started"]);
    let log = read(&world.data.join("host.log"));
    let events: Vec<&str> = log.lines().filter(|l| l.contains(&format!("Alpha ({a})"))).collect();
    assert_eq!(events.len(), 2, "{log}");
    assert!(events[0].contains("] game started: Alpha"), "{}", events[0]);
    assert!(events[1].contains("] game closed: Alpha"), "{}", events[1]);
}

#[test]
fn a_game_running_before_the_host_starts_is_on_the_stack_without_an_invented_start() {
    let world = World::new();
    let ((a, exe), _) = {
        let _host = world.host();
        two_games(&world)
    };
    let mut running = launch(&exe, &[]);
    std::thread::sleep(Duration::from_millis(300));
    let mut host = world.host();
    assert_eq!(world.game(&a)["running"], true, "found right away");
    let log = read(&world.data.join("host.log"));
    assert!(log.contains(&format!("game already running when the host started: Alpha ({a})")), "{log}");
    assert_eq!(s(&world.ok(&["hotkey-target"])["game"]), a);
    world.ok(&["hotkey", "save"]);

    // The host restarts while the game runs: the stack is rebuilt, and time
    // the host didn't observe never joins two sessions.
    host.kill();
    let _host = world.host();
    assert_eq!(world.game(&a)["running"], true);
    world.ok(&["save", &a]);
    running.quit();
    world.wait_game(&a, "closed", |g| g["running"] == false);
    assert_eq!(world.kinds(&a), vec!["game_closed", "saved", "saved"], "no invented Game started");
}

#[test]
fn hotkeys_act_on_the_selected_game_while_the_window_is_focused() {
    let world = World::new();
    let _host = world.host();
    let ((a, a_exe), (b, _)) = two_games(&world);
    let _alpha = launch(&a_exe, &[]);
    world.wait_game(&a, "running", |g| g["running"] == true);

    // The window is focused with Beta selected (even though Beta is stopped).
    world.ok(&["ui-report", "--focused", "--visible", "--selected", &b]);
    let target = world.ok(&["hotkey-target"]);
    assert_eq!(s(&target["game"]), b);
    assert_eq!(s(&target["source"]), "window");
    let saved = world.ok(&["hotkey", "save"]);
    assert_eq!(s(&saved["game"]), b);

    // The window loses focus: the top of the stack again.
    world.ok(&["ui-report", "--visible", "--selected", &b]);
    assert_eq!(s(&world.ok(&["hotkey-target"])["game"]), a);
    let saved = world.ok(&["hotkey", "save"]);
    assert_eq!(s(&saved["game"]), a);
}

#[test]
fn a_host_crash_mid_session_ends_that_session_without_an_invented_close() {
    let world = World::new();
    let mut host = world.host();
    let saves = world.home.join("Saves").join("Alpha");
    write(&saves.join("a.sav"), "tuesday");
    let (a, exe) = world.custom_game("Alpha", &saves);

    // Tuesday: play and save, then the host dies. The game is closed while
    // nothing is watching.
    let mut game = launch(&exe, &[]);
    world.wait_game(&a, "running", |g| g["running"] == true);
    let tuesday = s(&world.ok(&["save", &a, "--label", "tuesday"])["result"]["checkpoint"]);
    host.kill();
    game.quit();

    // Thursday: the host is back. The earlier run is known to have ended
    // without a clean exit, somewhere after it was last seen.
    let mut host = world.host();
    let runs = world.ok(&["host-runs"])["runs"].as_array().unwrap().clone();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs[0]["current"], true);
    assert!(runs[0]["ended_at"].is_null());
    assert_eq!(runs[1]["current"], false);
    assert!(runs[1]["ended_at"].is_null(), "a killed host has no end time");
    assert!(s(&runs[1]["last_seen_at"]) >= s(&runs[1]["started_at"]));
    assert_eq!(world.game(&a)["running"], false);

    let mut game = launch(&exe, &[]);
    world.wait_game(&a, "running again", |g| g["running"] == true);
    write(&saves.join("a.sav"), "thursday");
    world.ok(&["save", &a, "--label", "thursday"]);
    game.quit();
    world.wait_game(&a, "closed", |g| g["running"] == false);
    // Two sessions. The first has no Game closed: its exit wasn't seen.
    assert_eq!(world.kinds(&a), vec!["game_closed", "saved", "game_started", "saved", "game_started"]);

    // They really are separate: emptying Tuesday's session hides its start
    // marker and leaves Thursday's session whole.
    world.ok(&["delete", &a, &tuesday]);
    assert_eq!(world.kinds(&a), vec!["game_closed", "saved", "game_started"]);

    // A clean exit records the run's end.
    world.ok(&["shutdown"]);
    host.wait_exit(Duration::from_secs(20));
    let _host = world.host();
    let runs = world.ok(&["host-runs", "--limit", "2"])["runs"].as_array().unwrap().clone();
    assert_eq!(runs.len(), 2);
    assert!(!runs[1]["ended_at"].is_null(), "a clean exit has an end time: {runs:?}");
    assert_eq!(runs[1]["ended_at"], runs[1]["last_seen_at"]);
}
