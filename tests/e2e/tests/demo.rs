//! `--demo`: the real host on a generated machine, playing real catalog
//! games through scripted processes. It never touches a folder it didn't
//! make.

mod common;

use std::process::{Command, Stdio};
use std::time::Duration;

use common::*;
use serde_json::Value;

fn demo_cli(data: &std::path::Path, args: &[&str]) -> Value {
    let child = Command::new(CLI)
        .arg("--data-dir")
        .arg(data)
        .args(["--json", "--no-start"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = output_within(child, CLI_LIMIT, "the demo CLI");
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.lines().last().unwrap()).unwrap()
}

#[test]
fn the_demo_plays_real_catalog_games_on_a_simulated_machine() {
    let world = World::new();
    let demo = world.root.join("demo");
    let _host = world.host_with(&["--demo", "--data-dir", demo.to_str().unwrap()], &[]);
    let data = demo.join("data");

    // Real catalog games with instructions, each given a short history.
    let state = wait_for("the demo's games have history", Duration::from_secs(30), || {
        let state = demo_cli(&data, &["status"]);
        let games = state["games"].as_array().cloned().unwrap_or_default();
        (games.len() >= 4 && games.iter().all(|g| g["has_history"] == true)).then_some(state)
    });
    let games = state["games"].as_array().unwrap();
    assert!(games.iter().all(|g| g["catalog_id"].is_string() && g["kind"] == "known"), "{state}");

    // Then it plays: a game runs, saves and loads through the real host.
    let played = wait_for("a demo game runs", Duration::from_secs(30), || {
        let state = demo_cli(&data, &["status"]);
        state["games"].as_array().unwrap().iter().find(|g| g["running"] == true).map(|g| g["id"].clone())
    });
    let game = played.as_str().unwrap().to_string();
    wait_for("the running game loads", Duration::from_secs(40), || {
        let child = Command::new(CLI)
            .arg("--data-dir")
            .arg(&data)
            .args(["--json", "--no-start", "history", &game, "--all"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let out = output_within(child, CLI_LIMIT, "the demo history");
        String::from_utf8_lossy(&out.stdout).contains("\"loaded\"").then_some(())
    });
    // Nothing of the simulated machine leaks out of the demo folder.
    assert!(!world.data.exists(), "the world's own data folder is untouched");
}

#[test]
fn the_demo_refuses_a_folder_it_didnt_make() {
    let world = World::new();
    let real = world.root.join("real-data");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("host.db"), "precious").unwrap();
    let out = Command::new(HOST)
        .args(world.host_args())
        .args(["--demo", "--data-dir", real.to_str().unwrap()])
        .output()
        .unwrap();
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("\"ready\":false") && line.contains("--demo"), "{line}");
    assert_eq!(std::fs::read_to_string(real.join("host.db")).unwrap(), "precious");
}
