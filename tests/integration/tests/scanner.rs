use savescummer_scanner::*;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
struct DiscoveryFixture {
    steam: PathBuf,
    appdata: PathBuf,
}
impl Discovery for DiscoveryFixture {
    fn steam_roots(&self) -> Vec<PathBuf> {
        vec![self.steam.clone(), self.steam.clone()]
    }
    fn known_folders(&self) -> BTreeMap<String, PathBuf> {
        [("APPDATA".into(), self.appdata.clone())]
            .into_iter()
            .collect()
    }
    fn platform(&self) -> &'static str {
        "windows"
    }
}
#[test]
fn steam_scan_uses_all_libraries_app_ids_and_real_executables_without_requiring_saves() {
    let temp = tempfile::tempdir().unwrap();
    let steam = temp.path().join("Steam");
    let library = temp.path().join("Library é");
    let appdata = temp.path().join("AppData");
    fs::create_dir_all(steam.join("steamapps")).unwrap();
    fs::create_dir_all(library.join("steamapps/common/Actual folder")).unwrap();
    let escaped = library.display().to_string().replace('\\', "\\\\");
    fs::write(
        steam.join("steamapps/libraryfolders.vdf"),
        format!(r#""libraryfolders" {{ "1" {{ "path" "{escaped}" }} }}"#),
    )
    .unwrap();
    fs::write(library.join("steamapps/appmanifest_2853590.acf"), r#""AppState" { "appid" "2853590" "name" "Different display name" "installdir" "Actual folder" }"#).unwrap();
    let exe = library.join("steamapps/common/Actual folder/Void War.exe");
    fs::write(&exe, b"fixture").unwrap();
    let definition = parse_catalog(include_str!("../../../catalog/games/void-war.yaml")).unwrap();
    let discovery = DiscoveryFixture {
        steam,
        appdata: appdata.clone(),
    };
    let result = scan(std::slice::from_ref(&definition), &discovery);
    assert!(result.errors.is_empty());
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].executables, std::slice::from_ref(&exe));
    assert_eq!(result.candidates[0].data_dir, appdata.join("Void_War"));
    assert!(!appdata.exists());
    fs::remove_file(exe).unwrap();
    let result = scan(&[definition], &discovery);
    assert!(result.candidates.is_empty());
    assert_eq!(result.errors.len(), 1);
}

fn install_fixture(library: &Path) {
    fs::create_dir_all(library.join("steamapps/common/Game")).unwrap();
    fs::write(
        library.join("steamapps/common/Game/Void War.exe"),
        b"fixture",
    )
    .unwrap();
    fs::write(
        library.join("steamapps/appmanifest_2853590.acf"),
        r#""AppState" { "appid" "2853590" "installdir" "Game" }"#,
    )
    .unwrap();
}

fn scan_with_library_alias(steam: &Path, alias: &Path) -> Scan {
    let escaped = alias.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        steam.join("steamapps/libraryfolders.vdf"),
        format!(r#""libraryfolders" {{ "0" {{ "path" "{escaped}" }} }}"#),
    )
    .unwrap();
    let definition = parse_catalog(include_str!("../../../catalog/games/void-war.yaml")).unwrap();
    scan(
        &[definition],
        &DiscoveryFixture {
            steam: steam.to_path_buf(),
            appdata: steam.join("AppData"),
        },
    )
}

#[test]
fn case_only_installation_paths_follow_the_filesystems_case_sensitivity() {
    let temp = tempfile::tempdir().unwrap();
    let steam = temp.path().join("Steam");
    let alias = temp.path().join("steam");
    install_fixture(&steam);
    // Probe the actual filesystem rather than assuming a host OS implies a
    // particular case policy (Windows and macOS both support either policy).
    let expected = match fs::create_dir(&alias) {
        Ok(()) => {
            install_fixture(&alias);
            2
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => 1,
        Err(e) => panic!("cannot probe filesystem case sensitivity: {e}"),
    };
    let result = scan_with_library_alias(&steam, &alias);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.candidates.len(), expected);
}

#[test]
fn distinct_steam_installations_remain_separate_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let steam = temp.path().join("Steam");
    let other = temp.path().join("Other library");
    install_fixture(&steam);
    install_fixture(&other);
    let result = scan_with_library_alias(&steam, &other);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.candidates.len(), 2);
}

#[cfg(unix)]
#[test]
fn symlinked_steam_installations_are_one_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let steam = temp.path().join("Steam");
    let alias = temp.path().join("Steam alias");
    install_fixture(&steam);
    std::os::unix::fs::symlink(&steam, &alias).unwrap();
    let result = scan_with_library_alias(&steam, &alias);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.candidates.len(), 1);
}

#[test]
fn uninstall_fallback_and_declared_paths_deduplicate_and_keep_data_choices() {
    struct Records {
        root: PathBuf,
        records: Vec<ApplicationRecord>,
    }
    impl Discovery for Records {
        fn steam_roots(&self) -> Vec<PathBuf> {
            vec![]
        }
        fn known_folders(&self) -> BTreeMap<String, PathBuf> {
            [("HOME".into(), self.root.clone())].into()
        }
        fn platform(&self) -> &'static str {
            "windows"
        }
        fn applications(&self) -> ApplicationScan {
            ApplicationScan {
                records: self.records.clone(),
                errors: vec![],
            }
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let install = temp.path().join("Game");
    fs::create_dir(&install).unwrap();
    fs::write(install.join("game.exe"), "executable").unwrap();
    let definition = parse_catalog(
        r#"
id: game
name: Game
stores:
  steam: 123
platforms:
  windows:
    executables: [game.exe]
    data_dir: '{HOME}/saves'
    alternative_data_dirs: ['{HOME}/other-saves']
    known_install_dirs: ['{HOME}/Game']
    registry_keys: [ExactVendorKey]
"#,
    )
    .unwrap();
    let records = Records {
        root: temp.path().into(),
        records: vec![
            ApplicationRecord {
                key: "Steam App 123".into(),
                install_dir: install.clone(),
            },
            ApplicationRecord {
                key: "ExactVendorKey".into(),
                install_dir: install.clone(),
            },
        ],
    };
    let result = scan(std::slice::from_ref(&definition), &records);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.candidates.len(), 2);
    assert!(!temp.path().join("saves").exists());
    let mut definition = definition;
    definition
        .platforms
        .get_mut("windows")
        .unwrap()
        .known_install_dirs
        .clear();
    let result = scan(std::slice::from_ref(&definition), &records);
    assert_eq!(result.candidates.len(), 2);
    let mismatch = Records {
        root: temp.path().into(),
        records: vec![ApplicationRecord {
            key: "Game".into(),
            install_dir: install,
        }],
    };
    assert!(scan(&[definition], &mismatch).candidates.is_empty());
}
