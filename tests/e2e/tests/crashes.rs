//! Kills at every recorded step: the host dies between (and within) the
//! stages of a Save or a Load, then starts again on the same folders and
//! database. Each case is checked against the four recovery rules by both
//! the files and the records.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;

struct Setup {
    world: World,
    game: String,
    saves: PathBuf,
    /// The live state before the interrupted Load.
    before: Vec<String>,
    /// The checkpoint's state.
    checkpoint: Vec<String>,
}

/// A game with one checkpoint (a, b) and a changed live state (a changed,
/// c new), so a Load replaces, keeps and deletes files.
fn setup() -> Setup {
    let world = World::new();
    let saves = world.home.join("Saves").join("Crashy");
    write(&saves.join("a.sav"), "a1");
    write(&saves.join("b.sav"), "b1");
    let (game, checkpoint) = {
        let _host = world.host();
        let (game, _) = world.custom_game("Crashy", &saves);
        world.ok(&["save", &game]);
        (game, tree(&saves))
    };
    write(&saves.join("a.sav"), "a2");
    write(&saves.join("c.sav"), "c2");
    let before = tree(&saves);
    Setup { world, game, saves, before, checkpoint }
}

/// Runs a Load on a host that crashes at `point`, then restarts the host.
fn crash_during_load(setup: &Setup, point: &str) -> HostProcess {
    let mut host = setup.world.host_with(&[], &[("SAVESCUMMER_TEST_CRASH_AT", point)]);
    let out = setup.world.cli(&["load", &setup.game]);
    assert_ne!(out.code, 0, "the host died mid-load at {point}");
    assert_eq!(host.wait_exit(Duration::from_secs(20)), Some(86), "the host crashed at {point}");
    setup.world.host()
}

fn leftovers(setup: &Setup) -> Vec<String> {
    tree(&setup.saves).into_iter().filter(|l| l.contains(".ssnew") || l.contains(".ssold")).collect()
}

#[test]
fn a_kill_before_anything_live_changed_is_rolled_back_silently() {
    // Rule 1: while copying the recovery checkpoint, or during stage 1.
    for point in ["recovery.copy:1", "load.recovery:1", "load.copy_in:1"] {
        let setup = setup();
        let _host = crash_during_load(&setup, point);
        assert_eq!(tree(&setup.saves), setup.before, "{point}: live saves unchanged");
        assert!(leftovers(&setup).is_empty(), "{point}: our copies are gone");
        assert_eq!(setup.world.kinds(&setup.game), vec!["saved"], "{point}: no Loaded row");
        let game = setup.world.game(&setup.game);
        assert!(game["blocked"].is_null(), "{point}: the game is released");
        assert!(game["notice"].is_null(), "{point}: an interrupted load is silent");
        // And it works again.
        setup.world.ok(&["load", &setup.game]);
        assert_eq!(tree(&setup.saves), setup.checkpoint);
    }
}

#[test]
fn a_kill_in_stage_two_or_three_is_undone_by_reversing_renames() {
    // Rule 2: some files set aside, or some swapped in.
    for point in ["load.set_aside:1", "load.set_aside:2", "load.set_aside:3", "load.swap_in:1"] {
        let setup = setup();
        let _host = crash_during_load(&setup, point);
        assert_eq!(tree(&setup.saves), setup.before, "{point}: the Load never happened");
        assert!(leftovers(&setup).is_empty(), "{point}");
        assert_eq!(setup.world.kinds(&setup.game), vec!["saved"], "{point}");
        assert!(setup.world.game(&setup.game)["blocked"].is_null());
    }
}

#[test]
fn a_kill_after_every_file_was_swapped_in_is_finished_and_recorded() {
    // Rule 3: the result is in place but wasn't recorded (or cleaned up).
    for point in ["load.swap_in:2", "load.applied:1", "load.clean_up:1"] {
        let setup = setup();
        let _host = crash_during_load(&setup, point);
        assert_eq!(tree(&setup.saves), setup.checkpoint, "{point}: the Load is in place");
        assert!(leftovers(&setup).is_empty(), "{point}: set-aside files are removed");
        let rows = setup.world.history(&setup.game);
        assert_eq!(rows[0]["kind"], "loaded", "{point}: recorded as if it had completed");
        assert_eq!(rows[0]["removed_files"], 1, "{point}");
        // The recovery checkpoint holds the state from before the Load.
        let recovery = s(&rows[0]["checkpoint"]);
        setup.world.ok(&["revert", &setup.game, &recovery]);
        assert_eq!(tree(&setup.saves), setup.before, "{point}: Revert brings it back");
    }
}

#[test]
fn a_save_interrupted_while_copying_leaves_no_checkpoint_and_a_notice() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Saver");
    write(&saves.join("a.sav"), "a");
    write(&saves.join("b.sav"), "b");
    let game = {
        let _host = world.host();
        world.custom_game("Saver", &saves).0
    };
    let mut host = world.host_with(&[], &[("SAVESCUMMER_TEST_CRASH_AT", "saved.copy:1")]);
    let _ = world.cli(&["save", &game]);
    assert_eq!(host.wait_exit(Duration::from_secs(20)), Some(86));
    let _host = world.host();
    let summary = world.game(&game);
    assert_eq!(summary["notice"], "save_interrupted");
    assert!(summary["latest"].is_null(), "a half copy never becomes a checkpoint");
    assert!(world.kinds(&game).is_empty());
    let folder = world.ok(&["open", "checkpoints", "--game", &game, "--resolve-only"]);
    let leftovers: Vec<_> = std::fs::read_dir(s(&folder["path"])).map(|d| d.flatten().collect()).unwrap_or_default();
    assert!(leftovers.is_empty(), "the temporary folder was removed: {leftovers:?}");
    // The next action clears the notice.
    world.ok(&["save", &game]);
    assert!(world.game(&game)["notice"].is_null());
}

#[test]
fn a_save_published_but_not_recorded_is_recorded_at_the_next_start() {
    let world = World::new();
    let saves = world.home.join("Saves").join("Saver");
    write(&saves.join("a.sav"), "a");
    let game = {
        let _host = world.host();
        world.custom_game("Saver", &saves).0
    };
    let mut host = world.host_with(&[], &[("SAVESCUMMER_TEST_CRASH_AT", "saved.published:1")]);
    let _ = world.cli(&["save", &game, "--label", "kept anyway"]);
    assert_eq!(host.wait_exit(Duration::from_secs(20)), Some(86));
    let _host = world.host();
    let rows = world.history(&game);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "saved");
    assert_eq!(rows[0]["label"], "kept anyway");
    assert!(world.game(&game)["notice"].is_null());
    write(&saves.join("a.sav"), "changed");
    world.ok(&["load", &game]);
    assert_eq!(read(&saves.join("a.sav")), "a");
}

#[test]
fn an_unverifiable_interruption_keeps_everything_and_blocks_until_coherent() {
    let setup = setup();
    {
        let mut host = setup.world.host_with(&[], &[("SAVESCUMMER_TEST_CRASH_AT", "load.set_aside:1")]);
        let _ = setup.world.cli(&["load", &setup.game]);
        assert_eq!(host.wait_exit(Duration::from_secs(20)), Some(86));
    }
    // a.sav was set aside when the host died. Something removes the
    // set-aside file: the state no longer matches the journal.
    let old = setup.saves.join("a.sav.ssold");
    assert_eq!(read(&old), "a2");
    std::fs::remove_file(&old).unwrap();
    let _host = setup.world.host();
    let game = setup.world.game(&setup.game);
    assert_eq!(game["blocked"]["kind"], "rollback_failed", "{game}");
    assert_eq!(game["save"]["reason"], "blocked");
    let out = setup.world.cli(&["save", &setup.game]);
    assert_eq!(out.code, 3);
    assert_eq!(out.error_kind(), "blocked");
    // Nothing kept was deleted by the scan.
    assert!(setup.saves.join("a.sav.ssnew").exists());
    // Retry runs the same rules again: still incoherent, still blocked.
    assert_eq!(setup.world.cli(&["retry", &setup.game]).code, 3);
    // The user puts a save back under the name; now every name has a live
    // file, so Retry releases the game and keeps the material.
    write(&setup.saves.join("a.sav"), "put back by hand");
    setup.world.ok(&["retry", &setup.game]);
    let game = setup.world.game(&setup.game);
    assert!(game["blocked"].is_null());
    assert_eq!(read(&setup.saves.join("a.sav")), "put back by hand", "live files are left exactly as they are");
    assert!(setup.saves.join("a.sav.ssnew").exists(), "kept material stays");
    assert_eq!(setup.world.kinds(&setup.game), vec!["saved"], "no history for a failed Load");
    setup.world.ok(&["scan", "--full"]);
    assert!(setup.saves.join("a.sav.ssnew").exists(), "a full scan never cleans kept material");
}

#[test]
fn other_games_stay_usable_while_one_is_blocked() {
    let setup = setup();
    let other_saves = setup.world.home.join("Saves").join("Fine");
    write(&other_saves.join("x.sav"), "x");
    let other = {
        let _host = setup.world.host();
        setup.world.custom_game("Fine", &other_saves).0
    };
    {
        let mut host = setup.world.host_with(&[], &[("SAVESCUMMER_TEST_CRASH_AT", "load.set_aside:1")]);
        let _ = setup.world.cli(&["load", &setup.game]);
        assert_eq!(host.wait_exit(Duration::from_secs(20)), Some(86));
    }
    std::fs::remove_file(setup.saves.join("a.sav.ssold")).unwrap();
    let _host = setup.world.host();
    assert!(!setup.world.game(&setup.game)["blocked"].is_null());
    setup.world.ok(&["save", &other]);
    write(&other_saves.join("x.sav"), "y");
    setup.world.ok(&["load", &other]);
    assert_eq!(read(&other_saves.join("x.sav")), "x");
}
