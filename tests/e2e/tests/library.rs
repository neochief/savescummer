//! The game library: known games found through a (fake) Steam library and
//! the catalog, installs and uninstalls noticed without asking, several
//! installs of one game, custom games, and the save set safety rules.

mod common;

use std::time::Duration;

use common::*;
use serde_json::json;

#[test]
fn a_steam_game_is_found_with_its_whole_save_set() {
    let world = World::new();
    let install = world.steam_install(1001, "Rogue One", "RogueOne.exe");
    write(&install.join("saves/run.sav"), "run");
    write(&install.join("saves/options.ini"), "volume=3");
    write(&world.appdata.join("RogueOne/profile.dat"), "profile");
    let _host = world.host();

    let game = world.game("steam-1001");
    assert_eq!(game["name"], "Rogue One");
    assert_eq!(game["kind"], "known");
    assert_eq!(game["store"], "steam");
    assert_eq!(game["info"], "Exit to the main menu before loading.");
    assert_eq!(game["save"]["available"], true);

    let set = world.ok(&["save-set", "steam-1001"]);
    let active = set["active"].as_array().unwrap();
    assert_eq!(active.len(), 2, "every target, never a pick: {set}");
    assert_eq!(active[0]["filter"]["value"], "saves");
    assert_eq!(active[0]["excludes"], json!(["saves/options.ini"]));
    assert_eq!(active[1]["filter"]["value"], "profile.dat");

    // Save copies both targets, never the settings file; Load puts both back.
    world.ok(&["save", "Rogue One"]);
    write(&install.join("saves/run.sav"), "later");
    write(&install.join("saves/options.ini"), "volume=9");
    write(&world.appdata.join("RogueOne/profile.dat"), "later profile");
    world.ok(&["load", "steam-1001"]);
    assert_eq!(read(&install.join("saves/run.sav")), "run");
    assert_eq!(read(&world.appdata.join("RogueOne/profile.dat")), "profile");
    assert_eq!(read(&install.join("saves/options.ini")), "volume=9", "a Load never resets settings");
}

#[test]
fn an_install_is_noticed_without_asking_and_an_uninstall_hides_but_keeps_history() {
    let world = World::new();
    let _host = world.host_with(&["--watch"], &[]);
    assert!(world.game("steam-1001").is_null());

    // Steam finishes an install while the host runs: the watched steamapps
    // folder triggers an install scan, with no request.
    let install = world.steam_install(1001, "Rogue One", "RogueOne.exe");
    write(&install.join("saves/run.sav"), "run");
    world.wait_game("steam-1001", "the new install is found", |g| g["installed"] == true);
    let background = world.state()["scan"].clone();
    assert!(background["last_user"].is_null(), "background scans are silent: {background}");
    world.ok(&["save", "steam-1001", "--label", "first"]);

    // Uninstalled: hidden, never forgotten.
    world.steam_uninstall(1001, "Rogue One");
    world.wait_state("the game is hidden", |s| !s["games"].as_array().unwrap().iter().any(|g| g["id"] == "steam-1001"));
    // Reinstalled: it comes back with its history.
    let install = world.steam_install(1001, "Rogue One", "RogueOne.exe");
    write(&install.join("saves/run.sav"), "run 2");
    world.wait_game("steam-1001", "the game is back", |g| g["installed"] == true);
    let rows = world.history("steam-1001");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["label"], "first");

    // The host's log says what happened, in order, and why it looked.
    let log = read(&world.data.join("host.log"));
    let events: Vec<&str> = log.lines().filter(|l| l.contains("Rogue One (steam-1001)")).collect();
    assert_eq!(events.len(), 3, "{log}");
    assert!(events[0].contains("] installed: Rogue One (steam-1001) at Steam, "), "{}", events[0]);
    assert!(events[1].contains("] uninstalled: Rogue One (steam-1001)"), "{}", events[1]);
    assert!(events[2].contains("] installed again: Rogue One (steam-1001)"), "{}", events[2]);
    assert!(events.iter().all(|e| e.contains("(scan: a store folder or registry key changed)")), "{log}");
}

#[test]
fn a_steam_download_in_progress_is_not_an_install_until_steam_finishes() {
    let world = World::new();
    let _host = world.host_with(&["--watch"], &[]);

    // Steam starts downloading: the manifest says so, and files (even the
    // executable) arrive while it runs.
    world.steam_manifest(1001, "Rogue One", 1026);
    let install = world.steam.join("steamapps").join("common").join("Rogue One");
    copy_game(&install.join("RogueOne.exe"));
    world.ok(&["scan"]);
    std::thread::sleep(std::time::Duration::from_secs(3)); // past the watch debounce
    assert!(world.game("steam-1001").is_null(), "not installed while downloading");

    // Steam rewrites the manifest when it finishes: the game appears then.
    world.steam_manifest(1001, "Rogue One", 4);
    world.wait_game("steam-1001", "the finished install is found", |g| g["installed"] == true);
    let log = read(&world.data.join("host.log"));
    let events: Vec<&str> = log.lines().filter(|l| l.contains("(steam-1001)")).collect();
    assert_eq!(events.len(), 1, "{log}");
    assert!(events[0].contains("] installed: Rogue One"), "{}", events[0]);

    // An update in progress keeps the fully-installed bit: nothing changes.
    world.steam_manifest(1001, "Rogue One", 1030);
    world.ok(&["scan"]);
    assert_eq!(world.game("steam-1001")["installed"], true, "still installed while updating");
}

#[test]
fn a_user_scan_reports_newly_found_games_and_scans_never_duplicate() {
    let world = World::new();
    world.steam_install(1001, "Rogue One", "RogueOne.exe");
    let _host = world.host();
    world.steam_install(1005, "Pattern Game", "PatternGame.exe");
    let result = world.ok(&["scan"]);
    assert_eq!(result["new_games"], 1);
    assert_eq!(result["origin"], "user");
    let again = world.ok(&["scan", "--full"]);
    assert_eq!(again["new_games"], 0);
    let games = world.state()["games"].as_array().unwrap().len();
    assert_eq!(games, 2);
}

#[test]
fn two_installs_of_one_game_are_two_records_with_install_tags() {
    let world = World::new();
    let steam_copy = world.steam_install(1004, "Twin Game", "TwinGame.exe");
    write(&steam_copy.join("save/slot"), "steam progress");
    let gog_copy = world.root.join("GOG Games").join("Twin Game");
    copy_game(&gog_copy.join("TwinGame.exe"));
    write(&gog_copy.join("save/slot"), "gog progress");
    world.set_gog_games(json!([{ "id": 2004, "path": gog_copy }]));
    let _host = world.host();

    let steam = world.game("steam-1004");
    let gog = world.game("steam-1004#gog");
    assert_eq!(steam["install_tag"], "Steam");
    assert_eq!(gog["install_tag"], "GOG");

    // Each record has its own save set and history; hotkeys follow the
    // install that runs.
    world.ok(&["save", "steam-1004#gog"]);
    let mut running = launch(&gog_copy.join("TwinGame.exe"), &[]);
    world.wait_game("steam-1004#gog", "the GOG copy runs", |g| g["running"] == true);
    assert_eq!(world.game("steam-1004")["running"], false);
    assert_eq!(s(&world.ok(&["hotkey-target"])["game"]), "steam-1004#gog");
    write(&gog_copy.join("save/slot"), "gog, later");
    world.ok(&["hotkey", "load"]);
    assert_eq!(read(&gog_copy.join("save/slot")), "gog progress");
    assert_eq!(read(&steam_copy.join("save/slot")), "steam progress", "the other copy is untouched");
    running.quit();
    assert!(world.kinds("steam-1004").is_empty());
}

/// A catalog game only found through its installer's uninstall key.
fn with_indie_quest(world: &World) {
    let mut catalog = fixture_catalog();
    catalog["games"].as_array_mut().unwrap().push(json!({
        "id": "indie-quest",
        "name": "Indie Quest",
        "detect": { "uninstall": ["{5A1E-QUEST}_is1"] },
        "installDirs": ["Indie Quest"],
        "executables": { "windows": ["bin/Quest.exe"] },
        "save": [ { "path": "{INSTALL_DIR}/saves" } ]
    }));
    world.set_catalog(&catalog);
}

#[test]
fn a_standalone_install_is_found_through_its_uninstall_key() {
    // Installed by its own installer somewhere no loose probe looks, and
    // under a folder name the catalog doesn't know.
    let world = World::new();
    with_indie_quest(&world);
    let install = world.root.join("D-Games").join("IQ v1.2");
    copy_game(&install.join("bin").join("Quest.exe"));
    write(&install.join("saves/slot1"), "chapter 1");
    world.register_uninstall("{5A1E-QUEST}_is1", &install);
    let _host = world.host();

    let game = world.game("indie-quest");
    assert_eq!(game["installed"], true, "{game}");
    world.ok(&["save", "indie-quest"]);
    write(&install.join("saves/slot1"), "chapter 2");
    world.ok(&["load", "indie-quest"]);
    assert_eq!(read(&install.join("saves/slot1")), "chapter 1");
    let mut running = launch(&install.join("bin").join("Quest.exe"), &[]);
    world.wait_game("indie-quest", "the standalone copy runs", |g| g["running"] == true);
    running.quit();
    world.wait_game("indie-quest", "closed", |g| g["running"] == false);

    // Uninstalled: the key and the folder are gone. The game is hidden but
    // its history stays.
    world.unregister_uninstall("{5A1E-QUEST}_is1");
    std::fs::remove_dir_all(&install).unwrap();
    world.ok(&["scan"]);
    world.wait_state("the game is hidden", |s| s["games"].as_array().unwrap().iter().all(|g| g["id"] != "indie-quest"));
    assert!(!world.history("indie-quest").is_empty());
}

#[test]
fn a_standalone_install_and_uninstall_are_noticed_through_the_registry() {
    // Nobody asks for a scan: the uninstall key changing is the signal.
    let world = World::new();
    with_indie_quest(&world);
    let _host = world.host_with(&["--watch"], &[]);
    assert!(world.game("indie-quest").is_null());

    let install = world.root.join("D-Games").join("Indie Quest");
    copy_game(&install.join("bin").join("Quest.exe"));
    write(&install.join("saves/slot1"), "chapter 1");
    world.register_uninstall("{5A1E-QUEST}_is1", &install);
    world.wait_game("indie-quest", "the install is noticed", |g| g["installed"] == true);
    assert!(world.state()["scan"]["last_user"].is_null(), "found by a background scan");
    world.ok(&["save", "indie-quest"]);

    // The uninstaller removes the files, then its own entry.
    std::fs::remove_dir_all(&install).unwrap();
    world.unregister_uninstall("{5A1E-QUEST}_is1");
    world.wait_state("the uninstall is noticed", |s| {
        s["games"].as_array().unwrap().iter().all(|g| g["id"] != "indie-quest")
    });
    assert_eq!(world.kinds("indie-quest"), vec!["saved"], "hidden, never forgotten");
}

#[test]
fn a_program_inside_the_install_folder_counts_as_the_game() {
    // Launchers: the catalog knows SlayTheSpire.exe, but the game runs as
    // jre/bin/javaw.exe inside the install folder.
    let world = World::new();
    let install = world.steam_install(1001, "Rogue One", "RogueOne.exe");
    write(&install.join("saves/run.sav"), "run");
    let _host = world.host();
    let helper = install.join("runtime").join("bin").join("engine.exe");
    copy_game(&helper);
    let mut running = launch(&helper, &[]);
    world.wait_game("steam-1001", "the engine process counts as the game", |g| g["running"] == true);
    running.quit();
    world.wait_game("steam-1001", "closed", |g| g["running"] == false);
}

#[test]
fn custom_games_are_validated_whole_and_survive_scans() {
    let world = World::new();
    let _host = world.host();
    let exe = world.root.join("games").join("Indie").join("Indie.exe");
    copy_game(&exe);
    let exe = exe.to_str().unwrap().to_string();
    let add = |saves: &str| world.cli(&["add-game", "--name", "Indie", "--exe", &exe, "--saves", saves]);

    // A relative path.
    assert_eq!(add("saves").error_kind(), "invalid_config");
    // A broad folder whole, or a wildcard directly in one.
    let docs = world.documents.to_str().unwrap().to_string();
    assert_eq!(add(&docs).error_kind(), "invalid_target");
    assert_eq!(add(&format!("{docs}\\*.sav")).error_kind(), "invalid_target");
    // A wildcard in the game's own folder could match the game's files.
    let game_dir = world.root.join("games").join("Indie");
    assert_eq!(add(&format!("{}\\save*", game_dir.display())).error_kind(), "invalid_target");
    // A filter that matches the executable.
    assert_eq!(
        add(&format!("{}\\*.exe", game_dir.join("x").display()).replace("\\x\\", "\\")).error_kind(),
        "invalid_target"
    );
    // A reserved suffix on its own.
    assert_eq!(add(&format!("{}\\saves\\*.ssnew", game_dir.display())).error_kind(), "invalid_target");
    assert!(world.state()["games"].as_array().unwrap().is_empty(), "nothing partial is left behind");

    // An exact name inside a broad folder is fine, and so is a path that
    // doesn't exist yet.
    let ok = world.ok(&["add-game", "--name", "Indie", "--exe", &exe, "--saves", &format!("{docs}\\indie.sav")]);
    let id = s(&ok["game"]);
    assert_eq!(world.game(&id)["save"]["reason"], "no_game_data");
    assert!(!world.documents.join("indie.sav").exists(), "validation never creates anything");

    // Overlaps with another game are refused, naming it.
    let other_exe = world.root.join("games").join("Other").join("Other.exe");
    copy_game(&other_exe);
    let overlap = world.cli(&[
        "add-game",
        "--name",
        "Other",
        "--exe",
        other_exe.to_str().unwrap(),
        "--saves",
        &format!("{docs}\\indie.sav"),
    ]);
    assert_eq!(overlap.error_kind(), "invalid_target");
    assert!(overlap.stdout.contains("Indie"), "the error names the other game: {}", overlap.stdout);

    // A patterned location in the game's own save folder works.
    let saves = world.home.join("IndieSaves");
    write(&saves.join("slot1.sav"), "one");
    write(&saves.join("notes.txt"), "not a save");
    world.ok(&["configure", &id, "--saves", &format!("{}\\*.sav", saves.display())]);
    world.ok(&["scan", "--full"]);
    world.ok(&["save", &id]);
    write(&saves.join("slot1.sav"), "later");
    write(&saves.join("notes.txt"), "edited notes");
    world.ok(&["load", &id]);
    assert_eq!(read(&saves.join("slot1.sav")), "one");
    assert_eq!(read(&saves.join("notes.txt")), "edited notes", "only what the pattern matches is restored");

    // An invalid change leaves the old configuration exactly as it was.
    assert_eq!(world.cli(&["configure", &id, "--saves", &docs]).error_kind(), "invalid_target");
    let set = world.ok(&["save-set", &id]);
    assert!(s(&set["location"]).ends_with("*.sav"));
}

#[test]
fn an_override_replaces_the_catalog_save_set_and_reset_brings_it_back() {
    let world = World::new();
    let install = world.steam_install(1001, "Rogue One", "RogueOne.exe");
    write(&install.join("saves/run.sav"), "catalog run");
    let _host = world.host();
    world.ok(&["save", "steam-1001", "--label", "catalog"]);

    let elsewhere = world.home.join("RogueOneSaves");
    write(&elsewhere.join("run.sav"), "override run");
    world.ok(&["configure", "steam-1001", "--saves", elsewhere.to_str().unwrap()]);
    let set = world.ok(&["save-set", "steam-1001"]);
    assert_eq!(set["active"].as_array().unwrap().len(), 1);
    assert_eq!(set["catalog"].as_array().unwrap().len(), 2, "the catalog's targets stay visible");
    // The old checkpoint shares no target with the override: unavailable.
    let game = world.game("steam-1001");
    assert_eq!(game["load"]["reason"], "no_saves");
    let rows = world.history("steam-1001");
    assert_eq!(rows[0]["unavailable"], "different_save_set");
    assert_eq!(rows[0]["actions"]["load"], false);
    let out = world.cli(&["load", "steam-1001", "--checkpoint", &s(&rows[0]["checkpoint"])]);
    assert_eq!(out.error_kind(), "different_save_set");

    // Reset: the old checkpoint is usable again, same ids and history.
    world.ok(&["configure", "steam-1001", "--reset-saves"]);
    let rows = world.history("steam-1001");
    assert!(rows[0]["unavailable"].is_null());
    write(&install.join("saves/run.sav"), "changed");
    world.ok(&["load", "steam-1001"]);
    assert_eq!(read(&install.join("saves/run.sav")), "catalog run");
}

#[test]
fn a_linked_save_folder_is_resolved_and_a_repointed_link_is_refused() {
    let world = World::new();
    let real = world.home.join("RealSaves");
    write(&real.join("a.sav"), "a");
    let link = world.home.join("LinkedSaves");
    // A junction needs no admin rights.
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J", link.to_str().unwrap(), real.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(status.status.success(), "mklink /J failed");
    let _host = world.host();
    let exe = world.root.join("games").join("Linky").join("Linky.exe");
    copy_game(&exe);
    let added = world.ok(&[
        "add-game",
        "--name",
        "Linky",
        "--exe",
        exe.to_str().unwrap(),
        "--saves",
        link.join("a.sav").to_str().unwrap(),
    ]);
    let id = s(&added["game"]);
    let set = world.ok(&["save-set", &id]);
    assert!(s(&set["active"][0]["root"]).ends_with("RealSaves"), "operations run on the real folder: {set}");
    world.ok(&["save", &id]);
    write(&real.join("a.sav"), "b");
    world.ok(&["load", &id]);
    assert_eq!(read(&real.join("a.sav")), "a");

    // The link now points elsewhere: refused until configured again.
    let other = world.home.join("OtherSaves");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::remove_dir(&link).unwrap();
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J", link.to_str().unwrap(), other.to_str().unwrap()])
        .output()
        .unwrap();
    world.ok(&["scan"]);
    let game = world.game(&id);
    assert_eq!(game["config_error"]["kind"], "invalid_target", "{game}");
    assert_eq!(world.cli(&["save", &id]).code, 3);
}

#[test]
fn a_game_whose_catalog_has_no_location_for_this_build_stays_visible() {
    let world = World::new();
    let mut catalog = fixture_catalog();
    catalog["games"].as_array_mut().unwrap().push(json!({
        "id": "steam-1009",
        "name": "Mac Only",
        "detect": { "steam": 1009 },
        "save": [ { "when": { "os": "macos" }, "path": "{HOME}/Library/Application Support/MacOnly" } ]
    }));
    world.set_catalog(&catalog);
    world.steam_install(1009, "Mac Only", "MacOnly.exe");
    let _host = world.host();
    let game = world.game("steam-1009");
    assert_eq!(game["installed"], true);
    assert_eq!(game["save"]["reason"], "no_save_location");
    assert_eq!(game["config_error"]["kind"], "no_save_location");
    // The user can set a location in Configure.
    let saves = world.home.join("MacOnlySaves");
    write(&saves.join("s"), "s");
    world.ok(&["configure", "steam-1009", "--saves", saves.to_str().unwrap()]);
    assert_eq!(world.game("steam-1009")["save"]["available"], true);
}

#[test]
fn a_focus_report_runs_an_install_scan_at_most_once_per_cooldown() {
    let world = World::new();
    let _host = world.host_with(&["--focus-scan-cooldown-secs", "30"], &[]);
    let scans = || world.state()["scan"]["scans"].as_u64().unwrap();
    let before = scans();
    world.ok(&["ui-report", "--focused", "--visible"]);
    wait_for("a focus scan", Duration::from_secs(10), || (scans() > before).then_some(()));
    let after_first = scans();
    world.ok(&["ui-report", "--visible"]);
    world.ok(&["ui-report", "--focused", "--visible"]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(scans(), after_first, "a second focus within the cooldown doesn't scan");
}
