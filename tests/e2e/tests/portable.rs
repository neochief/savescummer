//! The user's folders are kept portably (`~` on macOS and Linux, `%APPDATA%`
//! and the like on Windows): a stored or shared configuration names no user
//! and works on another PC.

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use savescummer_storage::Storage;

/// A custom game whose executable and saves are in the user's folders, added
/// with full paths the way a file picker gives them.
fn add_home_game(world: &World) -> (String, std::path::PathBuf) {
    let exe = world.home.join("Games").join("Indie").join("Indie.exe");
    copy_game(&exe);
    let saves = world.user_saves("Indie");
    write(&saves.join("slot1.sav"), "one");
    let added = world.ok(&[
        "add-game",
        "--name",
        "Indie",
        "--exe",
        exe.to_str().unwrap(),
        "--saves",
        saves.join("*.sav").to_str().unwrap(),
    ]);
    (s(&added["game"]), saves)
}

fn stop(world: &World, mut host: HostProcess) {
    world.ok(&["shutdown"]);
    assert_eq!(host.wait_exit(Duration::from_secs(20)), Some(0));
}

/// Everything stored about games and checkpoints: game records, the store
/// setting, checkpoint records and each checkpoint's own `checkpoint.json`.
fn stored(world: &World, store: &Path) -> String {
    let storage = Storage::open(&world.data.join("host.db")).unwrap();
    let games = savescummer_storage::games(storage.conn()).unwrap();
    let setting = savescummer_storage::setting(storage.conn(), "checkpoint_store").unwrap();
    let checkpoints = savescummer_storage::all_live_checkpoints(storage.conn()).unwrap();
    assert!(!checkpoints.is_empty(), "a checkpoint to look at");
    let metas: Vec<String> = checkpoints
        .iter()
        .map(|c| std::fs::read_to_string(store.join(&c.folder).join("checkpoint.json")).unwrap())
        .collect();
    let rows: Vec<String> = checkpoints.iter().map(|c| serde_json::to_string(&c.targets).unwrap()).collect();
    format!("{games:?} {setting:?} {rows:?} {metas:?}")
}

fn contains_path(text: &str, path: &Path) -> bool {
    // Records are JSON: a Windows path's backslashes are escaped there.
    let path = path.to_string_lossy();
    text.contains(&*path) || text.contains(&path.replace('\\', "\\\\"))
}

#[test]
fn the_stored_configuration_names_no_user_folder() {
    let world = World::new();
    let host = world.host();
    let (game, saves) = add_home_game(&world);
    world.ok(&["scan", "--full"]);
    let store = world.home.join("Checkpoints");
    assert_eq!(world.ok(&["move-store", store.to_str().unwrap()])["status"], "succeeded");
    world.ok(&["save", &game]);

    // Shown portably too.
    assert_eq!(s(&world.ok(&["save-set", &game])["location"]), world.portable(&saves.join("*.sav")));
    let exe = world.home.join("Games").join("Indie").join("Indie.exe");
    assert_eq!(s(&world.game(&game)["executable"]), world.portable(&exe));
    assert_eq!(s(&world.state()["store"]["path"]), world.portable(&store));
    stop(&world, host);

    let stored = stored(&world, &store);
    assert!(!contains_path(&stored, &world.home), "no user folder is stored: {stored}");
    assert!(stored.contains(&world.portable(&exe).replace('\\', "\\\\")), "{stored}");
}

#[test]
fn a_configuration_moved_to_another_pc_finds_its_games_there() {
    let first = World::new();
    let host = first.host();
    let (game, _) = add_home_game(&first);
    first.ok(&["save", &game, "--label", "from the first PC"]);
    stop(&first, host);

    // Another user on another PC, with the game in the same place in their home.
    let second = World::new();
    assert_ne!(first.home, second.home);
    copy_tree(&first.data, &second.data);
    copy_game(&second.home.join("Games").join("Indie").join("Indie.exe"));
    let saves = second.user_saves("Indie");
    write(&saves.join("slot1.sav"), "played on");
    let _host = second.host();

    assert_eq!(second.game(&game)["installed"], true, "the executable is found in this home");
    assert_eq!(s(&second.ok(&["save-set", &game])["location"]), second.portable(&saves.join("*.sav")));
    // The first PC's checkpoint restores into this user's folder.
    second.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot1.sav")), "one");
    // And this PC's own checkpoints work alongside it.
    write(&saves.join("slot1.sav"), "played on");
    second.ok(&["save", &game]);
    write(&saves.join("slot1.sav"), "lost the run");
    second.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot1.sav")), "played on");
}
