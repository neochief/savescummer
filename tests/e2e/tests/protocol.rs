//! The protocol as clients see it: accepted-or-rejected operations, request
//! ids that are safe to repeat, history pages, watching, versions, starting
//! and stopping the host.

mod common;

use std::io::BufRead;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::*;
use serde_json::Value;

fn game(world: &World, name: &str) -> (String, std::path::PathBuf) {
    let saves = world.home.join("Saves").join(name);
    write(&saves.join("slot.sav"), "v1");
    (world.custom_game(name, &saves).0, saves)
}

fn checkpoints(world: &World, game: &str) -> usize {
    world.history(game).iter().filter(|r| r["kind"] == "saved").count()
}

#[test]
fn a_repeated_request_id_returns_the_same_operation_instead_of_running_twice() {
    let world = World::new();
    let game = {
        let _host = world.host();
        game(&world, "Repeat").0
    };
    let first;
    {
        let _host = world.host();
        first = world.ok(&["--request-id", "client-42", "save", &game]);
        let again = world.ok(&["--request-id", "client-42", "save", &game]);
        assert_eq!(first["id"], again["id"]);
        assert_eq!(checkpoints(&world, &game), 1);
    }
    // Even after a restart.
    let _host = world.host();
    let again = world.ok(&["--request-id", "client-42", "save", &game]);
    assert_eq!(first["id"], again["id"]);
    assert_eq!(checkpoints(&world, &game), 1);
}

#[test]
fn a_busy_game_rejects_and_never_replays() {
    let world = World::new();
    let _host = world.host_with(&[], &[("SAVESCUMMER_TEST_DELAY_AT", "saved.copy:1:1500")]);
    let (game, _) = game(&world, "Busy");
    let (other, _) = self::game(&world, "Other");
    let running = world.ok(&["save", &game, "--no-wait"]);
    assert_eq!(running["status"], "accepted");
    let busy = world.cli(&["save", &game]);
    assert_eq!(busy.code, 3, "rejected, not queued");
    assert_eq!(busy.error_kind(), "busy");
    assert_eq!(world.game(&game)["save"]["reason"], "busy");
    // Other games stay fully usable.
    world.ok(&["save", &other]);
    world.ok(&["outcome", &s(&running["id"]), "--wait"]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(checkpoints(&world, &game), 1, "the rejected request never ran later");
}

#[test]
fn every_rejection_happens_before_anything_is_created() {
    let world = World::new();
    let _host = world.host();
    let (game, _) = game(&world, "Rejects");
    let out = world.cli(&["load", &game]);
    assert_eq!(out.code, 3);
    assert_eq!(out.error_kind(), "no_saves");
    let out = world.cli(&["revert", &game, "cp-nope"]);
    assert_eq!(out.error_kind(), "checkpoint_changed");
    let out = world.cli(&["save", "no such game"]);
    assert_eq!(out.error_kind(), "not_found");
    assert!(world.history(&game).is_empty());
    let store = world.data.join("checkpoints");
    assert!(std::fs::read_dir(&store).map(|mut d| d.next().is_none()).unwrap_or(true), "nothing was created");
}

#[test]
fn history_pages_are_stable_and_invalidated_by_relevant_changes() {
    let world = World::new();
    let mut host = world.host();
    let (game, _) = game(&world, "Pages");
    for i in 0..25 {
        world.ok(&["save", &game, "--label", &format!("save {i}")]);
    }
    let first = world.ok(&["history", &game, "--limit", "10"]);
    let rows = first["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    assert_eq!(rows[0]["label"], "save 24", "newest first");
    let cursor = s(&first["next"]);
    let second = world.ok(&["history", &game, "--limit", "10", "--cursor", &cursor]);
    assert_eq!(second["rows"][0]["label"], "save 14", "no gaps or duplicates with equal timestamps");
    // Streaming reads every page.
    assert_eq!(world.history(&game).len(), 25);

    // A label edit doesn't invalidate a position; a new row does.
    world.ok(&["label", &s(&rows[3]["checkpoint"]), "renamed"]);
    world.ok(&["history", &game, "--limit", "10", "--cursor", &cursor]);
    world.ok(&["save", &game]);
    let stale = world.cli(&["history", &game, "--limit", "10", "--cursor", &cursor]);
    assert_eq!(stale.error_kind(), "reload");

    // A host restart invalidates every position.
    let fresh = world.ok(&["history", &game, "--limit", "10"]);
    host.kill();
    let _host = world.host();
    let after = world.cli(&["history", &game, "--limit", "10", "--cursor", &s(&fresh["next"])]);
    assert_eq!(after.error_kind(), "reload");
}

#[test]
fn a_version_mismatch_is_refused_clearly() {
    let world = World::new();
    let _host = world.host();
    let out = world.cli(&["raw", r#"{"v":999,"id":"old-client","type":"state"}"#]);
    assert_eq!(out.code, 3);
    let response = out.last();
    assert_eq!(response["re"], "old-client");
    assert_eq!(response["error"]["kind"], "version_mismatch");
}

#[test]
fn an_operation_outcome_can_be_asked_for_later() {
    let world = World::new();
    let _host = world.host_with(&[], &[("SAVESCUMMER_TEST_DELAY_AT", "saved.copy:1:800")]);
    let (game, _) = game(&world, "Later");
    let accepted = world.ok(&["save", &game, "--no-wait"]);
    // The client disconnected; a new one asks by id.
    let outcome = world.ok(&["outcome", &s(&accepted["id"]), "--wait"]);
    assert_eq!(outcome["status"], "succeeded");
    assert!(outcome["result"]["checkpoint"].is_string());
}

fn watch(world: &World) -> (std::process::Child, std::sync::mpsc::Receiver<Value>) {
    let mut child = Command::new(CLI)
        .args(["--data-dir", world.data.to_str().unwrap(), "--json", "--no-start", "watch"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str(&line) {
                let _ = tx.send(v);
            }
        }
    });
    (child, rx)
}

fn next_event(rx: &std::sync::mpsc::Receiver<Value>, what: &str, check: impl Fn(&Value) -> bool) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let event = rx.recv_timeout(left).unwrap_or_else(|_| panic!("no event: {what}"));
        if check(&event) {
            return event;
        }
    }
}

#[test]
fn watching_gets_the_state_then_every_change_labels_and_the_shutdown() {
    let world = World::new();
    let mut host = world.host();
    let (game, _) = game(&world, "Watched");
    let (mut watcher, events) = watch(&world);
    let first = next_event(&events, "the full state", |e| e["event"] == "state");
    let instance = s(&first["state"]["instance"]);
    assert!(first["state"]["games"].as_array().unwrap().iter().any(|g| g["id"] == game));

    let saved = world.ok(&["save", &game]);
    next_event(&events, "the new latest checkpoint", |e| {
        e["event"] == "state"
            && e["state"]["games"]
                .as_array()
                .unwrap()
                .iter()
                .any(|g| g["latest"]["id"] == saved["result"]["checkpoint"])
    });
    world.ok(&["label", &s(&saved["result"]["checkpoint"]), "watched"]);
    let labels = next_event(&events, "labels changed", |e| e["event"] == "labels");
    assert_eq!(s(&labels["game"]), game);

    world.ok(&["shutdown"]);
    next_event(&events, "the shutdown notice", |e| e["event"] == "shutdown");
    let _ = watcher.wait();
    assert!(host.wait_exit(Duration::from_secs(20)).is_some(), "the host exits");

    // A restarted host has a new instance id: clients start over.
    let _host = world.host();
    assert_ne!(s(&world.state()["instance"]), instance);
}

#[test]
fn shutdown_lets_a_running_operation_finish_and_runs_pending_deletes() {
    let world = World::new();
    let mut host =
        world.host_with(&["--delete-countdown-ms", "60000"], &[("SAVESCUMMER_TEST_DELAY_AT", "saved.copy:1:1000")]);
    let (game, _) = game(&world, "Stopping");
    let (other, _) = self::game(&world, "Doomed");
    let doomed = world.ok(&["save", &other]);
    let pending = world.ok(&["delete", &other, &s(&doomed["result"]["checkpoint"]), "--no-wait"]);
    let running = world.ok(&["save", &game, "--no-wait"]);
    world.ok(&["shutdown"]);
    // New requests are refused while shutting down.
    let refused = world.cli(&["save", &other]);
    assert_ne!(refused.code, 0);
    assert!(host.wait_exit(Duration::from_secs(30)).is_some());

    let _host = world.host();
    assert_eq!(world.ok(&["outcome", &s(&running["id"])])["status"], "succeeded", "the save reached a safe point");
    assert_eq!(world.ok(&["outcome", &s(&pending["id"])])["status"], "succeeded", "the countdown ended early");
    assert!(world.history(&other).iter().all(|r| r["kind"] != "saved"));
}

#[test]
fn the_cli_starts_a_host_when_needed_and_never_a_second_one() {
    let world = World::new();
    let mut args = vec!["--data-dir".to_string(), world.data.to_string_lossy().into_owned(), "--json".to_string()];
    for a in world.host_args().into_iter().skip(2) {
        args.push("--host-arg".into());
        args.push(a);
    }
    let run = |extra: &[&str]| {
        let out = Command::new(CLI).args(&args).args(extra).env("SAVESCUMMER_HOST_EXE", HOST).output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
    };
    // --no-start fails when nothing runs.
    let (code, _) = run(&["--no-start", "status"]);
    assert_eq!(code, 4);
    let (code, out) = run(&["status"]);
    assert_eq!(code, 0, "{out}");
    let state: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    let instance = s(&state["instance"]);
    // A second command attaches to the same host.
    let (_, out) = run(&["status"]);
    let again: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    assert_eq!(s(&again["instance"]), instance);
    // Starting a host directly while one runs is refused.
    let second = Command::new(HOST).args(world.host_args()).output().unwrap();
    assert_eq!(second.status.code(), Some(3));
    run(&["shutdown"]);
}

#[test]
fn delete_and_flush_outcomes_survive_a_host_restart() {
    let world = World::new();
    let mut host = world.host();
    let (game, _) = game(&world, "Durable");
    let first = world.ok(&["save", &game]);
    world.ok(&["save", &game]);
    let deleted = world.ok(&["delete", &game, &s(&first["result"]["checkpoint"])]);
    let flushed = world.ok(&["flush", &game, "--yes"]);
    assert_eq!(deleted["status"], "succeeded");
    assert_eq!(flushed["status"], "succeeded");
    host.kill();
    let _host = world.host();
    assert_eq!(world.ok(&["outcome", &s(&deleted["id"])])["status"], "succeeded");
    assert_eq!(world.ok(&["outcome", &s(&flushed["id"])])["status"], "succeeded");
}

#[test]
#[cfg_attr(not(windows), ignore = "needs a process source for this OS (PLAN-MACOS.md, PROCESS MONITORING)")]
fn a_repeated_hotkey_request_returns_the_same_operation() {
    let world = World::new();
    let _host = world.host();
    let saves = world.home.join("Saves").join("Pressed");
    write(&saves.join("slot.sav"), "v1");
    let (game, exe) = world.custom_game("Pressed", &saves);
    let _running = launch(&exe, &[]);
    world.wait_game(&game, "running", |g| g["running"] == true);
    let first = world.ok(&["--request-id", "press-1", "hotkey", "save"]);
    let again = world.ok(&["--request-id", "press-1", "hotkey", "save"]);
    assert_eq!(first["id"], again["id"]);
    assert_eq!(checkpoints(&world, &game), 1, "the repeat didn't save twice");
}
