//! Drives that come and go (PLAN-HOST, "A drive the host has seen stays
//! expected"; PLAN-ERRORS E-N3, E-C14), on real disk images: a drive
//! that's unplugged is disconnected, never empty or missing, even when an
//! empty folder is left where it was mounted, and its checkpoints keep their
//! history when it comes back. macOS only: it needs `hdiutil`.

#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;

/// A disk image attached at a folder of our own, like an external drive.
struct Drive {
    image: PathBuf,
    mount: PathBuf,
    attached: bool,
}

impl Drive {
    fn new(root: &Path, name: &str, fs: &str) -> Drive {
        let image = root.join(format!("{name}.dmg"));
        hdiutil(&["create", "-quiet", "-size", "16m", "-fs", fs, "-volname", name, image.to_str().unwrap()]);
        let mut drive = Drive { image, mount: root.join(name), attached: false };
        drive.attach();
        drive
    }

    fn attach(&mut self) {
        std::fs::create_dir_all(&self.mount).unwrap();
        hdiutil(&[
            "attach",
            "-quiet",
            "-nobrowse",
            "-mountpoint",
            self.mount.to_str().unwrap(),
            self.image.to_str().unwrap(),
        ]);
        self.attached = true;
    }

    /// Unplugs it. The mount point stays behind as an empty folder.
    fn detach(&mut self) {
        hdiutil(&["detach", "-quiet", "-force", self.mount.to_str().unwrap()]);
        self.attached = false;
        assert!(self.mount.is_dir(), "the empty mount point is left behind");
    }
}

impl Drop for Drive {
    fn drop(&mut self) {
        if self.attached {
            let _ = Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&self.mount).status();
        }
    }
}

fn hdiutil(args: &[&str]) {
    let out = Command::new("hdiutil").args(args).output().expect("hdiutil");
    assert!(out.status.success(), "hdiutil {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn game_with_saves(world: &World, name: &str, saves: &Path) -> String {
    write(&saves.join("slot.sav"), "v1");
    world.custom_game(name, saves).0
}

#[test]
fn a_store_on_an_unplugged_drive_is_disconnected_and_keeps_its_history() {
    let world = World::new();
    let mut drive = Drive::new(&world.root, "StoreDrive", "APFS");
    let mut host = world.host();
    let (game, saves) = {
        let saves = world.home.join("Saves").join("Keeper");
        (game_with_saves(&world, "Keeper", &saves), saves)
    };
    let store = drive.mount.join("Checkpoints");
    assert_eq!(world.ok(&["move-store", store.to_str().unwrap()])["status"], "succeeded");
    let saved = world.ok(&["save", &game, "--label", "before the boss"]);
    let checkpoint = s(&saved["result"]["checkpoint"]);

    // Unplugged: an empty folder is left where the drive was. The store is
    // unavailable, not empty: nothing is retired.
    drive.detach();
    world.ok(&["scan", "--full"]);
    assert_eq!(world.game(&game)["save"]["reason"], "store_unavailable");
    let rows = world.history(&game);
    assert_eq!(rows.last().unwrap()["unavailable"], "store_unavailable", "{rows:?}");

    // A host started while it's out remembers the drive.
    host.kill();
    drop(host);
    let _host = world.host();
    assert_eq!(world.game(&game)["save"]["reason"], "store_unavailable");

    // Plugged back in (with another device number): same checkpoint, same
    // label, and it loads.
    drive.attach();
    world.ok(&["scan", "--full"]);
    let rows = world.history(&game);
    let row = rows.last().unwrap();
    assert!(row["unavailable"].is_null(), "{row}");
    assert_eq!(s(&row["checkpoint"]), checkpoint, "the same checkpoint, not a new generation");
    assert_eq!(row["label"], "before the boss");
    write(&saves.join("slot.sav"), "v2");
    world.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot.sav")), "v1");
}

#[test]
fn saves_on_an_unplugged_drive_are_disconnected_not_absent() {
    let world = World::new();
    let mut drive = Drive::new(&world.root, "SaveDrive", "APFS");
    let _host = world.host();
    let saves = drive.mount.join("Saves");
    let game = game_with_saves(&world, "Traveller", &saves);
    world.ok(&["save", &game]);

    // Unplugged, and its empty mount point removed too, the way macOS does
    // for drives under /Volumes.
    drive.detach();
    std::fs::remove_dir(&drive.mount).unwrap();
    assert_eq!(world.cli(&["save", &game]).error_kind(), "target_unavailable", "never saved as absent");
    assert_eq!(world.cli(&["load", &game]).error_kind(), "target_unavailable");

    drive.attach();
    write(&saves.join("slot.sav"), "v2");
    world.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot.sav")), "v1");
}

#[test]
fn saves_and_the_store_work_on_a_drive_without_exclusive_renames() {
    let world = World::new();
    let drive = Drive::new(&world.root, "ExFat", "ExFAT");
    let _host = world.host();
    let saves = drive.mount.join("Saves");
    let game = game_with_saves(&world, "Portable", &saves);
    world.ok(&["save", &game]);
    write(&saves.join("slot.sav"), "v2");

    // exFAT can't rename without replacing in one step: the Load checks,
    // then renames, and works.
    world.ok(&["load", &game]);
    assert_eq!(read(&saves.join("slot.sav")), "v1");
    // macOS keeps extended attributes in `._` companions on exFAT; they're
    // the OS's, not saves.
    let files: Vec<String> = tree(&saves).into_iter().filter(|f| !f.starts_with("._")).collect();
    assert_eq!(files, vec!["slot.sav=v1"], "no leftovers");

    // The checkpoint store works there too.
    let store = drive.mount.join("Checkpoints");
    assert_eq!(world.ok(&["move-store", store.to_str().unwrap()])["status"], "succeeded");
    world.ok(&["save", &game, "--label", "on exFAT"]);
    // exFAT keeps coarser times, so the moved copies got signatures of their
    // own: a full scan finds every checkpoint unchanged.
    world.ok(&["scan", "--full"]);
    let saves_left: Vec<_> = world.history(&game).into_iter().filter(|r| r["kind"] == "saved").collect();
    assert_eq!(saves_left.len(), 2, "{saves_left:?}");
    assert_eq!(saves_left[0]["label"], "on exFAT");
    assert!(saves_left.iter().all(|r| r["unavailable"].is_null()), "{saves_left:?}");
}
