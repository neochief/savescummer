//! macOS privacy permissions (PLAN-MACOS.md, PRIVACY PERMISSIONS): games
//! whose saves are somewhere macOS guards wait, inactive, until the user
//! allows access, and only a user action ever asks. The guarded table and
//! the user's answers come from the test environment, so these run on every
//! OS.

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use serde_json::{Value, json};

/// Marks `folder` as guarded like Documents, and says how the user answers.
fn guard(world: &World, folder: &Path, answer: &str) {
    world.set_env("privacy", json!({ "folders": [[folder, "documents"]] }));
    world.set_env("privacy_answers", json!({ "documents": answer }));
}

/// A catalog game saving under Documents, installed on Steam, with a save.
fn documents_game(world: &World, appid: u64, name: &str) -> String {
    let mut catalog = fixture_catalog();
    let games = catalog["games"].as_array_mut().unwrap();
    for (id, game) in [(9001, "Docs One"), (9002, "Docs Two")] {
        games.push(json!({
            "id": format!("steam-{id}"),
            "name": game,
            "detect": { "steam": id },
            "installDirs": [game],
            "executables": { "windows": [format!("{}.exe", game.replace(' ', ""))] },
            "save": [ { "path": format!("{{DOCUMENTS}}/{game}") } ]
        }));
    }
    world.set_catalog(&catalog);
    world.steam_install(appid, name, &format!("{}.exe", name.replace(' ', "")));
    write(&world.documents.join(name).join("slot.sav"), "progress");
    format!("steam-{appid}")
}

fn access(world: &World, game: &str) -> Value {
    world.game(game)["access"].clone()
}

#[test]
fn a_game_found_in_the_background_waits_without_asking_and_is_notified_once() {
    let world = World::new();
    let game = documents_game(&world, 9001, "Docs One");
    guard(&world, &world.documents, "granted");
    let _host = world.host();

    assert_eq!(access(&world, &game)["category"], "documents");
    assert_eq!(world.game(&game)["save"]["reason"], "access_needed");
    assert_eq!(world.cli(&["save", &game]).error_kind(), "access_needed");
    // A background scan (the window gaining focus) notifies no more.
    world.ok(&["ui-report", "--focused"]);
    wait_for("the focus scan", Duration::from_secs(10), || {
        (world.state()["scan"]["scans"].as_u64() >= Some(2)).then_some(())
    });
    let log = world.host_log();
    assert_eq!(log.matches("notification: SaveScummer needs access to Documents for 1 game").count(), 1, "{log}");
    assert!(!log.contains("asking for access"), "nothing asked in the background: {log}");

    // Allow access: the game becomes active and saves.
    assert_eq!(world.ok(&["request-access", &game])["access"], "granted");
    assert!(access(&world, &game).is_null());
    world.ok(&["save", &game]);
}

#[test]
fn a_denied_request_offers_the_settings_pane() {
    let world = World::new();
    let game = documents_game(&world, 9001, "Docs One");
    guard(&world, &world.documents, "denied");
    let _host = world.host();

    let answer = world.ok(&["request-access", &game]);
    assert_eq!(answer["access"], "denied");
    assert!(s(&answer["settings_url"]).contains("Privacy_FilesAndFolders"), "{answer}");
    assert_eq!(access(&world, &game)["denied"], true);
    assert_eq!(world.cli(&["save", &game]).error_kind(), "access_needed");
}

#[test]
fn a_user_scan_asks_and_activates_every_waiting_game() {
    let world = World::new();
    let one = documents_game(&world, 9001, "Docs One");
    world.steam_install(9002, "Docs Two", "DocsTwo.exe");
    write(&world.documents.join("Docs Two").join("slot.sav"), "progress");
    guard(&world, &world.documents, "granted");
    let _host = world.host();
    assert!(!access(&world, &one).is_null());
    assert!(!access(&world, "steam-9002").is_null());

    world.ok(&["scan"]);
    assert!(access(&world, &one).is_null());
    assert!(access(&world, "steam-9002").is_null());
    assert_eq!(world.host_log().matches("asking for access to Documents").count(), 1, "one question per category");
}

#[test]
fn adding_a_game_in_a_guarded_folder_asks_right_away() {
    let world = World::new();
    let saves = world.documents.join("Custom");
    write(&saves.join("slot.sav"), "progress");
    guard(&world, &world.documents, "denied");
    let mut host = world.host();
    let exe = world.root.join("games").join("Custom").join("Custom.exe");
    copy_game(&exe);
    let add = ["add-game", "--name", "Custom", "--exe", exe.to_str().unwrap(), "--saves", saves.to_str().unwrap()];
    assert_eq!(world.cli(&add).error_kind(), "access_needed");
    host.kill();

    guard(&world, &world.documents, "granted");
    let _host = world.host();
    let game = s(&world.ok(&add)["game"]);
    world.ok(&["save", &game]);
}

#[test]
fn a_new_build_forgets_what_the_old_one_was_allowed() {
    let world = World::new();
    let game = documents_game(&world, 9001, "Docs One");
    guard(&world, &world.documents, "granted");
    std::fs::create_dir_all(&world.data).unwrap();
    std::fs::write(
        world.data.join("privacy.json"),
        json!({ "identity": "an-older-build", "granted": ["documents"] }).to_string(),
    )
    .unwrap();
    let _host = world.host();
    assert_eq!(access(&world, &game)["category"], "documents");
    let log = world.host_log();
    assert!(log.contains("a new build: macOS forgot access to Documents"), "{log}");
    assert!(log.contains("notification: SaveScummer needs access to Documents"), "{log}");
    assert!(!log.contains("asking for access"), "{log}");
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn a_hotkey_in_front_of_a_waiting_game_fails_and_changes_nothing() {
    if !desktop_unlocked("a_hotkey_in_front_of_a_waiting_game_fails_and_changes_nothing") {
        return;
    }
    let world = World::new();
    let game = documents_game(&world, 9001, "Docs One");
    let exe = world.steam.join("steamapps").join("common").join("Docs One").join("DocsOne.exe");
    guard(&world, &world.documents, "granted");
    let _host = world.host();
    let _running = launch(&exe, &["--window"]);
    wait_for("the game in front", Duration::from_secs(20), || {
        world.host_log().contains("game started, waiting for access").then_some(())
    });
    std::thread::sleep(Duration::from_millis(800));
    let out = world.cli(&["hotkey", "save"]);
    assert_eq!(out.error_kind(), "access_needed", "{}", out.stdout);
    assert_eq!(out.last()["error"]["game"], game.as_str());
    assert!(world.state()["active_stack"].as_array().unwrap().is_empty(), "never on the stack");
    assert!(world.kinds(&game).is_empty(), "no markers, no checkpoints");
    assert!(world.host_log().contains("notification: SaveScummer needs access to Documents for Docs One"));
}

#[test]
fn a_stalled_save_is_reported_failed_and_keeps_the_game_locked() {
    let world = World::new();
    let _host = world.host_with(&["--stall-secs", "1"], &[("SAVESCUMMER_TEST_DELAY_AT", "saved.copy:1:4000")]);
    let saves = world.home.join("Saves").join("Stuck");
    write(&saves.join("slot.sav"), "progress");
    let (game, _) = world.custom_game("Stuck", &saves);

    let accepted = world.ok(&["save", &game, "--no-wait"]);
    let stalled = world.wait_game(&game, "reported stalled", |g| g["last_result"]["error"]["kind"] == "stalled");
    assert_eq!(stalled["busy"]["id"], accepted["id"], "the game stays locked");
    assert_eq!(world.cli(&["save", &game]).error_kind(), "busy");
    // The stuck work ends: the lock is freed and its real outcome recorded.
    world.wait_game(&game, "unlocked", |g| g["busy"].is_null());
    assert_eq!(world.ok(&["outcome", &s(&accepted["id"])])["status"], "succeeded");
}

// Found in review: each of these failed before its fix.

#[test]
fn a_game_whose_program_is_in_a_guarded_folder_asks_when_added() {
    let world = World::new();
    guard(&world, &world.documents, "granted");
    let _host = world.host();
    let exe = world.documents.join("Games").join("InDocs.exe");
    copy_game(&exe);
    let saves = world.home.join("Saves").join("InDocs");
    write(&saves.join("slot.sav"), "progress");
    let out = world.cli(&[
        "add-game",
        "--name",
        "InDocs",
        "--exe",
        exe.to_str().unwrap(),
        "--saves",
        saves.to_str().unwrap(),
    ]);
    assert_eq!(out.code, 0, "adding is a user action, so it asks instead of refusing: {}", out.stdout);
    assert!(world.host_log().contains("asking for access to Documents"), "{}", world.host_log());
}

#[test]
fn a_prompt_while_configuring_never_freezes_the_host() {
    let world = World::new();
    guard(&world, &world.documents, "hangs");
    let _host = world.host();
    let saves = world.home.join("Saves").join("Mover");
    write(&saves.join("slot.sav"), "progress");
    let (game, _) = world.custom_game("Mover", &saves);
    // The user moves the saves into Documents; macOS's question stays open.
    let moved = world.documents.join("Mover");
    let _asking = world.cli_background(&["configure", &game, "--saves", moved.to_str().unwrap()]);
    wait_for("the question", Duration::from_secs(10), || {
        world.host_log().contains("asking for access to Documents").then_some(())
    });
    // Everything else keeps answering meanwhile (history reads the host's
    // live records, not just the last published state).
    let child = std::process::Command::new(CLI)
        .args(["--data-dir", world.data.to_str().unwrap(), "--json", "--no-start", "history", &game])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let out = output_within(child, Duration::from_secs(10), "history while a question is open");
    assert!(out.status.success());
}

#[test]
fn a_guarded_game_found_later_in_the_first_run_is_notified() {
    let world = World::new();
    documents_game(&world, 9001, "Docs One");
    world.steam_uninstall(9001, "Docs One");
    guard(&world, &world.documents, "granted");
    // The app's first launch, by the user: nothing waits for access yet.
    let _host = world.host_launched(&[]);
    world.steam_install(9001, "Docs One", "DocsOne.exe");
    // A background scan (the window gaining focus) finds it.
    world.ok(&["ui-report", "--focused"]);
    wait_for("the focus scan", Duration::from_secs(10), || {
        (world.state()["scan"]["scans"].as_u64() >= Some(2)).then_some(())
    });
    let log = world.host_log();
    assert!(log.contains("notification: SaveScummer needs access to Documents for 1 game"), "{log}");
}

#[test]
fn a_game_added_by_the_program_inside_an_app_bundle_asks_nothing() {
    let world = World::new();
    // App bundles guard writes; a program is only ever read.
    world.set_env("privacy", json!({ "app_bundles": true }));
    world.set_env("privacy_answers", json!({ "app_bundles": "denied" }));
    let _host = world.host();
    let exe = world.root.join("Applications").join("Bundled.app").join("Contents").join("MacOS").join("Bundled");
    copy_game(&exe);
    let saves = world.home.join("Saves").join("Bundled");
    write(&saves.join("slot.sav"), "progress");
    let out = world.cli(&[
        "add-game",
        "--name",
        "Bundled",
        "--exe",
        exe.to_str().unwrap(),
        "--saves",
        saves.to_str().unwrap(),
    ]);
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert!(!world.host_log().contains("asking for access"), "{}", world.host_log());
}

#[test]
fn a_stalled_save_is_reported_while_other_file_work_goes_on() {
    let world = World::new();
    let _host = world.host_with(&["--stall-secs", "3"], &[("SAVESCUMMER_TEST_DELAY_AT", "saved.copy:1:8000")]);
    let stuck_saves = world.home.join("Saves").join("Stuck");
    write(&stuck_saves.join("slot.sav"), "progress");
    let (stuck, _) = world.custom_game("Stuck", &stuck_saves);
    // Another game runs: the host re-reads its saves every 2 s.
    let busy_saves = world.home.join("Saves").join("Busy");
    write(&busy_saves.join("slot.sav"), "progress");
    let (busy, busy_exe) = world.custom_game("Busy", &busy_saves);
    let _running = launch(&busy_exe, &[]);
    world.wait_game(&busy, "running", |g| g["running"] == true);

    world.ok(&["save", &stuck, "--no-wait"]);
    // Stuck for 8 s; reported after 3 s without progress of its own.
    wait_for("the stall reported", Duration::from_secs(7), || {
        (world.game(&stuck)["last_result"]["error"]["kind"] == "stalled").then_some(())
    });
}

#[test]
fn a_damaged_privacy_record_asks_again_only_once() {
    let world = World::new();
    documents_game(&world, 9001, "Docs One");
    guard(&world, &world.documents, "denied");
    std::fs::create_dir_all(&world.data).unwrap();
    std::fs::write(world.data.join("privacy.json"), "{ \"identity\": ").unwrap();
    for launch in 1..=2 {
        let mut host = world.host_launched(&[]);
        wait_for("the launch ready", Duration::from_secs(10), || {
            (world.host_log().matches("ready line").count() == launch).then_some(())
        });
        std::thread::sleep(Duration::from_secs(1));
        host.kill();
    }
    // Read as a first run once; answering rewrote the record.
    let log = world.host_log();
    assert_eq!(log.matches("asking for access").count(), 1, "{log}");
}

#[test]
fn allowing_access_in_scan_games_finds_what_that_scan_could_not_read() {
    let world = World::new();
    let one = documents_game(&world, 9001, "Docs One");
    let mut host = world.host();
    assert_eq!(world.game(&one)["installed"], true);
    host.kill();
    // Its Steam library turns guarded (a new build: macOS forgot access),
    // and another game was installed there meanwhile.
    world.steam_install(9002, "Docs Two", "DocsTwo.exe");
    write(&world.documents.join("Docs Two").join("slot.sav"), "progress");
    guard(&world, &world.steam, "granted");
    let _host = world.host();
    assert_eq!(access(&world, &one)["category"], "documents");
    assert!(world.game("steam-9002").is_null(), "its library can't be read yet");

    world.ok(&["scan"]);
    assert!(access(&world, &one).is_null());
    wait_for("the other game found", Duration::from_secs(10), || {
        (world.game("steam-9002")["installed"] == true).then_some(())
    });
}
