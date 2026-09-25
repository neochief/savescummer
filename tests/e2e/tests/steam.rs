//! Steam specifics: the save set follows the Steam account a game starts
//! under, and the Steam Cloud check at the first start after a Load.

mod common;

use common::*;

const ID64_B: u64 = 76561197960265729;

fn userdata(world: &World, account: u32) -> std::path::PathBuf {
    world.steam.join("userdata").join(account.to_string()).join("1002").join("remote")
}

fn local(world: &World, id64: u64) -> std::path::PathBuf {
    world.documents.join("My Games").join("CloudGame").join(id64.to_string())
}

#[test]
#[cfg_attr(not(windows), ignore = "needs a process source for this OS (PLAN-MACOS.md, PROCESS MONITORING)")]
fn a_steam_account_switch_between_sessions_follows_the_account_at_game_start() {
    let world = World::new();
    let install = world.steam_install(1002, "Cloud Game", "CloudGame.exe");
    write(&userdata(&world, ACCOUNT_A).join("save.json"), "A progress");
    write(&local(&world, ID64_A).join("local.sav"), "A local");
    write(&userdata(&world, ACCOUNT_B).join("save.json"), "B progress");
    write(&local(&world, ID64_B).join("local.sav"), "B local");
    let _host = world.host();
    let set = world.ok(&["save-set", "steam-1002"]);
    assert_eq!(set["context"]["steam_account"], ACCOUNT_A);
    world.ok(&["save", "steam-1002", "--label", "account A"]);

    // Another account logs in, then the game starts: it is re-resolved at
    // its start, before any Save.
    world.set_steam_user(ACCOUNT_B);
    let exe = install.join("CloudGame.exe");
    let mut running = launch(&exe, &[]);
    world.wait_game("steam-1002", "running", |g| g["running"] == true);
    let set = world.ok(&["save-set", "steam-1002"]);
    assert_eq!(set["context"]["steam_account"], ACCOUNT_B, "{set}");
    // Account A's checkpoint belongs to A's folders: unavailable.
    let game = world.game("steam-1002");
    assert_eq!(game["load"]["reason"], "no_saves");
    let rows = world.history("steam-1002");
    assert_eq!(rows.last().unwrap()["unavailable"], "different_save_set");
    world.ok(&["save", "steam-1002", "--label", "account B"]);
    write(&userdata(&world, ACCOUNT_B).join("save.json"), "B later");
    world.ok(&["load", "steam-1002"]);
    assert_eq!(read(&userdata(&world, ACCOUNT_B).join("save.json")), "B progress");
    assert_eq!(
        read(&userdata(&world, ACCOUNT_A).join("save.json")),
        "A progress",
        "never into another account's saves"
    );
    running.quit();
    world.wait_game("steam-1002", "closed", |g| g["running"] == false);

    // Back to A: A's checkpoint is usable again, with the same id.
    world.set_steam_user(ACCOUNT_A);
    let mut running = launch(&exe, &[]);
    world.wait_game("steam-1002", "running", |g| g["running"] == true);
    let latest = world.game("steam-1002")["latest"].clone();
    assert_eq!(latest["label"], "account A");
    write(&userdata(&world, ACCOUNT_A).join("save.json"), "A later");
    world.ok(&["load", "steam-1002"]);
    assert_eq!(read(&userdata(&world, ACCOUNT_A).join("save.json")), "A progress");
    running.quit();
}

#[test]
#[cfg_attr(not(windows), ignore = "needs a process source for this OS (PLAN-MACOS.md, PROCESS MONITORING)")]
fn steam_cloud_replacing_a_restored_file_is_noted_at_the_next_start() {
    let world = World::new();
    let install = world.steam_install(1002, "Cloud Game", "CloudGame.exe");
    let remote = userdata(&world, ACCOUNT_A);
    write(&remote.join("save.json"), "checkpointed");
    write(&local(&world, ID64_A).join("local.sav"), "local");
    let _host = world.host();
    world.ok(&["save", "steam-1002"]);
    write(&remote.join("save.json"), "played on");

    // Load with the game closed, then launch.
    world.ok(&["load", "steam-1002"]);
    assert_eq!(read(&remote.join("save.json")), "checkpointed");
    let exe = install.join("CloudGame.exe");
    let mut first = launch(&exe, &[]);
    world.wait_game("steam-1002", "running", |g| g["running"] == true);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(world.history("steam-1002")[1]["cloud_replaced"], false, "untouched files: no note");
    first.quit();
    world.wait_game("steam-1002", "closed", |g| g["running"] == false);

    // This time Steam downloads its cloud copy over the restored file before
    // the game starts.
    world.ok(&["load", "steam-1002"]);
    write(&remote.join("save.json"), "cloud copy from the Deck");
    let mut second = launch(&exe, &[]);
    world.wait_game("steam-1002", "running", |g| g["running"] == true);
    let loaded = wait_for("the cloud check", std::time::Duration::from_secs(10), || {
        let rows = world.history("steam-1002");
        let row = rows.iter().find(|r| r["kind"] == "loaded").cloned()?;
        (row["cloud_replaced"] == true).then_some(row)
    });
    assert_eq!(loaded["kind"], "loaded");
    assert_eq!(read(&remote.join("save.json")), "cloud copy from the Deck", "nothing is changed");
    second.quit();
}
