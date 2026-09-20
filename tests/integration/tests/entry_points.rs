use savescummer_core::*;
use savescummer_platform::{Paths, SystemClock};
use savescummer_snapshots::FileSnapshots;
use savescummer_storage::SqliteRepository;
use std::{fs, sync::Arc};

fn runtime(root: &std::path::Path) -> Runtime {
    Runtime::open(
        Arc::new(SqliteRepository::open(&root.join("db")).unwrap()),
        Arc::new(FileSnapshots::default()),
        Arc::new(Paths::new(vec![root.to_path_buf()])),
        Arc::new(SystemClock),
    )
    .unwrap()
}
#[test]
fn explorer_actions_revalidate_generations_and_never_offer_recovery_or_unrelated_folders() {
    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("Game");
    fs::create_dir(&live).unwrap();
    fs::write(live.join("save"), "one").unwrap();
    let rt = runtime(temp.path());
    rt.configure("game".into(), "Game".into(), live.clone(), vec![])
        .unwrap();
    let targets = rt.explorer_targets(&live).unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].action, Action::Save);
    let save = rt
        .accept("game", targets[0].action.clone(), new_id())
        .unwrap();
    rt.execute(&save).unwrap();
    let state = rt.state().unwrap();
    let snapshot = state
        .snapshots
        .values()
        .find(|s| s.kind == SnapshotKind::Saved)
        .unwrap();
    let selected = rt.explorer_targets(&snapshot.path).unwrap();
    assert_eq!(selected.len(), 1);
    fs::write(live.join("save"), "two").unwrap();
    let load = rt
        .accept("game", selected[0].action.clone(), new_id())
        .unwrap();
    rt.execute(&load).unwrap();
    let state = rt.state().unwrap();
    let recovery = state
        .snapshots
        .values()
        .find(|s| s.kind == SnapshotKind::Recovery)
        .unwrap();
    assert!(rt.explorer_targets(&recovery.path).unwrap().is_empty());
    assert!(rt.explorer_targets(temp.path()).unwrap().is_empty());
    fs::write(snapshot.path.join("different"), "new generation").unwrap();
    assert!(
        rt.accept("game", selected[0].action.clone(), new_id())
            .is_err()
    );
    assert_eq!(fs::read_to_string(live.join("save")).unwrap(), "one");
    let new_target = rt.explorer_targets(&snapshot.path).unwrap();
    assert_ne!(selected[0].action, new_target[0].action);
}

#[test]
fn ambiguous_and_invalid_discovery_stays_visible_and_choices_survive_restart_and_rescan() {
    let temp = tempfile::tempdir().unwrap();
    let rt = runtime(temp.path());
    let a = GameLocation {
        data_dir: temp.path().join("a"),
        executables: vec![],
    };
    let b = GameLocation {
        data_dir: temp.path().join("b"),
        executables: vec![],
    };
    let scan = |rt: &Runtime, locations| {
        rt.record_discovery(
            "game".into(),
            "Game".into(),
            "Instructions".into(),
            locations,
        )
        .unwrap()
    };
    scan(&rt, vec![a.clone(), b.clone()]);
    assert!(
        rt.state().unwrap().games["game"]
            .configuration_error
            .is_some()
    );
    assert!(rt.accept("game", Action::Save, new_id()).is_err());
    assert!(rt.select_detected_location("game", None).is_err());
    rt.select_detected_location("game", Some(&b)).unwrap();
    scan(&rt, vec![a.clone()]);
    assert_eq!(rt.state().unwrap().games["game"].data_dir, b.data_dir);
    drop(rt);
    let rt = runtime(temp.path());
    assert_eq!(rt.state().unwrap().games["game"].data_dir, b.data_dir);
    assert!(rt.state().unwrap().games["game"].user_configured);
    rt.select_detected_location("game", None).unwrap();
    assert_eq!(rt.state().unwrap().games["game"].data_dir, a.data_dir);
    let bad = GameLocation {
        data_dir: temp.path().into(),
        executables: vec![],
    };
    rt.record_discovery("bad".into(), "Bad".into(), "".into(), vec![bad])
        .unwrap();
    assert!(
        rt.state().unwrap().games["bad"]
            .configuration_error
            .is_some()
    );
    fs::create_dir(&a.data_dir).unwrap();
    let save = rt.accept("game", Action::Save, new_id()).unwrap();
    rt.execute(&save).unwrap();
}
