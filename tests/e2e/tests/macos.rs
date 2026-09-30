//! A macOS machine (PLAN-MACOS.md, TESTS): games as `.app` bundles, saves
//! under `~/Library/Application Support`, and the frontmost app as the top of
//! the ACTIVE STACK. The other suites keep their Windows-shaped world, which
//! exercises the rules the same way here.
#![cfg(target_os = "macos")]

mod common;

use std::time::Duration;

use common::*;
use serde_json::json;

fn top(world: &World) -> String {
    s(&world.state()["active_stack"][0])
}

#[test]
fn a_custom_game_given_as_its_app_bundle_runs_as_the_executable_inside() {
    let world = World::new();
    let _host = world.host();
    let app = world.root.join("Applications").join("Bundled Game.app");
    let exe = world.app_bundle(&app);
    let saves = world.home.join("Library").join("Application Support").join("Bundled Game");
    write(&saves.join("slot.sav"), "v1");
    let added = world.ok(&[
        "add-game",
        "--name",
        "Bundled",
        "--exe",
        app.to_str().unwrap(),
        "--saves",
        saves.to_str().unwrap(),
    ]);
    let game = s(&added["game"]);

    let mut running = launch(&exe, &[]);
    world.wait_game(&game, "running", |g| g["running"] == true);
    running.quit();
    world.wait_game(&game, "closed", |g| g["running"] == false);
}

#[test]
fn the_frontmost_app_is_the_top_of_the_stack() {
    let Some(_screen) = desktop("the_frontmost_app_is_the_top_of_the_stack") else { return };
    let world = World::new();
    let _host = world.host();
    let mut games = Vec::new();
    for name in ["Front A", "Front B"] {
        let app = world.root.join("Applications").join(format!("{name}.app"));
        let exe = world.app_bundle(&app);
        let saves = world.home.join("Saves").join(name);
        write(&saves.join("slot.sav"), "v1");
        let added =
            world.ok(&["add-game", "--name", name, "--exe", app.to_str().unwrap(), "--saves", saves.to_str().unwrap()]);
        games.push((s(&added["game"]), exe, world.root.join(format!("activate-{name}"))));
    }
    let [(a, exe_a, activate_a), (b, exe_b, _)] = &games[..] else { unreachable!() };

    let _running_a = launch(exe_a, &["--window", "--activate-file", activate_a.to_str().unwrap()]);
    wait_for("A in front", Duration::from_secs(20), || (top(&world) == *a).then_some(()));
    let _running_b = launch(exe_b, &["--window"]);
    wait_for("B in front", Duration::from_secs(20), || (top(&world) == *b).then_some(()));
    // ⌘-Tab back to A.
    std::fs::write(activate_a, "").unwrap();
    wait_for("A in front again", Duration::from_secs(20), || (top(&world) == *a).then_some(()));
}

/// The fixture catalog's game as a Mac port: a bundle in its Steam folder,
/// saves in Application Support.
fn mac_catalog(world: &World) {
    let mut catalog = fixture_catalog();
    catalog["games"].as_array_mut().unwrap().push(json!({
        "id": "steam-3001",
        "name": "Mac Port",
        "detect": { "steam": 3001 },
        "installDirs": ["Mac Port"],
        "executables": { "macos": ["Mac Port.app"] },
        "save": [ { "path": "{HOME}/Library/Application Support/MacPort" } ]
    }));
    world.set_catalog(&catalog);
    world.set_env("platform", json!("macos"));
}

#[test]
fn a_mac_port_is_found_played_saved_and_loaded() {
    let world = World::new();
    mac_catalog(&world);
    let install = world.steam.join("steamapps").join("common").join("Mac Port");
    let exe = world.app_bundle(&install.join("Mac Port.app"));
    world.steam_manifest(3001, "Mac Port", 4);
    let saves = world.home.join("Library").join("Application Support").join("MacPort");
    write(&saves.join("profile.sav"), "before");
    let _host = world.host();
    let game = "steam-3001";
    world.enable_expert_mode(game);
    assert_eq!(world.game(game)["save"]["available"], true, "{}", world.game(game));

    let running = launch(&exe, &[]);
    world.wait_game(game, "running", |g| g["running"] == true);
    world.ok(&["save", game]);
    write(&saves.join("profile.sav"), "after");
    world.ok(&["load", game]);
    assert_eq!(read(&saves.join("profile.sav")), "before");
    drop(running);
    world.wait_game(game, "closed", |g| g["running"] == false);
}

#[test]
fn quitting_the_game_in_front_keeps_it_the_target_while_macos_brings_another_forward() {
    let Some(_screen) = desktop("quitting_the_game_in_front_keeps_it_the_target_while_macos_brings_another_forward")
    else {
        return;
    };
    let world = World::new();
    let _host = world.host();
    let mut games = Vec::new();
    for name in ["Quit A", "Quit B"] {
        let app = world.root.join("Applications").join(format!("{name}.app"));
        let exe = world.app_bundle(&app);
        let saves = world.home.join("Saves").join(name);
        write(&saves.join("slot.sav"), "v1");
        let added =
            world.ok(&["add-game", "--name", name, "--exe", app.to_str().unwrap(), "--saves", saves.to_str().unwrap()]);
        games.push((s(&added["game"]), exe, app, world.root.join(format!("activate-{name}"))));
    }
    let [(a, exe_a, _, activate_a), (b, exe_b, app_b, _)] = &games[..] else { unreachable!() };

    // Into the Breach open, then the user ⌘-Tabs to FTL and quits it.
    let mut running_a = launch(exe_a, &["--window", "--activate-file", activate_a.to_str().unwrap()]);
    wait_for("A in front", Duration::from_secs(20), || (top(&world) == *a).then_some(()));
    let _running_b = launch(exe_b, &["--window"]);
    wait_for("B in front", Duration::from_secs(20), || (top(&world) == *b).then_some(()));
    std::fs::write(activate_a, "").unwrap();
    wait_for("A in front again", Duration::from_secs(20), || (top(&world) == *a).then_some(()));
    running_a.quit();
    world.wait_state("A closed", |st| st["active_stack"].as_array().unwrap().len() == 1);
    // macOS brings the app used before forward on its own: here, B. (Which
    // app that is depends on the session's history, and a click elsewhere
    // on a machine in use moves it, so the test keeps B in front.)
    let bring_b = || assert!(std::process::Command::new("open").arg(app_b).status().unwrap().success());
    bring_b();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(s(&world.ok(&["hotkey-target"])["game"]), *a, "the user didn't switch to B");
    // Still in front 5 s after the quit: B is the game being played.
    wait_for("B takes over", Duration::from_secs(10), || {
        bring_b();
        (s(&world.ok(&["hotkey-target"])["game"]) == *b).then_some(())
    });
}
