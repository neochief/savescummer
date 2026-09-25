//! Scale: histories of 100, 10,000 and 100,000 rows, in one game and spread
//! across many. Each scenario checks that the host still works (it starts,
//! pages history correctly, saves, keeps an idle watch quiet) and measures
//! startup, memory, writes per Save, the cost of an idle watch and the
//! first history page.
//!
//! Timings are reported, never asserted, so a busy machine doesn't fail
//! the suite: each scenario prints one JSON line and appends it to
//! `scale-report.jsonl` in Cargo's test temp folder. The 100,000-row
//! scenarios take minutes to seed and run with `--ignored`.
//!
//! Seeding: one real Save per game makes a template checkpoint; the test
//! then copies its folder and records each copy (with its own signature)
//! straight into the database, alternating start, two saves and close, the
//! way a long play history looks. The host then verifies every copy at
//! startup, as it would real checkpoints.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use savescummer_core::history::RowKind;
use savescummer_ipc::{Client, Command, EventBody, HistoryPage};
use savescummer_storage::{self as db, CheckpointRow, HistoryRow, Storage};
use serde_json::{Value, json};

const PAGE: usize = 50;

fn scale_game(k: usize) -> Value {
    json!({
        "id": format!("steam-{}", 3000 + k),
        "name": format!("Scale Game {k}"),
        "detect": { "steam": 3000 + k },
        "installDirs": [format!("Scale {k}")],
        "executables": { "windows": ["Scale.exe"] },
        "save": [ { "path": "{INSTALL_DIR}/saves" } ]
    })
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Adds `rows` history rows to `game`, half of them Saves with their own
/// checkpoint folder.
fn seed_game(storage: &Storage, store: &Path, game: &str, rows: usize) {
    let conn = storage.conn();
    let template = db::existing_checkpoints(conn, game).unwrap().into_iter().next().expect("a template checkpoint");
    let template_dir = store.join(&template.folder);
    conn.execute_batch("BEGIN").unwrap();
    for i in 0..rows {
        let session = Some(format!("session-scale-{game}-{}", i / 4));
        let mut row = HistoryRow {
            seq: 0,
            id: format!("row-scale-{game}-{i}"),
            game_id: game.to_string(),
            kind: RowKind::GameStarted,
            at: template.created_at.clone(),
            session,
            checkpoint_id: None,
            recovery_id: None,
            reverted_row: None,
            removed: 0,
            cloud_check: false,
            cloud_replaced: false,
            visible: true,
        };
        match i % 4 {
            0 => {}
            3 => row.kind = RowKind::GameClosed,
            _ => {
                let folder = format!("{} #{i}", template.folder);
                let dir = store.join(&folder);
                copy_dir(&template_dir, &dir);
                let sig = savescummer_snapshots::checkpoint::signature(&dir).unwrap();
                let checkpoint = CheckpointRow {
                    seq: 0,
                    id: format!("cp-scale-{game}-{i}"),
                    folder,
                    signature: sig.hash,
                    identity: sig.identity,
                    label: (i % 10 == 1).then(|| format!("run {i}")),
                    ..template.clone()
                };
                db::insert_checkpoint(conn, &checkpoint).unwrap();
                row.kind = RowKind::Saved;
                row.checkpoint_id = Some(checkpoint.id);
            }
        }
        db::insert_row(conn, &row).unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
}

/// A world with `games` scale games, each with a real template Save and
/// then `rows / games` seeded rows.
fn seeded_world(games: usize, rows: usize) -> (World, Vec<String>, Duration) {
    let world = World::new();
    let mut catalog = fixture_catalog();
    let ids: Vec<String> = (0..games).map(|k| format!("steam-{}", 3000 + k)).collect();
    for k in 0..games {
        catalog["games"].as_array_mut().unwrap().push(scale_game(k));
        let install = world.steam_install(3000 + k as u64, &format!("Scale {k}"), "Scale.exe");
        write(&install.join("saves").join("slot1.sav"), &format!("game {k}, floor 1"));
    }
    world.set_catalog(&catalog);
    {
        let _host = world.host();
        for id in &ids {
            world.ok(&["save", id, "--label", "template"]);
        }
    }
    let started = Instant::now();
    let storage = Storage::open(&world.data.join("host.db")).unwrap();
    let store = world.data.join("checkpoints");
    for id in &ids {
        seed_game(&storage, &store, id, rows / games);
    }
    drop(storage);
    (world, ids, started.elapsed())
}

#[cfg(windows)]
mod process {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::{
        GetProcessIoCounters, GetProcessTimes, IO_COUNTERS, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    pub struct Process(HANDLE);

    impl Process {
        pub fn open(pid: u32) -> Process {
            // SAFETY: a query handle, closed in drop.
            let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
            assert!(!handle.is_null(), "open the host process");
            Process(handle)
        }

        /// Working set and private bytes.
        pub fn memory(&self) -> (u64, u64) {
            // SAFETY: a zeroed out-structure with its size set.
            unsafe {
                let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
                counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                GetProcessMemoryInfo(self.0, &mut counters as *mut _ as *mut _, counters.cb);
                (counters.WorkingSetSize as u64, counters.PrivateUsage as u64)
            }
        }

        /// Write operations and bytes written so far.
        pub fn writes(&self) -> (u64, u64) {
            // SAFETY: a zeroed out-structure.
            unsafe {
                let mut io: IO_COUNTERS = std::mem::zeroed();
                GetProcessIoCounters(self.0, &mut io);
                (io.WriteOperationCount, io.WriteTransferCount)
            }
        }

        /// CPU time used so far (kernel plus user).
        pub fn cpu(&self) -> std::time::Duration {
            // SAFETY: zeroed out-structures.
            unsafe {
                let mut t: [FILETIME; 4] = std::mem::zeroed();
                GetProcessTimes(self.0, &mut t[0], &mut t[1], &mut t[2], &mut t[3]);
                let ticks = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
                std::time::Duration::from_nanos((ticks(t[2]) + ticks(t[3])) * 100)
            }
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: opened in `open`.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Off Windows the numbers come from `ps`. There's no cheap per-process
/// write counter, so writes report zero.
#[cfg(not(windows))]
mod process {
    pub struct Process(u32);

    impl Process {
        pub fn open(pid: u32) -> Process {
            Process(pid)
        }

        fn ps(&self, field: &str) -> String {
            let out = std::process::Command::new("ps").args(["-o", field, "-p", &self.0.to_string()]).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        /// Resident set size, for both numbers.
        pub fn memory(&self) -> (u64, u64) {
            let rss = self.ps("rss=").parse::<u64>().unwrap_or(0) * 1024;
            (rss, rss)
        }

        pub fn writes(&self) -> (u64, u64) {
            (0, 0)
        }

        /// CPU time used so far, from `[[dd-]hh:]mm:ss[.ff]`.
        pub fn cpu(&self) -> std::time::Duration {
            let time = self.ps("time=");
            let (days, rest) =
                time.split_once('-').map_or((0.0, time.as_str()), |(d, r)| (d.parse().unwrap_or(0.0), r));
            let secs = rest.split(':').fold(0.0, |acc, part| acc * 60.0 + part.parse::<f64>().unwrap_or(0.0));
            std::time::Duration::from_secs_f64(days * 86_400.0 + secs)
        }
    }
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 10_000.0).round() / 10.0
}

fn history(client: &mut Client, game: &str, cursor: Option<String>) -> HistoryPage {
    let response = client.request(None, Command::History { game: game.to_string(), cursor, limit: None }).unwrap();
    assert!(response.ok, "history failed: {:?}", response.error);
    serde_json::from_value(response.result.unwrap()).unwrap()
}

/// Runs the checks and measurements on a seeded world.
fn measure(name: &str, games: usize, rows: usize) {
    let (world, ids, seeding) = seeded_world(games, rows);
    let busiest = ids[0].clone();

    let started = Instant::now();
    let host = world.host();
    let startup = started.elapsed();
    let process = process::Process::open(host.pid().unwrap());
    let (working_set, private) = process.memory();

    // The first page: newest first, a full page, and the next page follows on.
    let mut client = Client::connect(&savescummer_ipc::endpoint(&world.data)).unwrap();
    let asked = Instant::now();
    let first = history(&mut client, &busiest, None);
    let first_page = asked.elapsed();
    let per_game = rows / games + 1;
    assert_eq!(first.rows.len(), PAGE.min(per_game), "a full first page");
    assert_eq!(first.rows[0].kind, RowKind::GameClosed, "newest first: the seeded history ends with a close");
    let asked = Instant::now();
    let second = history(&mut client, &busiest, first.next.clone());
    let second_page = asked.elapsed();
    if per_game > PAGE {
        assert!(!second.rows.is_empty());
        assert_ne!(second.rows[0].id, first.rows[0].id);
    }
    // The seeded checkpoints were verified as intact at startup: the newest
    // one is the latest usable checkpoint.
    let state = world.game(&busiest);
    assert!(state["has_history"] == true);
    assert_eq!(state["latest"]["id"], format!("cp-scale-{busiest}-{}", rows / games - 2));

    // Saves on a big history: their writes and their time. The first one
    // also settles which seeded rows are visible; the second is the
    // steady state.
    let install = world.steam.join("steamapps").join("common").join("Scale 0");
    let measured_save = |label: &str, text: &str| {
        write(&install.join("saves").join("slot1.sav"), text);
        let (ops_before, bytes_before) = process.writes();
        let saving = Instant::now();
        world.ok(&["save", &busiest, "--label", label]);
        let time = saving.elapsed();
        let (ops_after, bytes_after) = process.writes();
        (time, ops_after - ops_before, (bytes_after - bytes_before) / 1024)
    };
    let (first_save_time, first_save_ops, first_save_kb) = measured_save("first", "floor 2");
    let (save_time, save_ops, save_kb) = measured_save("measured", "floor 3");
    assert_eq!(world.game(&busiest)["latest"]["label"], "measured");

    // An idle watch: no events, and little CPU while nothing happens.
    let mut watcher = Client::connect(&savescummer_ipc::endpoint(&world.data)).unwrap();
    let response = watcher.request(None, Command::Watch).unwrap();
    assert!(response.ok);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(Some(event)) = watcher.next_event() {
            if tx.send(event).is_err() {
                return;
            }
        }
    });
    // The subscription's first snapshot, and anything it set off.
    std::thread::sleep(Duration::from_millis(1000));
    while rx.try_recv().is_ok() {}
    let cpu_before = process.cpu();
    let idle = Duration::from_secs(5);
    std::thread::sleep(idle);
    let cpu_idle = process.cpu().saturating_sub(cpu_before);
    let idle_events: Vec<_> = rx.try_iter().filter(|e| !matches!(e.body, EventBody::Shutdown)).collect();
    assert!(idle_events.is_empty(), "an idle host sends nothing: {idle_events:?}");

    let report = json!({
        "scenario": name,
        "games": games,
        "rows": rows,
        "seeding_ms": ms(seeding),
        "startup_ms": ms(startup),
        "working_set_mb": working_set as f64 / 1_048_576.0,
        "private_mb": private as f64 / 1_048_576.0,
        "first_page_ms": ms(first_page),
        "second_page_ms": ms(second_page),
        "first_save_ms": ms(first_save_time),
        "first_save_write_ops": first_save_ops,
        "first_save_write_kb": first_save_kb,
        "save_ms": ms(save_time),
        "save_write_ops": save_ops,
        "save_write_kb": save_kb,
        "idle_watch_cpu_ms": ms(cpu_idle),
        "idle_watch_seconds": idle.as_secs(),
    });
    eprintln!("{report}");
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("scale-report.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
    writeln!(file, "{report}").unwrap();
}

#[test]
fn a_hundred_rows() {
    measure("100 rows, one game", 1, 100);
}

#[test]
fn ten_thousand_rows_in_one_game() {
    measure("10,000 rows, one game", 1, 10_000);
}

#[test]
fn ten_thousand_rows_across_fifty_games() {
    measure("10,000 rows, 50 games", 50, 10_000);
}

#[test]
#[ignore = "minutes of seeding; run with --ignored"]
fn a_hundred_thousand_rows_in_one_game() {
    measure("100,000 rows, one game", 1, 100_000);
}

#[test]
#[ignore = "minutes of seeding; run with --ignored"]
fn a_hundred_thousand_rows_across_fifty_games() {
    measure("100,000 rows, 50 games", 50, 100_000);
}
