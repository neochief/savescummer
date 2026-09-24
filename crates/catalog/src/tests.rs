//! Resolver tests: one named test per PLAN-CATALOG.md Section 5 case, plus
//! the extra behaviors listed in 7.2. Everything runs on an in-memory probe.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::*;

#[derive(Default)]
struct FakeProbe {
    files: BTreeSet<PathBuf>,
    dirs: BTreeSet<PathBuf>,
    unknown: BTreeSet<PathBuf>,
    /// Alias → real path (a junction, a redirected folder).
    aliases: BTreeMap<PathBuf, PathBuf>,
    identities: BTreeMap<PathBuf, String>,
    account: Option<SteamAccount>,
    folders: KnownFolders,
    case_insensitive: bool,
    calls: RefCell<usize>,
}

impl FakeProbe {
    fn windows() -> Self {
        FakeProbe {
            folders: KnownFolders {
                home: Some("C:/Users/u".into()),
                appdata: Some("C:/Users/u/AppData/Roaming".into()),
                localappdata: Some("C:/Users/u/AppData/Local".into()),
                locallow: Some("C:/Users/u/AppData/LocalLow".into()),
                documents: Some("C:/Users/u/Documents".into()),
                public: Some("C:/Users/Public".into()),
                programdata: Some("C:/ProgramData".into()),
                programfiles: Some("C:/Program Files".into()),
                windir: Some("C:/Windows".into()),
                steam_root: Some("C:/Steam".into()),
                ..Default::default()
            },
            account: Some(SteamAccount { account_id: 44258119 }),
            case_insensitive: true,
            ..Default::default()
        }
    }

    fn linux() -> Self {
        FakeProbe {
            folders: KnownFolders {
                home: Some("/home/u".into()),
                xdg_data_home: Some("/home/u/.local/share".into()),
                xdg_config_home: Some("/home/u/.config".into()),
                steam_root: Some("/home/u/.steam/steam".into()),
                ..Default::default()
            },
            account: Some(SteamAccount { account_id: 44258119 }),
            ..Default::default()
        }
    }

    fn file(mut self, path: &str) -> Self {
        let path = PathBuf::from(path);
        for ancestor in path.ancestors().skip(1) {
            self.dirs.insert(ancestor.to_path_buf());
        }
        self.files.insert(path);
        self
    }

    fn dir(mut self, path: &str) -> Self {
        for ancestor in Path::new(path).ancestors() {
            self.dirs.insert(ancestor.to_path_buf());
        }
        self
    }

    fn real(&self, path: &Path) -> String {
        let mut path = path.to_path_buf();
        for (alias, real) in &self.aliases {
            if let Ok(rest) = path.strip_prefix(alias) {
                path = real.join(rest);
            }
        }
        let text = path.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string();
        if self.case_insensitive { text.to_lowercase() } else { text }
    }
}

impl Probe for FakeProbe {
    fn presence(&self, path: &Path) -> Presence {
        *self.calls.borrow_mut() += 1;
        if self.unknown.iter().any(|u| path.starts_with(u)) {
            return Presence::Unknown;
        }
        let real = self.real(path);
        let present = self.dirs.iter().chain(&self.files).any(|p| self.real(p) == real);
        if present { Presence::Present } else { Presence::Missing }
    }

    fn is_file(&self, path: &Path) -> bool {
        let real = self.real(path);
        self.files.iter().any(|p| self.real(p) == real)
    }

    fn same_dir(&self, a: &Path, b: &Path) -> bool {
        self.real(a) == self.real(b)
    }

    fn install_identity(&self, install_dir: &Path) -> Option<String> {
        self.identities.get(install_dir).cloned()
    }

    fn steam_account(&self) -> Option<SteamAccount> {
        self.account
    }

    fn folders(&self) -> KnownFolders {
        self.folders.clone()
    }

    fn list_dirs(&self, path: &Path) -> Option<Vec<String>> {
        let names: Vec<String> = self
            .dirs
            .iter()
            .filter(|d| d.parent() == Some(path))
            .filter_map(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        Some(names)
    }
}

fn game(id: &str, save: &[PathRule]) -> Game {
    Game {
        id: id.into(),
        name: id.into(),
        info: None,
        detect: Detect::default(),
        install_dirs: vec![],
        executables: Executables::default(),
        save: save.to_vec(),
        exclude: vec![],
    }
}

fn rule(path: &str) -> PathRule {
    PathRule::new(path)
}

fn win(path: &str) -> PathRule {
    PathRule::new(path).when(Some(Platform::Windows), None)
}

fn lin(path: &str) -> PathRule {
    PathRule::new(path).when(Some(Platform::Linux), None)
}

fn steam(path: &str) -> PathRule {
    PathRule::new(path).when(None, Some(Store::Steam))
}

fn install(store: Store, os: Platform, dir: &str) -> Install {
    Install { catalog_id: "g".into(), store, os, install_dir: dir.into(), proton_prefix: None }
}

fn proton(dir: &str, prefix: &str) -> Install {
    Install {
        catalog_id: "g".into(),
        store: Store::Steam,
        os: Platform::Linux,
        install_dir: dir.into(),
        proton_prefix: Some(prefix.into()),
    }
}

fn set(decision: &Decision) -> Vec<(String, Filter)> {
    decision
        .save_set()
        .expect("resolved")
        .iter()
        .map(|t| (t.root.to_string_lossy().replace('\\', "/"), t.filter.clone()))
        .collect()
}

fn exact(name: &str) -> Filter {
    Filter::Exact(name.into())
}

fn pattern(p: &str) -> Filter {
    Filter::Pattern(p.into())
}

// ---- Section 5 cases -------------------------------------------------------

#[test]
fn case_01_single_target() {
    let g = game("hades", &[win("{DOCUMENTS}/Saved Games/Hades")]);
    let probe = FakeProbe::windows().dir("C:/Users/u/Documents/Saved Games/Hades");
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "C:/Steam/steamapps/common/Hades"), &probe);
    assert_eq!(set(&d), vec![("C:/Users/u/Documents/Saved Games".into(), exact("Hades"))]);
}

#[test]
fn case_02_several_folders_one_save() {
    let g = game(
        "sts",
        &[
            win("{INSTALL_DIR}/saves"),
            win("{INSTALL_DIR}/preferences"),
            win("{INSTALL_DIR}/runs"),
            win("{INSTALL_DIR}/betaPreferences"),
        ],
    );
    let probe = FakeProbe::windows();
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/STS"), &probe);
    let names: Vec<Filter> = set(&d).into_iter().map(|(_, f)| f).collect();
    assert_eq!(names, vec![exact("saves"), exact("preferences"), exact("runs"), exact("betaPreferences")]);
}

#[test]
fn case_03_several_folders_of_different_kinds() {
    let g = game("hf", &[win("{INSTALL_DIR}/Saves"), win("{INSTALL_DIR}/SavesSkirmish"), win("{INSTALL_DIR}/Ships")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/HighFleet"), &FakeProbe::windows());
    assert_eq!(d.save_set().unwrap().len(), 3);
}

#[test]
fn case_04_old_and_new_paths_both_exist() {
    let g = game(
        "rw",
        &[
            win("{LOCALLOW}/Ludeon Studios/RimWorld/Saves"),
            win("{LOCALLOW}/Ludeon Studios/RimWorld by Ludeon Studios/Saves"),
        ],
    );
    let probe = FakeProbe::windows()
        .dir("C:/Users/u/AppData/LocalLow/Ludeon Studios/RimWorld/Saves")
        .dir("C:/Users/u/AppData/LocalLow/Ludeon Studios/RimWorld by Ludeon Studios/Saves");
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/RimWorld"), &probe);
    assert_eq!(d.save_set().unwrap().len(), 2);
    assert!(d.save_set().unwrap().iter().all(|t| t.presence == Presence::Present));
}

#[test]
fn case_05_old_and_new_only_one_exists() {
    let g = game("rw", &[win("{LOCALLOW}/Old/Saves"), win("{LOCALLOW}/New/Saves")]);
    let probe = FakeProbe::windows().dir("C:/Users/u/AppData/LocalLow/New/Saves");
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/RimWorld"), &probe);
    let presence: Vec<Presence> = d.save_set().unwrap().iter().map(|t| t.presence).collect();
    assert_eq!(presence, vec![Presence::Missing, Presence::Present]);
}

#[test]
fn case_06_store_scoped_paths() {
    let g = game(
        "returnal",
        &[
            PathRule::new("{LOCALAPPDATA}/Returnal/Steam/Saved").when(Some(Platform::Windows), Some(Store::Steam)),
            PathRule::new("{LOCALAPPDATA}/Returnal/Epic/Saved").when(Some(Platform::Windows), Some(Store::Epic)),
        ],
    );
    let probe = FakeProbe::windows();
    let s = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/R"), &probe);
    let e = resolve(&g, &install(Store::Epic, Platform::Windows, "D:/R"), &probe);
    assert!(set(&s)[0].0.contains("/Steam"));
    assert!(set(&e)[0].0.contains("/Epic"));
    assert_eq!(set(&s).len(), 1);
    assert_eq!(set(&e).len(), 1);
}

fn two_decisions(g: &Game, a: Install, b: Install, probe: &FakeProbe) -> Vec<GameRecord> {
    let decisions = vec![resolve(g, &a, probe), resolve(g, &b, probe)];
    assign_games(&decisions, probe, &HashMap::new())
}

#[test]
fn case_07_two_installs_distinct_folders() {
    let mut g = game("steam-588650", &[rule("{INSTALL_DIR}/save/user_*.dat")]);
    g.id = "steam-588650".into();
    let probe = FakeProbe::windows();
    let mut a = install(Store::Steam, Platform::Windows, "C:/Steam/steamapps/common/Dead Cells");
    let mut b = install(Store::Gog, Platform::Windows, "C:/GOG Games/Dead Cells");
    a.catalog_id = g.id.clone();
    b.catalog_id = g.id.clone();
    let records = two_decisions(&g, b, a, &probe);
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["steam-588650", "steam-588650#gog"]);
    assert_eq!(records[0].installs[0].store, Store::Steam, "the store the id is named after keeps the bare id");
}

#[test]
fn case_08_two_installs_shared_folder() {
    let mut g = game("steam-1", &[rule("{APPDATA}/Game")]);
    g.executables.windows = vec!["Game.exe".into()];
    let probe = FakeProbe::windows();
    let records = two_decisions(
        &g,
        install(Store::Steam, Platform::Windows, "C:/Steam/steamapps/common/Game"),
        install(Store::Gog, Platform::Windows, "C:/GOG Games/Game"),
        &probe,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].installs.len(), 2);
    assert_eq!(records[0].executables.len(), 2, "both executable sets map to the one record");
}

#[test]
fn case_09_same_store_twice() {
    let g = game("steam-9", &[rule("{INSTALL_DIR}/save")]);
    let mut probe = FakeProbe::windows();
    probe.identities.insert("C:/Lib1/common/G".into(), "v1-f1".into());
    probe.identities.insert("D:/Lib2/common/G".into(), "v2-f2".into());
    let records = two_decisions(
        &g,
        install(Store::Steam, Platform::Windows, "D:/Lib2/common/G"),
        install(Store::Steam, Platform::Windows, "C:/Lib1/common/G"),
        &probe,
    );
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["steam-9", "steam-9#v2-f2"]);
    // The copy seen before keeps its id, whatever the sort order says.
    let mut known = HashMap::new();
    known.insert("steam-9".to_string(), "v2-f2".to_string());
    let decisions = vec![
        resolve(&g, &install(Store::Steam, Platform::Windows, "C:/Lib1/common/G"), &probe),
        resolve(&g, &install(Store::Steam, Platform::Windows, "D:/Lib2/common/G"), &probe),
    ];
    let records = assign_games(&decisions, &probe, &known);
    assert_eq!(records[0].installs[0].install_dir, PathBuf::from("D:/Lib2/common/G"));
    assert_eq!(records[0].id, "steam-9");
    assert_eq!(records[1].id, "steam-9#v1-f1");
}

fn coq() -> Game {
    let mut g = game(
        "steam-333640",
        &[
            win("{HOME}/AppData/LocalLow/Freehold Games/CavesOfQud/Saves"),
            lin("{XDG_CONFIG_HOME}/unity3d/Freehold Games/CavesOfQud/Saves"),
        ],
    );
    g.executables.windows = vec!["CoQ.exe".into()];
    g.executables.linux = vec!["CoQ.x86_64".into()];
    g
}

const COQ: &str = "/home/u/.steam/steam/steamapps/common/Caves of Qud";
const PFX: &str = "/home/u/.steam/steam/steamapps/compatdata/333640/pfx";

#[test]
fn case_10_linux_native() {
    let probe = FakeProbe::linux().file(&format!("{COQ}/CoQ.x86_64"));
    let d = resolve(&coq(), &proton(COQ, PFX), &probe);
    assert_eq!(d.context.builds, vec![Platform::Linux]);
    assert_eq!(set(&d), vec![("/home/u/.config/unity3d/Freehold Games/CavesOfQud".into(), exact("Saves"))]);
}

#[test]
fn case_11_linux_proton() {
    let probe = FakeProbe::linux().file(&format!("{COQ}/CoQ.exe")).dir(&format!("{PFX}/drive_c/users/steamuser"));
    let d = resolve(&coq(), &proton(COQ, PFX), &probe);
    assert_eq!(d.context.builds, vec![Platform::Windows]);
    assert_eq!(
        set(&d),
        vec![(format!("{PFX}/drive_c/users/steamuser/AppData/LocalLow/Freehold Games/CavesOfQud"), exact("Saves"))]
    );
}

#[test]
fn case_12_proton_before_first_launch() {
    let probe = FakeProbe::linux().file(&format!("{COQ}/CoQ.exe"));
    let d = resolve(&coq(), &proton(COQ, PFX), &probe);
    assert_eq!(d.context.builds, vec![Platform::Windows]);
    let target = &d.save_set().unwrap()[0];
    assert!(target.root.starts_with(PFX), "prefix paths are computed anyway");
    assert_eq!(target.presence, Presence::Missing);
}

#[test]
fn case_13_stale_prefix() {
    let probe = FakeProbe::linux()
        .file(&format!("{COQ}/CoQ.x86_64"))
        .dir(&format!("{PFX}/drive_c/users/steamuser/AppData/LocalLow/Freehold Games/CavesOfQud/Saves"));
    let d = resolve(&coq(), &proton(COQ, PFX), &probe);
    assert_eq!(d.context.builds, vec![Platform::Linux]);
    assert!(!d.save_set().unwrap()[0].root.starts_with(PFX));
}

#[test]
fn case_14_files_cant_tell_the_build() {
    // No executables in the catalog.
    let mut eu4 = coq();
    eu4.executables = Executables::default();
    let probe = FakeProbe::linux();
    let d = resolve(&eu4, &proton(COQ, PFX), &probe);
    assert_eq!(d.context.builds, vec![Platform::Linux, Platform::Windows]);
    assert_eq!(d.save_set().unwrap().len(), 2);
    // The same file listed for both builds.
    let mut vs = coq();
    vs.executables.linux = vec!["CoQ.exe".into()];
    let d = resolve(&vs, &proton(COQ, PFX), &FakeProbe::linux().file(&format!("{COQ}/CoQ.exe")));
    assert_eq!(d.context.builds.len(), 2);
    // Both builds' files present.
    let probe = FakeProbe::linux().file(&format!("{COQ}/CoQ.exe")).file(&format!("{COQ}/CoQ.x86_64"));
    let d = resolve(&coq(), &proton(COQ, PFX), &probe);
    assert_eq!(d.context.builds.len(), 2);
    assert_eq!(d.executables.len(), 2, "one record whose executables cover both builds");
    let records = assign_games(&[d], &probe, &HashMap::new());
    assert_eq!(records.len(), 1);
}

#[test]
fn case_15_build_switch_both_folders_exist() {
    let probe = FakeProbe::linux()
        .file(&format!("{COQ}/CoQ.exe"))
        .dir("/home/u/.config/unity3d/Freehold Games/CavesOfQud/Saves");
    let d = resolve(&coq(), &proton(COQ, PFX), &probe);
    assert_eq!(d.context.builds, vec![Platform::Windows]);
    assert!(d.save_set().unwrap()[0].root.starts_with(PFX), "the native folder is ignored after the switch");
}

#[test]
fn case_16_switching_back() {
    let native = FakeProbe::linux().file(&format!("{COQ}/CoQ.x86_64"));
    let protonp = FakeProbe::linux().file(&format!("{COQ}/CoQ.exe"));
    let a = resolve(&coq(), &proton(COQ, PFX), &native);
    let b = resolve(&coq(), &proton(COQ, PFX), &protonp);
    let c = resolve(&coq(), &proton(COQ, PFX), &native);
    assert_ne!(a.save_set(), b.save_set());
    assert_eq!(a.save_set(), c.save_set(), "the native targets return exactly");
    assert_eq!(a.context, c.context);
}

#[test]
fn case_17_local_copy_and_cloud_copy() {
    let g = game(
        "steam-1337520",
        &[win("{APPDATA}/Risk_of_Rain_Returns/{STEAM_ID64}_localsave.json"), steam("{STEAM_USERDATA}/1337520/remote")],
    );
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/RoRR"), &FakeProbe::windows());
    assert_eq!(
        set(&d),
        vec![
            ("C:/Users/u/AppData/Roaming/Risk_of_Rain_Returns".into(), exact("76561198004523847_localsave.json")),
            ("C:/Steam/userdata/44258119/1337520".into(), exact("remote")),
        ]
    );
}

#[test]
fn case_18_fresh_install_no_saves_yet() {
    let g = game("g", &[win("{APPDATA}/Game/saves")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &FakeProbe::windows());
    assert_eq!(d.save_set().unwrap()[0].presence, Presence::Missing);
    assert!(matches!(d.outcome, Outcome::Resolved { .. }), "nothing is provisional");
}

#[test]
fn case_19_saves_only_in_steam_userdata() {
    let g = game("steam-632360", &[steam("{STEAM_USERDATA}/632360/remote/UserProfiles")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/RoR2"), &FakeProbe::windows());
    assert_eq!(set(&d), vec![("C:/Steam/userdata/44258119/632360/remote".into(), exact("UserProfiles"))]);
}

#[test]
fn case_20_upstream_rename_keeps_the_id() {
    let mut before = game("steam-5", &[rule("{APPDATA}/G")]);
    before.name = "Old Name".into();
    let mut after = before.clone();
    after.name = "New Name".into();
    let probe = FakeProbe::windows();
    let i = Install { catalog_id: "steam-5".into(), ..install(Store::Steam, Platform::Windows, "D:/G") };
    let a = assign_games(&[resolve(&before, &i, &probe)], &probe, &HashMap::new());
    let b = assign_games(&[resolve(&after, &i, &probe)], &probe, &HashMap::new());
    assert_eq!(a[0].id, b[0].id);
    assert_eq!(a[0].outcome, b[0].outcome);
}

#[test]
fn case_21_addendum_game_lands_upstream() {
    // The builder's concern; the resolver only sees the resulting entry, so
    // the same entry resolves the same way whichever source produced it.
    let g = game("steam-2853590", &[win("{APPDATA}/Void_War/*.sav")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/VW"), &FakeProbe::windows());
    assert_eq!(set(&d), vec![("C:/Users/u/AppData/Roaming/Void_War".into(), pattern("*.sav"))]);
}

#[test]
fn case_22_catalog_update_changes_a_path() {
    let before = game("g", &[win("{APPDATA}/G/a"), win("{APPDATA}/G/b")]);
    let after = game("g", &[win("{APPDATA}/G/a"), win("{APPDATA}/G/c")]);
    let probe = FakeProbe::windows();
    let i = install(Store::Steam, Platform::Windows, "D:/G");
    let a = set(&resolve(&before, &i, &probe));
    let b = set(&resolve(&after, &i, &probe));
    assert_eq!(a[0], b[0], "the common target stays");
    assert_ne!(a[1], b[1]);
}

#[test]
fn case_23_save_folder_on_an_unplugged_drive() {
    let g = game("g", &[rule("{INSTALL_DIR}/save")]);
    let mut probe = FakeProbe::windows();
    probe.unknown.insert("E:/".into());
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "E:/SteamLibrary/steamapps/common/G"), &probe);
    assert_eq!(d.save_set().unwrap()[0].presence, Presence::Unknown);
}

#[test]
fn case_24_user_deleted_their_saves() {
    let g = game("rw", &[win("{LOCALLOW}/Ludeon/RimWorld/Saves")]);
    let probe = FakeProbe::windows().dir("C:/Users/u/AppData/LocalLow/Ludeon");
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/RW"), &probe);
    assert_eq!(d.save_set().unwrap()[0].presence, Presence::Missing);
}

#[test]
fn case_25_game_update_moved_its_saves() {
    let g = game("g", &[win("{APPDATA}/G/old"), win("{LOCALAPPDATA}/G/new")]);
    let probe = FakeProbe::windows().dir("C:/Users/u/AppData/Roaming/G/old");
    let first = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &probe);
    let probe = probe.dir("C:/Users/u/AppData/Local/G/new");
    let later = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &probe);
    assert_eq!(set(&first), set(&later), "both paths are targets all along");
    assert_eq!(later.save_set().unwrap()[1].presence, Presence::Present);
}

#[test]
fn case_26_steam_account_switch() {
    let g = game("g", &[steam("{STEAM_USERDATA}/1/remote"), win("{APPDATA}/G")]);
    let mut probe = FakeProbe::windows();
    let i = install(Store::Steam, Platform::Windows, "D:/G");
    let a = resolve(&g, &i, &probe);
    probe.account = Some(SteamAccount { account_id: 1 });
    let b = resolve(&g, &i, &probe);
    assert_ne!(set(&a)[0], set(&b)[0]);
    assert_eq!(set(&a)[1], set(&b)[1], "account-independent targets stay in common");
    assert_ne!(a.context, b.context);
}

#[test]
fn case_27_steam_copy_plus_epic_copy() {
    let g = game("steam-3", &[rule("{INSTALL_DIR}/save")]);
    let probe = FakeProbe::windows();
    let records = two_decisions(
        &g,
        install(Store::Steam, Platform::Windows, "C:/Steam/steamapps/common/G"),
        install(Store::Epic, Platform::Windows, "C:/Program Files/Epic Games/G"),
        &probe,
    );
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["steam-3", "steam-3#epic"]);
}

#[test]
fn case_28_install_moved_to_another_library() {
    let g = game("steam-4", &[rule("{INSTALL_DIR}/save")]);
    let mut probe = FakeProbe::windows();
    probe.identities.insert("C:/L/G".into(), "old".into());
    probe.identities.insert("E:/L/G".into(), "new".into());
    let mut known = HashMap::new();
    known.insert("steam-4".to_string(), "old".to_string());
    let moved = resolve(&g, &install(Store::Steam, Platform::Windows, "E:/L/G"), &probe);
    let records = assign_games(&[moved], &probe, &known);
    assert_eq!(records[0].id, "steam-4", "one copy of the product is the same install");
}

#[test]
fn case_29_user_override_bypasses_the_resolver() {
    // An override is a user location split like a catalog path; the
    // resolver is never consulted for it.
    let target = split_location(Path::new("D:/Game/saves/*.sav")).unwrap();
    assert_eq!(target.filter, pattern("*.sav"));
    assert_eq!(target.root, PathBuf::from("D:/Game/saves"));
    let file = split_location(Path::new("D:/Game/saves/slot1.sav")).unwrap();
    assert_eq!(file.filter, exact("slot1.sav"));
}

#[test]
fn case_30_no_applicable_target() {
    let mut g = game("g", &[win("{APPDATA}/G")]);
    g.executables.linux = vec!["g.x86_64".into()];
    let d = resolve(&g, &install(Store::Steam, Platform::Linux, "/games/G"), &FakeProbe::linux());
    assert!(matches!(d.outcome, Outcome::Unsupported { .. }));
    assert_eq!(d.executables, vec![PathBuf::from("/games/G/g.x86_64")], "the game stays recognizable");
}

#[test]
fn case_31_steam_account_unknown() {
    let g = game("g", &[steam("{STEAM_USERDATA}/1/remote"), win("{APPDATA}/G")]);
    let mut probe = FakeProbe::windows();
    probe.account = None;
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &probe);
    assert_eq!(set(&d), vec![("C:/Users/u/AppData/Roaming".into(), exact("G"))]);
    assert!(d.warnings.iter().any(|w| w.contains("Steam account")));
}

#[test]
fn case_32_custom_proton_profile() {
    let probe = FakeProbe::linux()
        .file(&format!("{COQ}/CoQ.exe"))
        .dir(&format!("{PFX}/drive_c/users/steamuser"))
        .dir(&format!("{PFX}/drive_c/users/Public"))
        .dir(&format!("{PFX}/drive_c/users/deck"));
    let d = resolve(&coq(), &proton(COQ, PFX), &probe);
    assert!(set(&d)[0].0.contains("/drive_c/users/deck/AppData/LocalLow"));
}

#[test]
fn case_33_one_folder_spelled_two_ways() {
    let g = game("g", &[win("{HOME}/AppData/Roaming/G"), win("{APPDATA}/g"), win("{HOME}/Docs/G")]);
    let mut probe = FakeProbe::windows();
    probe.aliases.insert("C:/Users/u/Docs".into(), "C:/Users/u/AppData/Roaming".into());
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &probe);
    assert_eq!(d.save_set().unwrap().len(), 1, "case and a junction don't make a second target");

    let g = game("g", &[lin("{HOME}/Game/saves"), lin("{HOME}/game/saves")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Linux, "/g"), &FakeProbe::linux());
    assert_eq!(d.save_set().unwrap().len(), 2, "on Linux case-different folders stay distinct");
}

#[test]
fn case_34_folder_with_a_dot_in_a_shared_place() {
    let g = game(
        "nt",
        &[PathRule::new("{HOME}/Library/Application Support/com.vlambeer.nuclearthrone")
            .when(Some(Platform::Macos), None)],
    );
    let mut probe = FakeProbe::linux();
    probe.folders.home = Some("/Users/u".into());
    let d = resolve(&g, &install(Store::Steam, Platform::Macos, "/Apps/NT"), &probe);
    assert_eq!(set(&d), vec![("/Users/u/Library/Application Support".into(), exact("com.vlambeer.nuclearthrone"))]);
}

#[test]
fn case_35_save_file_among_game_files() {
    let g = game("necro", &[rule("{INSTALL_DIR}/data/save_data.xml")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/NecroDancer"), &FakeProbe::windows());
    assert_eq!(set(&d), vec![("D:/NecroDancer/data".into(), exact("save_data.xml"))]);
}

#[test]
fn case_36_patterns() {
    let g = game("bt", &[rule("{INSTALL_DIR}/save/user_*.dat"), rule("{DOCUMENTS}/My Games/BattleTech/C*/SGS*")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &FakeProbe::windows());
    assert_eq!(
        set(&d),
        vec![
            ("D:/G/save".into(), pattern("user_*.dat")),
            ("C:/Users/u/Documents/My Games/BattleTech".into(), pattern("C*/SGS*")),
        ]
    );
}

#[test]
fn case_37_settings_inside_a_save_folder() {
    let mut g = game("dk", &[win("{APPDATA}/Godot/app_userdata/Dome Keeper/savegame*")]);
    g.save.push(win("{INSTALL_DIR}/profile"));
    g.exclude = vec![win("{INSTALL_DIR}/profile/options.txt"), win("{APPDATA}/Unrelated/x.cfg")];
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/DK"), &FakeProbe::windows());
    let targets = d.save_set().unwrap();
    assert_eq!(targets[1].excludes, vec!["profile/options.txt".to_string()]);
    assert!(targets[0].excludes.is_empty(), "an exclude attaches only to the target it falls inside");
}

#[test]
fn case_38_account_standing_wildcard_pinned() {
    let g = game("rorr", &[win("{APPDATA}/Risk_of_Rain_Returns/{STEAM_ID64}_localsave.json")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/R"), &FakeProbe::windows());
    assert_eq!(set(&d)[0].1, exact("76561198004523847_localsave.json"), "never a wildcard over accounts");
}

#[test]
fn case_39_two_spellings_of_the_steam_id() {
    let g = game(
        "ja3",
        &[
            win("{HOME}/Saved Games/Jagged Alliance 3/{STEAM_ID64}/*.sav"),
            win("{HOME}/Saved Games/Jagged Alliance 3/{STEAM_ACCOUNT_ID}/*.sav"),
        ],
    );
    let probe = FakeProbe::windows().dir("C:/Users/u/Saved Games/Jagged Alliance 3/76561198004523847");
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/JA3"), &probe);
    let presence: Vec<Presence> = d.save_set().unwrap().iter().map(|t| t.presence).collect();
    assert_eq!(presence, vec![Presence::Present, Presence::Missing], "the form that doesn't exist is simply absent");
}

#[test]
fn case_40_target_covered_by_another() {
    let g = game("g", &[rule("{APPDATA}/G/save/*.dat"), rule("{APPDATA}/G/save"), rule("{APPDATA}/G/save/sub/x")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &FakeProbe::windows());
    assert_eq!(set(&d), vec![("C:/Users/u/AppData/Roaming/G".into(), exact("save"))]);
}

// ---- Beyond the cases ------------------------------------------------------

#[test]
fn placeholders_per_build_windows() {
    let g = game(
        "g",
        &[
            win("{HOME}/h/x"),
            win("{APPDATA}/a/x"),
            win("{LOCALAPPDATA}/l/x"),
            win("{LOCALLOW}/ll/x"),
            win("{DOCUMENTS}/d/x"),
            win("{PUBLIC}/p/x"),
            win("{PROGRAMDATA}/pd/x"),
            win("{PROGRAMFILES}/pf/x"),
            win("{WINDIR}/w/x"),
        ],
    );
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &FakeProbe::windows());
    let roots: Vec<String> = set(&d).into_iter().map(|(r, _)| r).collect();
    assert_eq!(
        roots,
        vec![
            "C:/Users/u/h",
            "C:/Users/u/AppData/Roaming/a",
            "C:/Users/u/AppData/Local/l",
            "C:/Users/u/AppData/LocalLow/ll",
            "C:/Users/u/Documents/d",
            "C:/Users/Public/p",
            "C:/ProgramData/pd",
            "C:/Program Files/pf",
            "C:/Windows/w",
        ]
    );
}

#[test]
fn placeholders_inside_a_proton_prefix() {
    let g = game(
        "g",
        &[
            win("{HOME}/h/x"),
            win("{APPDATA}/a/x"),
            win("{LOCALAPPDATA}/l/x"),
            win("{LOCALLOW}/ll/x"),
            win("{DOCUMENTS}/d/x"),
            win("{PUBLIC}/p/x"),
            win("{PROGRAMDATA}/pd/x"),
            win("{PROGRAMFILES}/pf/x"),
            win("{WINDIR}/w/x"),
            win("{INSTALL_DIR}/i/x"),
        ],
    );
    let mut coq = coq();
    coq.save = g.save;
    let d = resolve(&coq, &proton(COQ, PFX), &FakeProbe::linux().file(&format!("{COQ}/CoQ.exe")));
    let roots: Vec<String> = set(&d).into_iter().map(|(r, _)| r.replace(PFX, "<pfx>")).collect();
    assert_eq!(
        roots,
        vec![
            "<pfx>/drive_c/users/steamuser/h",
            "<pfx>/drive_c/users/steamuser/AppData/Roaming/a",
            "<pfx>/drive_c/users/steamuser/AppData/Local/l",
            "<pfx>/drive_c/users/steamuser/AppData/LocalLow/ll",
            "<pfx>/drive_c/users/steamuser/Documents/d",
            "<pfx>/drive_c/users/Public/p",
            "<pfx>/drive_c/ProgramData/pd",
            "<pfx>/drive_c/Program Files/pf",
            "<pfx>/drive_c/windows/w",
            &format!("{COQ}/i"),
        ]
    );
}

#[test]
fn windows_placeholders_are_dropped_for_a_linux_build() {
    let g = game("g", &[rule("{APPDATA}/G"), rule("{XDG_DATA_HOME}/G")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Linux, "/g"), &FakeProbe::linux());
    assert_eq!(set(&d), vec![("/home/u/.local/share".into(), exact("G"))]);
    assert!(d.warnings.iter().any(|w| w.contains("{APPDATA}")));
}

#[test]
fn windows_and_macos_installs_never_consider_another_build() {
    let probe = FakeProbe::windows().file("D:/G/CoQ.x86_64");
    let d = resolve(&coq(), &install(Store::Steam, Platform::Windows, "D:/G"), &probe);
    assert_eq!(d.context.builds, vec![Platform::Windows]);
    let d = resolve(&coq(), &install(Store::Steam, Platform::Macos, "/Apps/G"), &FakeProbe::linux());
    assert_eq!(d.context.builds, vec![Platform::Macos]);
}

#[test]
fn a_linux_install_without_a_prefix_is_native() {
    let d = resolve(
        &coq(),
        &install(Store::Gog, Platform::Linux, COQ),
        &FakeProbe::linux().file(&format!("{COQ}/CoQ.exe")),
    );
    assert_eq!(d.context.builds, vec![Platform::Linux]);
}

#[test]
fn store_filtering() {
    let g = game("g", &[steam("{STEAM_USERDATA}/1/remote"), win("{APPDATA}/G")]);
    let d = resolve(&g, &install(Store::Gog, Platform::Windows, "D:/G"), &FakeProbe::windows());
    assert_eq!(set(&d).len(), 1);
}

#[test]
fn broad_catalog_targets_are_dropped_as_a_second_line_of_defence() {
    let g = game("g", &[rule("{APPDATA}"), rule("{INSTALL_DIR}/save*"), rule("{APPDATA}/G")]);
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &FakeProbe::windows());
    assert_eq!(set(&d).len(), 1);
    assert_eq!(d.warnings.len(), 2);
}

#[test]
fn presence_is_reported_for_a_file_or_folder_behind_an_exact_name() {
    let g = game("g", &[rule("{INSTALL_DIR}/data/save_data.xml")]);
    let probe = FakeProbe::windows().file("D:/G/data/save_data.xml");
    let d = resolve(&g, &install(Store::Steam, Platform::Windows, "D:/G"), &probe);
    assert_eq!(d.save_set().unwrap()[0].presence, Presence::Present);
}

#[test]
fn determinism() {
    let g = coq();
    let probe = FakeProbe::linux().file(&format!("{COQ}/CoQ.exe")).file(&format!("{COQ}/CoQ.x86_64"));
    let a = resolve(&g, &proton(COQ, PFX), &probe);
    let b = resolve(&g, &proton(COQ, PFX), &probe);
    assert_eq!(a, b);
}

#[test]
fn steam_ids_convert_both_ways() {
    let account = SteamAccount::from_id64(76561198004523847).unwrap();
    assert_eq!(account.account_id, 44258119);
    assert_eq!(account.id64(), 76561198004523847);
    assert_eq!(SteamAccount::from_id64(5), None);
}

#[test]
fn filters_cover_relative_paths() {
    assert!(Filter::Exact("saves".into()).covers(&["saves", "a.sav"], false));
    assert!(!Filter::Exact("saves".into()).covers(&["other"], false));
    assert!(Filter::Pattern("C*/SGS*".into()).covers(&["C1", "SGS2", "file"], false));
    assert!(Filter::All.covers(&["x"], false));
}
