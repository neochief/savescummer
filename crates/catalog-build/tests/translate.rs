//! Translation rules, manifest entry → catalog entry (PLAN-CATALOG.md 3.1).

mod common;

use common::*;
use savescummer_catalog::Platform;
use savescummer_catalog::Store;
use savescummer_catalog_build::translate::{Category, DropReason};

#[test]
fn save_untagged_and_config_save_entries_are_targets_config_only_is_not() {
    let t = translate_one(
        r#"
Game:
  files:
    "<base>/saved":
      tags: [save]
    "<base>/untagged": {}
    "<base>/progress":
      tags: [config, save]
    "<base>/settings.ini":
      tags: [config]
"#,
    );
    assert_eq!(
        t.entry.save,
        vec![any("{INSTALL_DIR}/progress"), any("{INSTALL_DIR}/saved"), any("{INSTALL_DIR}/untagged")]
    );
    // Not inside any target: dropped, not excluded.
    assert!(t.entry.exclude.is_empty());
    // A literal config+save path without an extension is listed for review.
    assert!(t.config_save_folders.contains("{INSTALL_DIR}/progress"));
    assert!(!t.config_save_folders.contains("{INSTALL_DIR}/saved"));
}

#[test]
fn config_only_inside_a_save_target_becomes_an_exclude() {
    // Slay the Spire (macOS) and Dead Cells shapes: inside a literal folder,
    // and inside a glob's fixed part.
    let t = translate_one(
        r#"
Game:
  files:
    "<base>/preferences":
      tags: [save]
      when: [{ os: mac }]
    "<base>/preferences/STSGameplaySettings":
      tags: [config]
      when: [{ os: mac }]
    "<base>/save/user_*.dat":
      tags: [save]
    "<base>/save/dc_options.json":
      tags: [config]
    "<base>/twitchconfig.txt":
      tags: [config]
    "<base>/preferences/windows-only.cfg":
      tags: [config]
      when: [{ os: windows }]
"#,
    );
    assert_eq!(
        t.entry.exclude,
        vec![
            rule("{INSTALL_DIR}/preferences/STSGameplaySettings", Some(Platform::Macos), None),
            any("{INSTALL_DIR}/save/dc_options.json"),
        ]
    );
}

#[test]
fn paths_are_kept_exactly() {
    let t = translate_one(
        r#"
Game:
  files:
    "<base>/data/save_data.xml":
      tags: [save]
    "<home>/Library/Application Support/com.vlambeer.nuclearthrone":
      tags: [save]
      when: [{ os: mac }]
    "<base>/save/user_*.dat":
      tags: [save]
    "<winAppData>/Game/Profiles/":
      tags: [save]
      when: [{ os: windows }]
    "<winAppData>\\Game\\slot?.[ab]":
      tags: [save]
      when: [{ os: windows }]
"#,
    );
    assert_eq!(
        t.entry.save,
        vec![
            any("{INSTALL_DIR}/data/save_data.xml"),
            any("{INSTALL_DIR}/save/user_*.dat"),
            rule("{HOME}/Library/Application Support/com.vlambeer.nuclearthrone", Some(Platform::Macos), None),
            // Trailing slash is insignificant; backslashes become `/`.
            win("{APPDATA}/Game/Profiles"),
            win("{APPDATA}/Game/slot?.[ab]"),
        ]
    );
}

#[test]
fn base_and_steam_common_spellings_are_one_path() {
    let t = translate_one(
        r#"
Game:
  installDir:
    GameDir: {}
  files:
    "<base>/Saves":
      tags: [save]
      when: [{ os: windows }]
    "<root>/steamapps/common/GameDir/Saves":
      tags: [save]
      when: [{ store: steam }]
    "<root>/steamapps/common/<game>/Saves/":
      tags: [save]
      when: [{ store: steam }]
    "<root>/steamapps/common/gamedir/Only-Here":
      tags: [save]
      when: [{ store: steam }]
"#,
    );
    assert_eq!(t.entry.save, vec![win("{INSTALL_DIR}/Saves"), steam("{INSTALL_DIR}/Only-Here")]);
}

#[test]
fn placeholders_map_to_bundle_placeholders() {
    let t = translate_one(
        r#"
Game:
  files:
    "<home>/.game/a": {}
    "<winAppData>/G/b": {}
    "<winLocalAppData>/G/c": {}
    "<winLocalAppDataLow>/G/d": {}
    "<winDocuments>/G/e": {}
    "<winPublic>/G/f": {}
    "<winProgramData>/G/g": {}
    "<winDir>/G/h": {}
    "<xdgData>/G/i": {}
    "<xdgConfig>/G/j": {}
"#,
    );
    let paths: Vec<&str> = t.entry.save.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "{HOME}/.game/a",
            "{APPDATA}/G/b",
            "{WINDIR}/G/h",
            "{DOCUMENTS}/G/e",
            "{LOCALAPPDATA}/G/c",
            "{LOCALLOW}/G/d",
            "{PROGRAMDATA}/G/g",
            "{PUBLIC}/G/f",
            "{XDG_CONFIG_HOME}/G/j",
            "{XDG_DATA_HOME}/G/i",
        ]
    );
}

#[test]
fn store_user_id_becomes_both_steam_forms() {
    // Jagged Alliance 3's per-account folder.
    let t = translate_one(
        r#"
Game:
  steam:
    id: 1084160
  files:
    "<home>/Saved Games/Jagged Alliance 3/<storeUserId>/*.sav":
      tags: [save]
      when: [{ os: windows }]
    "<winAppData>/G/user_<storeUserId>":
      tags: [save]
      when: [{ os: windows, store: gog }]
"#,
    );
    assert_eq!(
        t.entry.save,
        vec![
            win("{HOME}/Saved Games/Jagged Alliance 3/{STEAM_ID64}/*.sav"),
            win("{HOME}/Saved Games/Jagged Alliance 3/{STEAM_ACCOUNT_ID}/*.sav"),
        ]
    );
    // A GOG account id has no Steam spelling: dropped with a warning.
    assert_eq!(t.dropped.len(), 1);
    assert_eq!(t.dropped[0].reason, DropReason::UnsupportedPath);
}

#[test]
fn steam_userdata_maps_to_the_userdata_placeholder() {
    let t = translate_one(
        r#"
Game:
  steam:
    id: 588650
  files:
    "<root>/userdata/<storeUserId>/588650/remote/user_*.dat":
      tags: [save]
      when: [{ store: steam }]
    "<root>/userdata/<storeUserId>/<storeGameId>/remote/other":
      tags: [save]
      when: [{ store: steam }]
    "<root>/userdata/<storeUserId>/<storeGameId>/remote/options.cfg":
      tags: [config]
      when: [{ store: steam }]
    "<winAppData>/Game/<storeGameId>":
      tags: [save]
      when: [{ os: windows }]
"#,
    );
    assert_eq!(
        t.entry.save,
        vec![
            steam("{STEAM_USERDATA}/588650/remote/user_*.dat"),
            steam("{STEAM_USERDATA}/588650/remote/other"),
            win("{APPDATA}/Game/588650"),
        ]
    );
    // Inside `user_*.dat`'s fixed part, `{STEAM_USERDATA}/588650/remote`.
    assert_eq!(t.entry.exclude, vec![steam("{STEAM_USERDATA}/588650/remote/options.cfg")]);
}

#[test]
fn broad_targets_drop_but_exact_names_inside_broad_folders_stay() {
    let t = translate_one(
        r#"
Game:
  files:
    "<base>/save*":
      tags: [save]
      when: [{ os: windows }]
    "<home>/Library/Application Support/*":
      tags: [save]
      when: [{ os: mac }]
    "<winDocuments>/My Games":
      tags: [save]
    "<home>/Library/Application Support/com.vlambeer.nuclearthrone":
      tags: [save]
      when: [{ os: mac }]
    "<root>/userdata/<storeUserId>":
      tags: [save]
"#,
    );
    assert_eq!(
        t.entry.save,
        vec![rule("{HOME}/Library/Application Support/com.vlambeer.nuclearthrone", Some(Platform::Macos), None)]
    );
    let broad: Vec<&str> =
        t.dropped.iter().filter(|d| d.reason == DropReason::Broad).map(|d| d.manifest_path.as_str()).collect();
    assert_eq!(
        broad,
        vec![
            "<base>/save*",
            "<home>/Library/Application Support/*",
            "<root>/userdata/<storeUserId>",
            "<winDocuments>/My Games"
        ]
    );
    assert!(t.dropped.iter().all(|d| d.reason.warns()));
}

#[test]
fn unresolvable_forms_drop_with_a_reason() {
    let t = translate_one(
        r#"
Game:
  files:
    "<root>/savegames/<storeUserId>/3353":
      tags: [save]
    "<root>/steamapps/compatdata/1466640/remote/pfx":
      tags: [save]
    "<home>/.macromedia/Flash_Player/#SharedObjects/<storeUserId>/<base>/Game/s.sol":
      tags: [save]
    "<winLocalAppData>/GOG.com/Galaxy/Applications/5059/Storage/Shared/Files/SGS*":
      tags: [save]
    "<winAppData>/<osUserName>/save":
      tags: [save]
"#,
    );
    assert!(t.entry.save.is_empty());
    assert_eq!(t.dropped.len(), 5);
    assert!(t.dropped.iter().all(|d| d.reason == DropReason::UnsupportedPath));
    assert_eq!(t.failure, vec![Category::UnsupportedPathForm]);
}

#[test]
fn ms_store_only_entries_are_dropped_silently() {
    let t = translate_one(
        r#"
Game:
  files:
    "<winLocalAppData>/Packages/Game_x/LocalCache":
      tags: [save]
      when: [{ os: windows, store: microsoft }]
    "<winAppData>/Game/uplay":
      tags: [save]
      when: [{ store: uplay }]
    "<winAppData>/Game/mixed":
      tags: [save]
      when: [{ os: windows, store: microsoft }, { os: windows }, { os: dos }]
"#,
    );
    assert_eq!(t.entry.save, vec![win("{APPDATA}/Game/mixed")]);
    assert!(t.dropped.iter().all(|d| d.reason == DropReason::UnsupportedStore && !d.reason.warns()));

    let only = translate_one(
        r#"
Game:
  files:
    "<winLocalAppData>/Packages/Game_x/LocalCache":
      tags: [save]
      when: [{ os: windows, store: microsoft }]
"#,
    );
    assert_eq!(only.failure, vec![Category::MsStoreOnly]);
}

#[test]
fn failure_categories() {
    assert_eq!(translate_one("Game:\n  steam:\n    id: 1\n").failure, vec![Category::NoFiles]);
    let config_only = translate_one("Game:\n  files:\n    \"<base>/setup.ini\":\n      tags: [config]\n");
    assert_eq!(config_only.failure, vec![Category::ConfigOnly]);
    let broad = translate_one("Game:\n  files:\n    \"<base>/save*\": {}\n");
    assert_eq!(broad.failure, vec![Category::BroadFolderOnly]);
    let userdata = translate_one("Game:\n  files:\n    \"<root>/userdata/<storeUserId>/<storeGameId>/remote\": {}\n");
    assert_eq!(userdata.failure, vec![Category::UserdataOnly]);
}

#[test]
fn conditions_are_kept_per_target_and_all_oses_collapse() {
    let t = translate_one(
        r#"
Game:
  files:
    "<base>/save/user_*.dat":
      tags: [save]
      when: [{ os: windows }, { os: mac }, { os: linux }]
    "<winAppData>/Game":
      tags: [save]
      when: [{ os: windows, store: steam, bit: 64 }, { os: windows, store: steam, bit: 32 }, { os: windows, store: gog }]
    "<xdgData>/Game":
      tags: [save]
      when: [{ os: linux }, { os: linux, store: steam }]
"#,
    );
    assert_eq!(
        t.entry.save,
        vec![
            any("{INSTALL_DIR}/save/user_*.dat"),
            rule("{APPDATA}/Game", Some(Platform::Windows), Some(Store::Steam)),
            rule("{APPDATA}/Game", Some(Platform::Windows), Some(Store::Gog)),
            rule("{XDG_DATA_HOME}/Game", Some(Platform::Linux), None),
        ]
    );
}

#[test]
fn launch_becomes_executables_per_os() {
    let t = translate_one(
        r#"
Game:
  files:
    "<base>/save": {}
  launch:
    "<base>/Game.exe":
      - when: [{ os: windows, store: steam, bit: 64 }]
        arguments: "-x"
      - when: [{ os: windows, store: gog }]
    "<base>/Game.app":
      - when: [{ os: mac }]
    "<base>/run.sh":
      - when: [{ os: linux }]
    "<base>/tool.exe": []
    "<base>/data/encyclopedia/how_to_play.html":
      - when: [{ os: windows }]
    "<base>/manual.pdf":
      - when: [{ os: windows }]
    "C:/elsewhere/other.exe":
      - when: [{ os: windows }]
"#,
    );
    let exes = &t.entry.executables;
    assert_eq!(exes.windows, vec!["Game.exe", "tool.exe"]);
    assert_eq!(exes.macos, vec!["Game.app", "tool.exe"]);
    assert_eq!(exes.linux, vec!["run.sh", "tool.exe"]);
    let dropped: Vec<&str> = t.dropped_launch.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        dropped,
        vec!["<base>/data/encyclopedia/how_to_play.html", "<base>/manual.pdf", "C:/elsewhere/other.exe"]
    );
}

#[test]
fn detect_and_install_dirs() {
    let t = translate_one(
        r#"
Game:
  steam:
    id: 10
  gog:
    id: 20
  id:
    steamExtra: [11, 10]
    gogExtra: [21]
    lutris: game
  installDir:
    Game Dir: {}
    Other: {}
  files:
    "<base>/save": {}
"#,
    );
    assert_eq!(t.entry.detect.steam, vec![10, 11]);
    assert_eq!(t.entry.detect.gog, vec![20, 21]);
    assert_eq!(t.entry.install_dirs, vec!["Game Dir", "Other"]);
}
