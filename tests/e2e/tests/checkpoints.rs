//! Managing checkpoints: labels, deleting with a countdown, Flush, backups
//! changed outside the app, moving the store, and the checkpoint size.

mod common;

use std::time::Duration;

use common::*;

fn game_with_saves(world: &World, name: &str) -> (String, std::path::PathBuf) {
    let saves = world.home.join("Saves").join(name);
    write(&saves.join("slot.sav"), "v1");
    let (id, _) = world.custom_game(name, &saves);
    (id, saves)
}

#[test]
fn labels_name_saves_everywhere_and_outlive_deletion() {
    let world = World::new();
    let _host = world.host();
    let (game, saves) = game_with_saves(&world, "Labeled");
    let saved = world.ok(&["save", &game, "--label", "  Before\nboss  "]);
    let checkpoint = s(&saved["result"]["checkpoint"]);
    assert_eq!(world.game(&game)["latest"]["label"], "Before boss", "trimmed, one line");

    // Renaming updates the Saved row, the Loaded rows and the latest summary.
    write(&saves.join("slot.sav"), "v2");
    world.ok(&["load", &game]);
    world.ok(&["label", &checkpoint, "Boss: phase 2"]);
    let rows = world.history(&game);
    assert_eq!(rows[0]["kind"], "loaded");
    assert_eq!(rows[0]["label"], "Boss: phase 2");
    assert_eq!(rows[1]["label"], "Boss: phase 2");
    assert_eq!(world.game(&game)["latest"]["label"], "Boss: phase 2");

    // At most 100 characters.
    let long = "x".repeat(150);
    let set = world.ok(&["label", &checkpoint, &long]);
    assert_eq!(s(&set["label"]).len(), 100);
    // Clearing.
    world.ok(&["label", &checkpoint, "--clear"]);
    assert!(world.game(&game)["latest"]["label"].is_null());
    world.ok(&["label", &checkpoint, "Before boss"]);

    // Labels live only in the database: folder names and contents unchanged.
    let folder = world.ok(&["open", "checkpoint", "--checkpoint", &checkpoint, "--resolve-only"]);
    assert!(!s(&folder["path"]).contains("Before boss"));

    // Deleted: the Loaded row still says what it loaded.
    world.ok(&["delete", &game, &checkpoint]);
    let rows = world.history(&game);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "loaded");
    assert_eq!(rows[0]["label"], "Before boss");
    // A label for a gone checkpoint is rejected quietly as "gone".
    assert_eq!(world.cli(&["label", &checkpoint, "late"]).error_kind(), "gone");
}

#[test]
fn a_label_can_be_set_while_the_game_is_busy() {
    let world = World::new();
    let _host = world.host_with(&[], &[("SAVESCUMMER_TEST_DELAY_AT", "saved.copy:1:2000")]);
    let (game, _) = game_with_saves(&world, "Busy");
    let first = world.ok(&["save", &game]);
    let accepted = world.ok(&["save", &game, "--no-wait"]);
    assert_eq!(world.game(&game)["busy"]["id"], accepted["id"]);
    world.ok(&["label", &s(&first["result"]["checkpoint"]), "while busy"]);
    world.ok(&["outcome", &s(&accepted["id"]), "--wait"]);
}

#[test]
fn a_delete_counts_down_and_can_be_cancelled() {
    let world = World::new();
    let _host = world.host();
    let (game, _) = game_with_saves(&world, "Deleter");
    let saved = world.ok(&["save", &game]);
    let checkpoint = s(&saved["result"]["checkpoint"]);

    let pending = world.ok(&["delete", &game, &checkpoint, "--no-wait"]);
    assert_eq!(pending["status"], "counting_down");
    let state = world.state();
    assert_eq!(state["deletes"].as_array().unwrap().len(), 1, "the countdown is in the state");
    assert!(state["deletes"][0]["remaining_ms"].as_u64().unwrap() > 0);
    let rows = world.history(&game);
    assert_eq!(rows[0]["deleting"], true);
    // The same row can't be requested twice.
    assert_eq!(world.cli(&["delete", &game, &checkpoint, "--no-wait"]).error_kind(), "invalid_request");
    // Load still targets the checkpoint while it counts down.
    world.ok(&["load", &game]);

    let cancelled = world.ok(&["cancel-delete", &s(&pending["id"])]);
    assert_eq!(cancelled["status"], "cancelled");
    std::thread::sleep(Duration::from_millis(2000));
    assert_eq!(world.history(&game).iter().filter(|r| r["kind"] == "saved").count(), 1, "nothing was deleted");

    // Run to the end: the CLI waits for the files to be gone, printing each state.
    let out = world.cli(&["delete", &game, &checkpoint]);
    assert_eq!(out.code, 0);
    let statuses: Vec<String> = out.lines.iter().map(|l| s(&l["status"])).collect();
    assert_eq!(statuses.first().unwrap(), "counting_down");
    assert_eq!(statuses.last().unwrap(), "succeeded");
    assert!(world.history(&game).iter().all(|r| r["kind"] != "saved"));
    // A cancel that arrives too late gets the real state back.
    let late = world.ok(&["cancel-delete", &s(&out.last()["id"])]);
    assert_eq!(late["status"], "succeeded");
}

#[test]
fn a_delete_waits_for_the_game_to_be_free() {
    let world = World::new();
    let _host = world.host_with(&[], &[("SAVESCUMMER_TEST_DELAY_AT", "saved.copy:1:3000")]);
    let (game, _) = game_with_saves(&world, "Waiter");
    let saved = world.ok(&["save", &game]);
    let pending = world.ok(&["delete", &game, &s(&saved["result"]["checkpoint"]), "--no-wait"]);
    // A countdown doesn't reserve the game: a Save still starts…
    let save = world.ok(&["save", &game, "--no-wait"]);
    assert_eq!(save["status"], "accepted");
    // …and the deletion waits for its turn.
    wait_for("the delete is waiting", Duration::from_secs(10), || {
        let op = world.ok(&["outcome", &s(&pending["id"])]);
        (op["status"] == "waiting").then_some(())
    });
    let done = world.ok(&["outcome", &s(&pending["id"]), "--wait"]);
    assert_eq!(done["status"], "succeeded");
    assert_eq!(world.ok(&["outcome", &s(&save["id"])])["status"], "succeeded");
}

#[test]
fn flush_deletes_every_checkpoint_and_the_history_but_never_the_saves() {
    let world = World::new();
    let _host = world.host();
    let (game, saves) = game_with_saves(&world, "Flusher");
    world.ok(&["save", &game, "--label", "one"]);
    write(&saves.join("slot.sav"), "v2");
    world.ok(&["save", &game]);
    world.ok(&["load", &game]);
    let live = tree(&saves);

    let preview = world.ok(&["flush", &game, "--preview"]);
    assert_eq!(preview["saved"], 2);
    assert_eq!(preview["recovery"], 1);
    assert!(preview["size"].as_u64().unwrap() > 0);
    assert!(preview["items"].as_array().unwrap().iter().any(|i| i["label"] == "one"));
    let size = world.game(&game)["checkpoints_size"].as_u64().unwrap();
    assert_eq!(size, preview["size"].as_u64().unwrap(), "the state carries what a Flush would delete");

    // Without --yes nothing happens.
    world.ok(&["flush", &game]);
    assert_eq!(world.history(&game).len(), 3);

    let flushed = world.ok(&["flush", &game, "--yes"]);
    assert_eq!(flushed["result"]["count"], 3);
    assert!(world.history(&game).is_empty());
    let summary = world.game(&game);
    assert!(summary["latest"].is_null());
    assert_eq!(summary["has_history"], false);
    assert_eq!(summary["checkpoints_size"], 0);
    assert_eq!(tree(&saves), live, "the game's own saves are not touched");
}

#[test]
fn backups_changed_outside_the_app_are_noticed() {
    let world = World::new();
    let _host = world.host();
    let (game, saves) = game_with_saves(&world, "Edited");
    let first = world.ok(&["save", &game, "--label", "original"]);
    write(&saves.join("slot.sav"), "v2");
    let second = world.ok(&["save", &game]);
    let folder = |id: &str| {
        std::path::PathBuf::from(s(&world.ok(&["open", "checkpoint", "--checkpoint", id, "--resolve-only"])["path"]))
    };

    // Edited in place: before any restore the host checks again and refuses.
    let edited = folder(&s(&first["result"]["checkpoint"]));
    write(&edited.join("Edited").join("Edited").join("slot.sav"), "tampered");
    let out = world.cli(&["load", &game, "--checkpoint", &s(&first["result"]["checkpoint"])]);
    assert_eq!(out.error_kind(), "checkpoint_changed");
    assert_eq!(read(&saves.join("slot.sav")), "v2", "nothing was touched");

    // Deleted outside the app.
    std::fs::remove_dir_all(folder(&s(&second["result"]["checkpoint"]))).unwrap();

    // A full scan retires both, and registers the edited folder as a new,
    // unlabeled checkpoint.
    world.ok(&["scan", "--full"]);
    let rows = world.history(&game);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["kind"], "saved");
    assert!(rows[0]["label"].is_null(), "a new generation starts unlabeled");
    assert_ne!(rows[0]["checkpoint"], first["result"]["checkpoint"]);
    world.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot.sav")), "tampered");

    // Folders copied into the store by hand are ignored.
    let store =
        std::path::PathBuf::from(s(&world.ok(&["open", "checkpoints", "--game", &game, "--resolve-only"])["path"]));
    write(&store.join("my manual copy").join("slot.sav"), "manual");
    world.ok(&["scan", "--full"]);
    assert_eq!(world.history(&game).iter().filter(|r| r["kind"] == "saved").count(), 1);
}

#[test]
fn the_store_moves_and_an_unreachable_store_makes_operations_unavailable() {
    let world = World::new();
    let _host = world.host();
    let (game, saves) = game_with_saves(&world, "Mover");
    world.ok(&["save", &game, "--label", "moved along"]);
    let new_store = world.root.join("BiggerDrive").join("Checkpoints");
    let moved = world.ok(&["move-store", new_store.to_str().unwrap()]);
    assert_eq!(moved["status"], "succeeded", "{moved}");
    let state = world.state();
    assert_eq!(s(&state["store"]["path"]), new_store.to_string_lossy());
    assert!(!world.data.join("checkpoints").exists(), "the old copies are deleted after the switch");
    write(&saves.join("slot.sav"), "v2");
    world.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot.sav")), "v1");
    assert_eq!(world.history(&game).last().unwrap()["label"], "moved along");

    // The store's drive goes away: every game's operations are unavailable,
    // checkpoints are kept, not retired.
    let parked = world.root.join("parked");
    std::fs::rename(world.root.join("BiggerDrive"), &parked).unwrap();
    world.ok(&["scan", "--full"]);
    let summary = world.game(&game);
    assert_eq!(summary["save"]["reason"], "store_unavailable");
    assert!(summary["checkpoints_size"].is_null(), "unknown while the store is unavailable");
    assert_eq!(world.cli(&["save", &game]).error_kind(), "store_unavailable");
    let rows = world.history(&game);
    assert_eq!(rows.len(), 2, "rows stay, with actions disabled");
    assert_eq!(rows[0]["actions"]["revert"], false);
    assert_eq!(rows.last().unwrap()["unavailable"], "store_unavailable");

    // It comes back: everything works as before.
    std::fs::rename(&parked, world.root.join("BiggerDrive")).unwrap();
    world.ok(&["scan", "--full"]);
    assert_eq!(world.game(&game)["save"]["available"], true);
    assert!(world.history(&game).last().unwrap()["unavailable"].is_null());
}

#[test]
fn a_move_interrupted_before_the_switch_changes_nothing() {
    let world = World::new();
    let game = {
        let _host = world.host();
        let (game, _) = game_with_saves(&world, "Interrupted");
        world.ok(&["save", &game]);
        game
    };
    let new_store = world.root.join("NewStore");
    let mut host = world.host_with(&[], &[("SAVESCUMMER_TEST_CRASH_AT", "move.copied:1")]);
    let _ = world.cli(&["move-store", new_store.to_str().unwrap()]);
    assert_eq!(host.wait_exit(Duration::from_secs(20)), Some(86));
    let _host = world.host();
    assert!(!new_store.exists(), "the partial copy was cleaned up");
    assert_eq!(s(&world.state()["store"]["path"]), world.data.join("checkpoints").to_string_lossy());
    assert_eq!(world.history(&game).len(), 1);
    world.ok(&["load", &game]);
}

#[test]
fn opening_folders_resolves_real_paths_and_missing_folders_open_their_parent() {
    let world = World::new();
    let _host = world.host();
    let saves = world.home.join("Saves").join("Opener").join("slots");
    let (game, exe) = world.custom_game("Opener", &saves.join("*.sav"));
    let root = world.ok(&["open", "saves", "--game", &game, "--resolve-only"]);
    assert_eq!(s(&root["path"]), world.home.to_string_lossy(), "the nearest existing parent");
    assert_eq!(root["opened"], false);
    std::fs::create_dir_all(&saves).unwrap();
    let root = world.ok(&["open", "saves", "--game", &game, "--resolve-only"]);
    assert_eq!(s(&root["path"]), saves.to_string_lossy(), "a pattern's root is the folder before the wildcard");
    let exe_dir = world.ok(&["open", "executable", "--game", &game, "--resolve-only"]);
    assert_eq!(s(&exe_dir["path"]), exe.parent().unwrap().to_string_lossy());
    let store = world.ok(&["open", "checkpoints", "--game", &game, "--resolve-only"]);
    assert!(s(&store["path"]).starts_with(&*world.data.to_string_lossy()));
}
