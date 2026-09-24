//! Scans: a scan stuck on a slow drive holds up nothing else, requests
//! during a scan join it or queue behind it, and the periodic scan finds
//! installs nobody reported.

mod common;

use std::time::{Duration, Instant};

use common::*;

/// Makes the scan after the startup one hang in discovery for `ms`, the way
/// a sleeping USB drive or a network share stalls reading store records.
fn stuck_scan(ms: u64) -> (&'static str, String) {
    ("SAVESCUMMER_TEST_DELAY_AT", format!("scan.discover:2:{ms}"))
}

fn user_scan_running(world: &World) {
    world.wait_state("the scan starts", |s| s["scan"]["running"] == "user");
}

#[test]
fn a_stuck_scan_delays_neither_requests_nor_monitoring() {
    let world = World::new();
    let install = world.steam_install(1001, "Rogue One", "RogueOne.exe");
    write(&install.join("saves/run.sav"), "floor 3");
    let (key, value) = stuck_scan(10_000);
    let _host = world.host_with(&[], &[(key, &value)]);

    let mut scan = world.cli_background(&["scan"]);
    user_scan_running(&world);

    // The monitor still sees the game start, and requests answer at once.
    let started = Instant::now();
    let mut game = launch(&install.join("RogueOne.exe"), &[]);
    world.wait_game("steam-1001", "the game runs", |g| g["running"] == true);
    world.ok(&["save", "steam-1001", "--label", "during the scan"]);
    write(&install.join("saves/run.sav"), "floor 4");
    world.ok(&["load", "steam-1001"]);
    assert_eq!(read(&install.join("saves/run.sav")), "floor 3");
    // The start marker was recorded (a Save in the session shows it).
    assert_eq!(world.kinds("steam-1001"), vec!["loaded", "saved", "game_started"]);
    assert!(started.elapsed() < Duration::from_secs(8), "took {:?}", started.elapsed());
    assert!(scan.is_running(), "all of that happened while the scan was stuck");
    assert_eq!(world.state()["scan"]["running"], "user");

    game.quit();
    world.wait_game("steam-1001", "closed", |g| g["running"] == false);
    let out = scan.finish();
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.last()["full"], false);
    assert!(world.state()["scan"]["running"].is_null());
}

#[test]
fn requests_during_a_scan_join_it_or_run_right_after_it() {
    let world = World::new();
    let (key, value) = stuck_scan(3_000);
    let _host = world.host_with(&[], &[(key, &value)]);
    let before = world.state()["scan"].clone();

    let install_scan = world.cli_background(&["scan"]);
    user_scan_running(&world);
    // Another install scan request joins the running one; a full scan
    // can't, so it queues behind it.
    let joined = world.cli_background(&["scan"]);
    let full = world.cli_background(&["scan", "--full"]);
    std::thread::sleep(Duration::from_millis(300));

    let first = install_scan.finish();
    let joined = joined.finish();
    let full = full.finish();
    for out in [&first, &joined, &full] {
        assert_eq!(out.code, 0, "{}", out.stderr);
    }
    let (first, joined, full) = (first.last(), joined.last(), full.last());
    assert_eq!(first["full"], false);
    assert_eq!(joined, first, "the joining request got the running scan's result");
    assert_eq!(full["full"], true, "the full scan wasn't swallowed by the install scan");
    assert!(s(&full["finished_at"]) >= s(&first["finished_at"]));

    let after = world.state()["scan"].clone();
    let count = |v: &serde_json::Value, k: &str| v[k].as_u64().unwrap();
    assert_eq!(count(&after, "scans"), count(&before, "scans") + 2, "{before} → {after}");
    assert_eq!(count(&after, "full_scans"), count(&before, "full_scans") + 1);
}

#[test]
fn the_periodic_scan_finds_an_install_nobody_reported() {
    // No folder watching and no focus reports: only the timer can notice.
    let world = World::new();
    let _host = world.host_with(&["--scan-interval-secs", "1"], &[]);
    assert!(world.game("steam-1001").is_null());
    let full_scans = world.state()["scan"]["full_scans"].as_u64().unwrap();

    let install = world.steam_install(1001, "Rogue One", "RogueOne.exe");
    write(&install.join("saves/run.sav"), "run");
    world.wait_game("steam-1001", "the periodic scan finds it", |g| g["installed"] == true);
    let scan = world.state()["scan"].clone();
    assert!(scan["full_scans"].as_u64().unwrap() > full_scans, "{scan}");
    assert!(scan["last_user"].is_null(), "periodic scans are background scans: {scan}");
}
