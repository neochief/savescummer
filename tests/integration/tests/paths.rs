#![cfg(windows)]

use savescummer_core::*;
use savescummer_platform::{Paths, SystemClock};
use savescummer_snapshots::FileSnapshots;
use savescummer_storage::SqliteRepository;
use std::{fs, path::PathBuf, sync::Arc};

fn unavailable_volume() -> PathBuf {
    // Reference an absent volume without mounting, unmounting or writing to one.
    (b'D'..=b'Z')
        .rev()
        .map(|drive| PathBuf::from(format!("{}:\\", drive as char)))
        .find(|root| matches!(root.try_exists(), Ok(false)))
        .expect("fixture needs an unused drive letter")
}

#[test]
fn offline_libraries_and_other_games_do_not_block_local_save_or_load() {
    for protected_library in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let live = temp.path().join("Game");
        fs::create_dir(&live).unwrap();
        fs::write(live.join("save"), b"original").unwrap();
        let paths = Arc::new(Paths::new(vec![]));
        let repository = Arc::new(SqliteRepository::open(&temp.path().join("db")).unwrap());
        let open = || {
            Runtime::open(
                repository.clone(),
                Arc::new(FileSnapshots::default()),
                paths.clone(),
                Arc::new(SystemClock),
            )
            .unwrap()
        };
        let rt = open();
        rt.configure("local".into(), "Local".into(), live.clone(), vec![])
            .unwrap();
        let save = rt.accept("local", Action::Save, new_id()).unwrap();
        rt.execute(&save).unwrap();
        let offline = unavailable_volume().join("SteamLibrary");
        assert!(paths.resolve(&offline).is_err());
        if protected_library {
            paths.protect([offline.clone()]).unwrap();
        } else {
            // Load metadata recorded when this other game's volume was online.
            let mut state = rt.state().unwrap();
            let mut other = state.games["local"].clone();
            other.id = "offline".into();
            other.data_dir = offline.clone();
            state.games.insert(other.id.clone(), other);
            repository.commit(&state).unwrap();
        }
        drop(rt);
        let rt = open();
        let save = rt.accept("local", Action::Save, new_id()).unwrap();
        rt.execute(&save).unwrap();
        fs::write(live.join("save"), b"changed").unwrap();
        let load = rt
            .accept("local", Action::Load { target: None }, new_id())
            .unwrap();
        rt.execute(&load).unwrap();
        assert_eq!(fs::read(live.join("save")).unwrap(), b"original");
        assert!(
            paths.validate(&offline.join("Game"), &[]).is_err(),
            "actual target still requires resolution"
        );
        if !protected_library {
            assert_eq!(
                rt.accept("offline", Action::Save, new_id())
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidPath
            );
        }
    }
}

#[test]
fn offline_references_do_not_disable_other_overlap_checks() {
    let temp = tempfile::tempdir().unwrap();
    let protected = temp.path().join("protected");
    fs::create_dir(&protected).unwrap();
    let offline = unavailable_volume().join("SteamLibrary");
    let paths = Paths::new(vec![offline.clone(), protected.clone()]);
    assert!(paths.validate(&temp.path().join("Unrelated"), &[]).is_ok());
    assert!(
        paths.validate(temp.path(), &[]).is_err(),
        "recorded protected ancestor remains blocked"
    );
    let paths = Paths::new(vec![]);
    let others = [("offline".into(), offline), ("other".into(), protected)];
    assert!(
        paths.validate(temp.path(), &others).is_err(),
        "recorded game overlap remains blocked"
    );
    assert!(
        paths
            .validate(&temp.path().join("Unrelated"), &others)
            .is_ok()
    );
    assert!(
        paths
            .validate(
                temp.path(),
                &[("invalid".into(), PathBuf::from("relative/path"))]
            )
            .is_err(),
        "malformed references must not enter the fallback"
    );
}
