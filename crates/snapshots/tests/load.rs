//! The four Load stages against real temporary folders, compared by file
//! contents: exact restores, deleting newer files, absent targets, undo at
//! every stage, and a save held open by another handle.

use std::fs;
use std::path::{Path, PathBuf};

use savescummer_core::common::RecordedTarget;
use savescummer_core::recovery::{Rule, classify_load};
use savescummer_core::{ErrorKind, Filter, Presence, Target};
use savescummer_snapshots::load::{
    self, observe, recover, stage_clean_up, stage_copy_in, stage_set_aside, stage_swap_in,
};
use savescummer_snapshots::{CheckpointMeta, copy_save_set, no_hook, plan_load};

struct Fixture {
    _dir: tempfile::TempDir,
    live: PathBuf,
    store: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    let store = dir.path().join("store");
    fs::create_dir_all(&live).unwrap();
    fs::create_dir_all(&store).unwrap();
    Fixture { live, store, _dir: dir }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn target(root: &Path, filter: Filter) -> Target {
    Target { root: root.to_path_buf(), filter, excludes: vec![], presence: Presence::Present }
}

fn snapshot(targets: &[Target], dest: &Path) -> Vec<RecordedTarget> {
    let mut meta = CheckpointMeta {
        format: 1,
        game_id: "g".into(),
        game_name: "G".into(),
        kind: "saved".into(),
        created_at: "now".into(),
        operation: None,
        targets: vec![],
    };
    copy_save_set(targets, dest, &mut meta, true, &no_hook).unwrap();
    meta.targets
}

/// The whole tree under a folder as `path=content` lines, for comparisons.
fn contents(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = format!("{prefix}{}", e.file_name().to_string_lossy());
            if e.path().is_dir() {
                out.push(format!("{name}/"));
                walk(&e.path(), &format!("{name}/"), out);
            } else {
                out.push(format!("{name}={}", fs::read_to_string(e.path()).unwrap_or_default()));
            }
        }
    }
    walk(root, "", &mut out);
    out
}

fn apply(checkpoint: &Path, recorded: &[RecordedTarget], targets: &[Target]) -> Result<usize, load::StageError> {
    let pairs: Vec<_> = recorded.iter().cloned().zip(targets.iter().cloned()).collect();
    let mut plan = plan_load(checkpoint, &pairs, true).map_err(|f| load::StageError { failure: f, undone: true })?;
    stage_copy_in(&mut plan, &no_hook)?;
    stage_set_aside(&plan, &no_hook)?;
    stage_swap_in(&plan, &no_hook)?;
    assert!(stage_clean_up(&plan, &no_hook).is_empty());
    Ok(plan.removed_files())
}

#[test]
fn a_load_makes_the_saves_exactly_as_they_were() {
    let f = fixture();
    let saves = f.live.join("saves");
    write(&saves.join("run1.sav"), "one");
    write(&saves.join("deep/a.sav"), "a");
    write(&saves.join("Player.log"), "log v1");
    let targets = [target(&f.live, Filter::Exact("saves".into()))];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    let before = contents(&saves);

    write(&saves.join("run1.sav"), "one, later");
    write(&saves.join("run2.sav"), "a newer run");
    write(&saves.join("newdir/x.sav"), "x");
    fs::remove_file(saves.join("deep/a.sav")).unwrap();
    write(&saves.join("Player.log"), "log v2");

    let removed = apply(&cp, &recorded, &targets).unwrap();
    assert_eq!(removed, 2, "run2.sav and newdir/x.sav are newer saves");
    let after: Vec<String> = contents(&saves).into_iter().filter(|l| !l.starts_with("Player.log")).collect();
    let before: Vec<String> = before.into_iter().filter(|l| !l.starts_with("Player.log")).collect();
    assert_eq!(after, before);
    assert_eq!(fs::read_to_string(saves.join("Player.log")).unwrap(), "log v2", "logs are never restored");
    assert!(contents(&f.live).iter().all(|l| !l.contains(".ssnew") && !l.contains(".ssold")));
}

#[test]
fn patterns_touch_only_what_they_match() {
    let f = fixture();
    write(&f.live.join("user_0.dat"), "u0");
    write(&f.live.join("dc_options.json"), "settings v1");
    let targets = [target(&f.live, Filter::Pattern("user_*.dat".into()))];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    write(&f.live.join("user_0.dat"), "u0 later");
    write(&f.live.join("user_1.dat"), "u1");
    write(&f.live.join("dc_options.json"), "settings v2");
    apply(&cp, &recorded, &targets).unwrap();
    assert_eq!(contents(&f.live), vec!["dc_options.json=settings v2", "user_0.dat=u0"]);
}

#[test]
fn an_absent_target_is_left_alone_and_an_empty_one_is_emptied() {
    let f = fixture();
    let a = f.live.join("a");
    let b = f.live.join("b");
    fs::create_dir_all(&b).unwrap(); // exists, matches nothing
    let targets = [target(&a, Filter::All), target(&b, Filter::All)];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    assert!(recorded[0].absent);
    assert!(!recorded[1].absent);
    write(&a.join("appeared.sav"), "cloud switched on");
    write(&b.join("run.sav"), "started a run");
    apply(&cp, &recorded, &targets).unwrap();
    assert_eq!(contents(&a), vec!["appeared.sav=cloud switched on"]);
    assert!(contents(&b).is_empty());
}

#[test]
fn a_missing_root_refuses_the_load() {
    let f = fixture();
    let a = f.live.join("a");
    write(&a.join("x.sav"), "x");
    let targets = [target(&a, Filter::All)];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    fs::remove_dir_all(&a).unwrap();
    let err = apply(&cp, &recorded, &targets).unwrap_err();
    assert_eq!(err.failure.kind, ErrorKind::RootMissing);
    assert!(!a.exists(), "roots are never recreated");
}

#[test]
fn a_save_set_spanning_several_folders() {
    let f = fixture();
    let (x, y) = (f.live.join("x"), f.live.join("y"));
    write(&x.join("saves/1"), "x1");
    write(&y.join("prefs/p"), "y1");
    let targets = [target(&x, Filter::Exact("saves".into())), target(&y, Filter::Exact("prefs".into()))];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    write(&x.join("saves/1"), "x2");
    write(&y.join("prefs/p"), "y2");
    apply(&cp, &recorded, &targets).unwrap();
    assert_eq!(fs::read_to_string(x.join("saves/1")).unwrap(), "x1");
    assert_eq!(fs::read_to_string(y.join("prefs/p")).unwrap(), "y1");
}

#[cfg(windows)]
#[test]
fn a_save_held_open_refuses_the_load_at_stage_two_with_nothing_changed() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = fixture();
    write(&f.live.join("a.sav"), "a1");
    write(&f.live.join("b.sav"), "b1");
    let targets = [target(&f.live, Filter::All)];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    write(&f.live.join("a.sav"), "a2");
    write(&f.live.join("b.sav"), "b2");
    let before = contents(&f.live);
    // Held open without delete sharing, like a game keeping its save open.
    let _held =
        fs::OpenOptions::new().read(true).share_mode(1 /* FILE_SHARE_READ */).open(f.live.join("b.sav")).unwrap();
    let err = apply(&cp, &recorded, &targets).unwrap_err();
    assert_eq!(err.failure.kind, ErrorKind::InUse);
    assert!(err.undone);
    assert_eq!(contents(&f.live), before, "renames undone, copies deleted");
}

#[test]
fn a_name_created_between_stages_fails_stage_three_and_undoes_everything() {
    let f = fixture();
    write(&f.live.join("a.sav"), "a1");
    let targets = [target(&f.live, Filter::All)];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    write(&f.live.join("a.sav"), "a2");
    let pairs: Vec<_> = recorded.iter().cloned().zip(targets.iter().cloned()).collect();
    let mut plan = plan_load(&cp, &pairs, true).unwrap();
    stage_copy_in(&mut plan, &no_hook).unwrap();
    stage_set_aside(&plan, &no_hook).unwrap();
    // The game writes its save between stages 2 and 3.
    write(&f.live.join("a.sav"), "game wrote");
    let err = stage_swap_in(&plan, &no_hook).unwrap_err();
    assert_eq!(err.failure.kind, ErrorKind::SwapFailed);
    // The set-aside original can't go back over the game's new file: the
    // undo reports it, and every file is kept.
    assert!(!err.undone);
    assert_eq!(fs::read_to_string(f.live.join("a.sav")).unwrap(), "game wrote");
    assert_eq!(fs::read_to_string(f.live.join("a.sav.ssold")).unwrap(), "a2");
}

/// Runs stages up to a point, then classifies and recovers as a restarted
/// host would.
fn interrupted(stop_after: usize) -> (Fixture, Rule, Vec<String>) {
    let f = fixture();
    write(&f.live.join("a.sav"), "a1");
    write(&f.live.join("b.sav"), "b1");
    let targets = [target(&f.live, Filter::All)];
    let cp = f.store.join("cp");
    let recorded = snapshot(&targets, &cp);
    write(&f.live.join("a.sav"), "a2");
    write(&f.live.join("c.sav"), "c2");
    let pairs: Vec<_> = recorded.iter().cloned().zip(targets.iter().cloned()).collect();
    let mut plan = plan_load(&cp, &pairs, true).unwrap();
    // Stop part-way through a stage by failing on the Nth hook call.
    let stop = std::cell::Cell::new(0usize);
    let hook = |_: &str, _: usize| {
        stop.set(stop.get() + 1);
        if stop.get() == stop_after {
            panic!("crash");
        }
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stage_copy_in(&mut plan, &hook).unwrap();
        stage_set_aside(&plan, &hook).unwrap();
        stage_swap_in(&plan, &hook).unwrap();
        stage_clean_up(&plan, &hook);
    }));
    assert!(result.is_err() || stop_after > 100);
    let rule = classify_load(&observe(&plan));
    recover(&plan, rule).unwrap();
    let state = contents(&f.live);
    (f, rule, state)
}

#[test]
fn a_kill_at_every_step_lands_on_the_right_rule() {
    // Hook calls: copy_in a, b (2); set_aside a, b, c (3); swap_in a, b (2);
    // clean_up (3).
    let untouched = vec!["a.sav=a2".to_string(), "b.sav=b1".into(), "c.sav=c2".into()];
    let loaded = vec!["a.sav=a1".to_string(), "b.sav=b1".into()];
    let expectations = [
        (1, Rule::R1, &untouched),
        (2, Rule::R1, &untouched),
        (3, Rule::R2, &untouched),
        (5, Rule::R2, &untouched),
        (6, Rule::R2, &untouched),
        (7, Rule::R3, &loaded),
        (8, Rule::R3, &loaded),
        (10, Rule::R3, &loaded),
    ];
    for (stop, rule, state) in expectations {
        let (_f, got_rule, got_state) = interrupted(stop);
        assert_eq!(got_rule, rule, "stop after hook call {stop}");
        assert_eq!(&got_state, state, "stop after hook call {stop}");
    }
}
