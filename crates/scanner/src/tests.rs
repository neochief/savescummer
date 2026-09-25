use std::fs;
use std::path::{Path, PathBuf};

use savescummer_catalog::{Bundle, Detect, Executables, Game, KnownFolders, PathRule, Platform, Source, Store};

use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn game(id: &str, steam: Option<u64>, gog: Option<u64>, dir: &str, exe: &str) -> Game {
    Game {
        id: id.into(),
        name: id.into(),
        info: None,
        detect: Detect { steam: steam.into_iter().collect(), gog: gog.into_iter().collect(), uninstall: vec![] },
        install_dirs: vec![dir.into()],
        executables: Executables { windows: vec![exe.into()], linux: vec![exe.into()], macos: vec![exe.into()] },
        save: vec![PathRule::new("{INSTALL_DIR}/save")],
        exclude: vec![],
    }
}

fn bundle(games: Vec<Game>) -> Bundle {
    Bundle { schema: 1, source: Source { repo: "r".into(), revision: "x".into() }, games }
}

struct Machine {
    _dir: tempfile::TempDir,
    root: PathBuf,
    env: Environment,
}

fn machine() -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let steam = root.join("Steam");
    fs::create_dir_all(steam.join("steamapps")).unwrap();
    let env = Environment {
        platform: Platform::current(),
        folders: KnownFolders { home: Some(root.join("home")), steam_root: Some(steam), ..Default::default() },
        steam_active_user_file: Some(root.join("active-user")),
        query_os: false,
        steam_registry_file: None,
        gog_games: vec![],
        uninstall: vec![],
        uninstall_keys: vec![],
        epic_manifests: Some(root.join("epic")),
        loose_roots: vec![LooseRoot { path: root.join("Programs"), store: Store::Standalone }],
    };
    Machine { _dir: dir, root, env }
}

fn app_manifest(library: &Path, appid: u64, dir: &str) {
    write(
        &library.join(format!("steamapps/appmanifest_{appid}.acf")),
        &format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\"{appid}\"\n\t\"installdir\"\t\"{dir}\"\n\t\"StateFlags\"\t\"4\"\n}}\n"
        ),
    );
    fs::create_dir_all(library.join("steamapps/common").join(dir)).unwrap();
}

fn library_folders(steam: &Path, libraries: &[&Path]) {
    let mut text = String::from("\"libraryfolders\"\n{\n");
    for (i, lib) in libraries.iter().enumerate() {
        let escaped = lib.to_string_lossy().replace('\\', "\\\\");
        text.push_str(&format!("\t\"{i}\"\n\t{{\n\t\t\"path\"\t\t\"{escaped}\"\n\t}}\n"));
    }
    text.push('}');
    write(&steam.join("steamapps/libraryfolders.vdf"), &text);
}

#[test]
fn steam_installs_across_libraries() {
    let m = machine();
    let steam = m.env.folders.steam_root.clone().unwrap();
    let second = m.root.join("Lib2");
    library_folders(&steam, &[&steam, &second]);
    app_manifest(&steam, 1, "Game One");
    app_manifest(&second, 2, "Game Two");
    write(&steam.join("steamapps/common/Game One/one.exe"), "");
    write(&second.join("steamapps/common/Game Two/two.exe"), "");
    // A manifest without its folder isn't an install; an unknown app is ignored.
    write(&steam.join("steamapps/appmanifest_3.acf"), "\"AppState\" { \"installdir\" \"Gone\" }");
    app_manifest(&steam, 99, "Not In Catalog");
    let b = bundle(vec![
        game("steam-1", Some(1), None, "Game One", "one.exe"),
        game("steam-2", Some(2), None, "Game Two", "two.exe"),
        game("steam-3", Some(3), None, "Gone", "gone.exe"),
    ]);
    let found = discover(&b, &m.env);
    let ids: Vec<&str> = found.installs.iter().map(|i| i.catalog_id.as_str()).collect();
    assert_eq!(ids, vec!["steam-1", "steam-2"]);
    assert!(found.installs[1].install_dir.ends_with("Lib2/steamapps/common/Game Two"));
}

#[test]
fn an_unreadable_library_is_reported_not_uninstalled() {
    let m = machine();
    let steam = m.env.folders.steam_root.clone().unwrap();
    let gone = if cfg!(windows) {
        let free = ('D'..='Z').rev().find(|l| fs::metadata(format!("{l}:\\")).is_err()).unwrap();
        PathBuf::from(format!("{free}:\\SteamLibrary"))
    } else {
        PathBuf::from("/nonexistent-drive/SteamLibrary")
    };
    library_folders(&steam, &[&steam, &gone]);
    let found = discover(&bundle(vec![]), &m.env);
    if cfg!(windows) {
        assert_eq!(found.unreadable, vec![gone]);
    }
}

#[test]
fn gog_epic_and_loose_installs() {
    let mut m = machine();
    let gog_dir = m.root.join("GOG/Game");
    write(&gog_dir.join("game.exe"), "");
    m.env.gog_games = vec![GogGame { id: 77, path: gog_dir.clone() }];
    let epic_dir = m.root.join("EpicGames/EpicGame");
    write(&epic_dir.join("epic.exe"), "");
    let item = serde_json::json!({ "InstallLocation": epic_dir, "DisplayName": "Epic Game" });
    write(&m.root.join("epic/ABC.item"), &item.to_string());
    let loose = m.root.join("Programs/Loose Game");
    write(&loose.join("loose.exe"), "");
    // A loose folder without the executable isn't an install.
    fs::create_dir_all(m.root.join("Programs/Empty Game")).unwrap();
    let b = bundle(vec![
        game("gog-77", None, Some(77), "Game", "game.exe"),
        game("epic-game", None, None, "EpicGame", "epic.exe"),
        game("loose-game", None, None, "Loose Game", "loose.exe"),
        game("empty-game", None, None, "Empty Game", "empty.exe"),
    ]);
    let found = discover(&b, &m.env);
    let got: Vec<(&str, Store)> = found.installs.iter().map(|i| (i.catalog_id.as_str(), i.store)).collect();
    assert_eq!(got, vec![("gog-77", Store::Gog), ("epic-game", Store::Epic), ("loose-game", Store::Standalone)]);
}

#[test]
fn loose_probing_runs_alongside_a_store_install() {
    let mut m = machine();
    let steam = m.env.folders.steam_root.clone().unwrap();
    app_manifest(&steam, 5, "Dual");
    write(&steam.join("steamapps/common/Dual/dual.exe"), "");
    let loose = m.root.join("Programs/Dual");
    write(&loose.join("dual.exe"), "");
    // The Steam folder is also a loose root: the same folder is one install.
    m.env.loose_roots.push(LooseRoot { path: steam.join("steamapps/common"), store: Store::Standalone });
    let found = discover(&bundle(vec![game("steam-5", Some(5), None, "Dual", "dual.exe")]), &m.env);
    let stores: Vec<Store> = found.installs.iter().map(|i| i.store).collect();
    assert_eq!(stores, vec![Store::Steam, Store::Standalone]);
}

fn login_users_file(steam: &Path, users: &[(u64, Option<&str>, u64)]) {
    let mut text = String::from("\"users\"\n{\n");
    for (id, most_recent, stamp) in users {
        text.push_str(&format!("\t\"{id}\"\n\t{{\n\t\t\"AccountName\"\t\"x\"\n\t\t\"Timestamp\"\t\"{stamp}\"\n"));
        if let Some(mr) = most_recent {
            text.push_str(&format!("\t\t\"MostRecent\"\t\"{mr}\"\n"));
        }
        text.push_str("\t}\n");
    }
    text.push('}');
    write(&steam.join("config/loginusers.vdf"), &text);
}

const A64: u64 = 76561198004523847; // account 44258119
const B64: u64 = 76561197960265729; // account 1

#[test]
fn steam_account_order() {
    let m = machine();
    let steam = m.env.folders.steam_root.clone().unwrap();
    let active = m.root.join("active-user");
    // Only userdata folders: one → that one; several → unknown.
    fs::create_dir_all(steam.join("userdata/44258119")).unwrap();
    assert_eq!(m.env.steam_account().unwrap().account_id, 44258119);
    fs::create_dir_all(steam.join("userdata/1")).unwrap();
    assert_eq!(m.env.steam_account(), None, "several userdata folders with no other source");
    // Newest Timestamp.
    login_users_file(&steam, &[(A64, None, 100), (B64, None, 200)]);
    assert_eq!(m.env.steam_account().unwrap().account_id, 1);
    // MostRecent wins over a newer Timestamp.
    login_users_file(&steam, &[(A64, Some("1"), 100), (B64, Some("0"), 200)]);
    assert_eq!(m.env.steam_account().unwrap().account_id, 44258119);
    // ActiveUser wins over loginusers.vdf; 0 falls through.
    write(&active, "1");
    assert_eq!(m.env.steam_account().unwrap().account_id, 1);
    write(&active, "0");
    assert_eq!(m.env.steam_account().unwrap().account_id, 44258119);
}

#[test]
fn watch_locations_follow_libraries() {
    let m = machine();
    let steam = m.env.folders.steam_root.clone().unwrap();
    let second = m.root.join("Lib2");
    library_folders(&steam, &[&steam, &second]);
    let watched = m.env.watch_locations();
    assert!(watched.contains(&steam.join("steamapps")));
    assert!(watched.contains(&second.join("steamapps")));
    assert!(watched.contains(&steam.join("steamapps/libraryfolders.vdf")));
}

#[test]
fn environments_load_from_json() {
    let m = machine();
    let text = serde_json::to_string(&m.env).unwrap();
    let path = m.root.join("env.json");
    fs::write(&path, text).unwrap();
    let env = Environment::from_file(&path).unwrap();
    assert_eq!(env.folders, m.env.folders);
}

#[cfg(windows)]
#[test]
fn detects_this_machine_without_panicking() {
    let env = Environment::detect();
    assert!(env.folders.home.is_some());
    assert!(env.folders.documents.is_some());
    let _ = env.steam_account();
    let _ = env.broad_folders();
}

#[test]
fn standalone_installs_from_uninstall_keys() {
    let mut m = machine();
    let dir = m.root.join("Games/Some Studio/Standalone");
    write(&dir.join("bin/game.exe"), "");
    let stale = m.root.join("Games/Uninstalled");
    fs::create_dir_all(&stale).unwrap();
    // Also under a loose root, so the loose probe finds the same folder.
    let loose = m.root.join("Programs/Dup");
    write(&loose.join("dup.exe"), "");
    m.env.uninstall = vec![
        UninstallEntry { key: "{ABC-123}_is1".into(), path: dir.clone() },
        UninstallEntry { key: "Stale".into(), path: stale },
        UninstallEntry { key: "Dup".into(), path: loose.clone() },
    ];
    let mut listed = game("standalone", None, None, "Unrelated Folder Name", "bin/game.exe");
    listed.detect.uninstall = vec!["{abc-123}_IS1".into()];
    // The key exists but its folder has no executable: not an install.
    let mut stale_game = game("stale", None, None, "x", "stale.exe");
    stale_game.detect.uninstall = vec!["Stale".into(), "Missing Key".into()];
    let mut dup = game("dup", None, None, "Dup", "dup.exe");
    dup.detect.uninstall = vec!["Dup".into()];
    let found = discover(&bundle(vec![listed, stale_game, dup]), &m.env);
    let got: Vec<(&str, Store, &Path)> =
        found.installs.iter().map(|i| (i.catalog_id.as_str(), i.store, i.install_dir.as_path())).collect();
    assert_eq!(
        got,
        vec![("standalone", Store::Standalone, dir.as_path()), ("dup", Store::Standalone, loose.as_path())]
    );
}

#[cfg(windows)]
#[test]
fn the_real_uninstall_and_gog_keys_are_read_and_watched() {
    let env = Environment::detect();
    assert_eq!(env.uninstall_keys.len(), 3);
    let watched = env.watch_registry_keys();
    assert_eq!(watched.len(), 5, "three uninstall keys and two GOG keys: {watched:?}");
    assert!(watched.iter().any(|k| k.path.ends_with("Games") && k.path.contains("GOG.com")));
    assert_eq!(env.uninstall_location("SaveScummer test key that does not exist"), None);
}

fn manifest_with_flags(library: &Path, appid: u64, dir: &str, flags: &str) {
    write(
        &library.join(format!("steamapps/appmanifest_{appid}.acf")),
        &format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\"{appid}\"\n\t\"installdir\"\t\"{dir}\"\n\t\"StateFlags\"\t\"{flags}\"\n}}\n"
        ),
    );
}

#[test]
fn a_steam_install_counts_once_finished_and_with_its_executable() {
    let m = machine();
    let steam = m.env.folders.steam_root.clone().unwrap();
    let b = bundle(vec![game("steam-7", Some(7), None, "Seven", "seven.exe")]);
    let found = || discover(&b, &m.env).installs.len();

    // Steam starts the install: manifest and folder exist, files arrive.
    manifest_with_flags(&steam, 7, "Seven", "1026");
    write(&steam.join("steamapps/common/Seven/seven.exe"), "");
    assert_eq!(found(), 0, "still downloading");
    // Finished.
    manifest_with_flags(&steam, 7, "Seven", "4");
    assert_eq!(found(), 1);
    // An update keeps the fully-installed bit: the game stays.
    manifest_with_flags(&steam, 7, "Seven", "1030");
    assert_eq!(found(), 1, "updating");
    // A finished manifest without the executable isn't an install.
    manifest_with_flags(&steam, 7, "Seven", "4");
    fs::remove_file(steam.join("steamapps/common/Seven/seven.exe")).unwrap();
    assert_eq!(found(), 0, "no executable");
    // A game the catalog lists no executables for is taken on the manifest.
    let mut bare = game("steam-7", Some(7), None, "Seven", "x");
    bare.executables = Executables::default();
    assert_eq!(discover(&bundle(vec![bare]), &m.env).installs.len(), 1);
}
