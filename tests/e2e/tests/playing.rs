//! Playing a game: the everyday flow of saving and loading with hotkeys while
//! the game runs, compared by actual file contents.

mod common;

use common::*;

#[test]
fn a_run_is_saved_loaded_and_reverted_while_playing() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Roguey");
    write(&saves.join("run1.sav"), "floor 3, full health");
    write(&saves.join("meta/unlocks.dat"), "3 unlocks");
    let _host = world.host();
    let (game, exe) = world.custom_game("Roguey", &saves);
    let before = tree(&saves);

    // The game starts: it goes on the ACTIVE STACK and becomes the hotkeys' target.
    let _running = launch(&exe, &[]);
    world.wait_game(&game, "the game is running", |g| g["running"] == true);
    let target = world.ok(&["hotkey-target"]);
    assert_eq!(s(&target["game"]), game);
    assert_eq!(s(&target["source"]), "active");

    // Ctrl+F5 before the boss.
    let saved = world.ok(&["hotkey", "save"]);
    assert_eq!(saved["status"], "succeeded");
    let checkpoint = s(&saved["result"]["checkpoint"]);

    // The run goes badly: the save changes and a new file appears.
    write(&saves.join("run1.sav"), "floor 5, one hit point");
    write(&saves.join("run2.sav"), "a newer run");
    write(&saves.join("meta/unlocks.dat"), "4 unlocks");
    let dying = tree(&saves);

    // Ctrl+F9: exactly as it was, the newer file removed.
    let loaded = world.ok(&["hotkey", "load"]);
    assert_eq!(loaded["status"], "succeeded", "{loaded}");
    assert_eq!(s(&loaded["result"]["checkpoint"]), checkpoint);
    assert_eq!(loaded["result"]["removed_files"], 1);
    assert_eq!(tree(&saves), before);

    let rows = world.history(&game);
    let kinds: Vec<&str> = rows.iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["loaded", "saved", "game_started"]);
    assert_eq!(rows[0]["removed_files"], 1);
    assert_eq!(rows[0]["actions"]["revert"], true);
    assert_eq!(rows[1]["actions"]["load"], true);

    // Revert brings back the state from just before the Load.
    let recovery = s(&rows[0]["checkpoint"]);
    let reverted = world.ok(&["revert", &game, &recovery]);
    assert_eq!(reverted["status"], "succeeded");
    assert_eq!(tree(&saves), dying);

    // Reverting the revert follows the same rule.
    let rows = world.history(&game);
    assert_eq!(rows[0]["kind"], "reverted");
    assert!(rows[0]["reverted_at"].is_string(), "a Reverted row says which row it reverted");
    let again = world.ok(&["revert", &game, &s(&rows[0]["checkpoint"])]);
    assert_eq!(again["status"], "succeeded");
    assert_eq!(tree(&saves), before);
    // Nothing is used up: the first Revert target is still there.
    let rows = world.history(&game);
    assert_eq!(rows.iter().filter(|r| r["actions"]["revert"] == true).count(), 3);
    // No temporary names are left next to the saves.
    assert!(tree(&saves).iter().all(|l| !l.contains(".ssnew") && !l.contains(".ssold")));
}

#[test]
fn load_this_save_restores_an_exact_older_checkpoint() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Picker");
    write(&saves.join("slot.sav"), "act 1");
    let _host = world.host();
    let (game, _) = world.custom_game("Picker", &saves);
    let first = world.ok(&["save", &game, "--label", "act 1"]);
    write(&saves.join("slot.sav"), "act 2");
    world.ok(&["save", &game, "--label", "act 2"]);
    write(&saves.join("slot.sav"), "act 3");

    // Load restores the newest save...
    world.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot.sav")), "act 2");
    // ...Load this save restores exactly the one asked for.
    world.ok(&["load", &game, "--checkpoint", &s(&first["result"]["checkpoint"])]);
    assert_eq!(read(&saves.join("slot.sav")), "act 1");
    let rows = world.history(&game);
    assert_eq!(rows[0]["kind"], "loaded");
    assert_eq!(rows[0]["label"], "act 1", "a Loaded row shows the label of the save it loaded");
}

/// What a Load refused because the game kept a save open reports: Windows
/// fails stage 2 (the rename), macOS and Linux find the open file first.
const HELD_KIND: &str = if cfg!(windows) { "in_use" } else { "held_open" };

#[test]
fn load_is_refused_while_the_game_holds_a_save_open_and_nothing_changes() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Holder");
    let slot = saves.join("slot.sav");
    write(&slot, "checkpointed");
    let other_saves = world.home.join("Saves").join("Other");
    write(&other_saves.join("a.sav"), "other game");
    let _host = world.host();
    let (game, exe) = world.custom_game("Holder", &saves);
    let (other, _) = world.custom_game("Other", &other_saves);
    world.ok(&["save", &game]);
    write(&slot, "current");
    write(&saves.join("other.sav"), "other");
    let before = tree(&saves);

    let mut running = launch(&exe, &["--hold", slot.to_str().unwrap()]);
    world.wait_game(&game, "the game is running", |g| g["running"] == true);
    let started = std::time::Instant::now();
    let load = world.cli_background(&["load", &game]);
    // While this game waits for its save, other games work as usual.
    world.ok(&["save", &other]);
    let out = load.finish();
    assert!(started.elapsed() >= std::time::Duration::from_secs(1), "the load waited before giving up");
    assert_eq!(out.code, 1, "the load ran and failed: {}", out.stdout);
    assert_eq!(out.last()["error"]["kind"], HELD_KIND);
    assert_eq!(tree(&saves), before, "nothing changed");
    assert_eq!(world.kinds(&game), vec!["saved"], "a failed load leaves no history");
    assert!(world.game(&game)["blocked"].is_null(), "an ordinary failure doesn't block the game");

    // Close the game; the same command again is the retry.
    running.quit();
    world.wait_game(&game, "the game closed", |g| g["running"] == false);
    world.ok(&["load", &game]);
    assert_eq!(read(&slot), "checkpointed");
}

#[test]
fn a_save_the_game_lets_go_of_within_a_moment_is_waited_out() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Brief");
    let slot = saves.join("slot.sav");
    write(&slot, "checkpointed");
    let _host = world.host();
    let (game, exe) = world.custom_game("Brief", &saves);
    world.ok(&["save", &game]);
    write(&slot, "current");

    // Outside the save folder, so it isn't part of the save.
    let release = world.home.join("release-the-save");
    let _running = launch(&exe, &["--hold", slot.to_str().unwrap(), "--release-file", release.to_str().unwrap()]);
    world.wait_game(&game, "the game is running", |g| g["running"] == true);
    let load = world.cli_background(&["load", &game]);
    std::thread::sleep(std::time::Duration::from_millis(300));
    write(&release, "");
    let out = load.finish();
    assert_eq!(out.code, 0, "the load waited the hold out: {}", out.stdout);
    assert_eq!(read(&slot), "checkpointed");
}

#[test]
fn a_session_without_saves_leaves_no_markers() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Quiet");
    write(&saves.join("a.sav"), "a");
    let _host = world.host();
    let (game, exe) = world.custom_game("Quiet", &saves);

    let mut quiet = launch(&exe, &[]);
    world.wait_game(&game, "running", |g| g["running"] == true);
    quiet.quit();
    world.wait_game(&game, "closed", |g| g["running"] == false);
    assert!(world.kinds(&game).is_empty(), "an empty session disappears");

    let mut played = launch(&exe, &[]);
    world.wait_game(&game, "running", |g| g["running"] == true);
    world.ok(&["save", &game]);
    played.quit();
    world.wait_game(&game, "closed", |g| g["running"] == false);
    assert_eq!(world.kinds(&game), vec!["game_closed", "saved", "game_started"]);
    assert!(world.game(&game)["has_history"] == true);
}

#[test]
fn saving_needs_game_data() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Fresh");
    let _host = world.host();
    let (game, _) = world.custom_game("Fresh", &saves);
    let summary = world.game(&game);
    assert_eq!(summary["save"]["available"], false);
    assert_eq!(summary["save"]["reason"], "no_game_data");
    assert_eq!(summary["load"]["reason"], "no_saves");
    let out = world.cli(&["save", &game]);
    assert_eq!(out.code, 3, "rejected before anything is created");
    assert_eq!(out.error_kind(), "no_game_data");
    assert!(!world.data.join("checkpoints").read_dir().map(|mut d| d.next().is_some()).unwrap_or(false));

    // The game writes its first save: Save becomes available.
    write(&saves.join("first.sav"), "hello");
    world.ok(&["scan"]);
    assert_eq!(world.game(&game)["save"]["available"], true);
}
