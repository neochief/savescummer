//! `--demo`: the real host on a simulated machine, for UI development
//! without touching real saves or games.
//!
//! - The machine is generated in the data folder's `machine` subfolder:
//!   known folders, a Steam library holding a few real catalog games (so
//!   their art and instructions are real), empty stand-ins for their
//!   executables, and save files wherever each game's save set resolves.
//! - A scripted process source stands in for the OS process list. It plays
//!   the games one after another: each "runs" for a while, takes focus,
//!   saves progress twice through the real Save, dies and loads the latest
//!   checkpoint through the real Load, then closes.
//! - Everything else (scans, checkpoints, history, the protocol) is the real
//!   host. The data folder is `<data folder>\demo` unless `--data-dir` names
//!   one; it is wiped at every start, and a folder that isn't empty and
//!   wasn't made by `--demo` is refused, so real data is never touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use savescummer_catalog::Bundle;
use savescummer_core::Filter;
use savescummer_ipc::Phase;
use savescummer_monitor::{Proc, ProcessSource};

use crate::host::{Host, new_id};
use crate::ops;
use crate::options::Options;

/// Marks a folder `--demo` made and may wipe.
const MARKER: &str = ".savescummer-demo";
/// The Steam account the simulated client is logged into.
const ACCOUNT: u32 = 44258119;
/// Catalog games the demo prefers, by Steam app id: well-known roguelikes
/// with art. Others fill in if the catalog lacks some.
const PREFERRED: [u64; 6] = [646570, 212680, 250900, 311690, 1337520, 203770];
const GAMES: usize = 6;

/// Where the demo lives when no data folder is given.
pub fn default_data_dir() -> PathBuf {
    savescummer_platform::data_dir().join("demo")
}

/// Builds the simulated machine and points the options at it: its data
/// folder, its environment file, no sign-in changes.
pub fn prepare(mut opts: Options, catalog: &str) -> Result<Options, String> {
    let root = opts.data_dir.clone().unwrap_or_else(default_data_dir);
    wipe(&root)?;
    let machine = root.join("machine");
    let folder = |p: &str| machine.join(p);
    let steam = folder("Steam");
    for dir in
        ["home/AppData/Roaming", "home/AppData/Local", "home/AppData/LocalLow", "home/Documents", "home/Saved Games"]
    {
        fs::create_dir_all(folder(dir)).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(steam.join("steamapps").join("common")).map_err(|e| e.to_string())?;
    fs::create_dir_all(steam.join("userdata").join(ACCOUNT.to_string())).map_err(|e| e.to_string())?;
    fs::write(machine.join("steam-active-user"), ACCOUNT.to_string()).map_err(|e| e.to_string())?;

    let bundle = Bundle::parse(catalog).map_err(|e| e.to_string())?;
    for (appid, dir, exe) in pick_games(&bundle) {
        let install = steam.join("steamapps").join("common").join(&dir);
        let exe_path = install.join(&exe);
        fs::create_dir_all(exe_path.parent().unwrap_or(&install)).map_err(|e| e.to_string())?;
        fs::write(&exe_path, b"").map_err(|e| e.to_string())?;
        let manifest = format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"installdir\"\t\t\"{dir}\"\n\t\"StateFlags\"\t\t\"4\"\n}}\n"
        );
        fs::write(steam.join("steamapps").join(format!("appmanifest_{appid}.acf")), manifest)
            .map_err(|e| e.to_string())?;
    }

    let home = folder("home");
    let env = json!({
        "platform": "windows",
        "folders": {
            "home": home,
            "appdata": home.join("AppData").join("Roaming"),
            "localappdata": home.join("AppData").join("Local"),
            "locallow": home.join("AppData").join("LocalLow"),
            "documents": home.join("Documents"),
            "public": folder("Public"),
            "programdata": folder("ProgramData"),
            "programfiles": folder("Program Files"),
            "windir": folder("Windows"),
            "saved_games": home.join("Saved Games"),
            // No library cache here: art comes from Steam's CDN.
            "steam_root": steam,
        },
        "steam_active_user_file": machine.join("steam-active-user"),
        "query_os": false,
        "gog_games": [],
        "epic_manifests": folder("epic"),
        "loose_roots": [],
        "uninstall_keys": [],
    });
    let env_file = machine.join("env.json");
    fs::write(&env_file, serde_json::to_string_pretty(&env).unwrap_or_default()).map_err(|e| e.to_string())?;

    opts.data_dir = Some(root.join("data"));
    opts.env = Some(env_file);
    // The games were picked from the built-in catalog; resolve them with it.
    opts.catalog = None;
    opts.autostart = None;
    Ok(opts)
}

/// Empties `root` if it is a demo folder (or new). Anything else is refused.
fn wipe(root: &Path) -> Result<(), String> {
    if root.exists() {
        let empty = fs::read_dir(root).map(|mut d| d.next().is_none()).unwrap_or(false);
        if !empty && !root.join(MARKER).exists() {
            return Err(format!(
                "{} isn't empty and wasn't made by --demo; give --demo an empty folder",
                root.display()
            ));
        }
        fs::remove_dir_all(root).map_err(|e| format!("can't clear {}: {e}", root.display()))?;
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    fs::write(root.join(MARKER), b"made by savescummer-host --demo; wiped at every demo start\n")
        .map_err(|e| e.to_string())
}

/// Steam games from the catalog: (app id, install folder, executable).
fn pick_games(bundle: &Bundle) -> Vec<(u64, String, String)> {
    let usable = |g: &&savescummer_catalog::Game| {
        !g.detect.steam.is_empty() && !g.install_dirs.is_empty() && !g.executables.windows.is_empty()
    };
    let mut games: Vec<&savescummer_catalog::Game> = PREFERRED
        .iter()
        .filter_map(|id| bundle.games.iter().filter(usable).find(|g| g.detect.steam[0] == *id))
        .collect();
    for game in bundle.games.iter().filter(usable) {
        if games.len() >= GAMES {
            break;
        }
        if !games.iter().any(|g| g.id == game.id) {
            games.push(game);
        }
    }
    games
        .into_iter()
        .take(GAMES)
        .map(|g| (g.detect.steam[0], g.install_dirs[0].clone(), g.executables.windows[0].clone()))
        .collect()
}

/// The scripted process list: whatever the demo says is running.
#[derive(Clone, Default)]
pub struct DemoProcesses(Arc<Mutex<(Vec<Proc>, Option<u32>)>>);

impl ProcessSource for DemoProcesses {
    fn list(&mut self) -> Vec<Proc> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).0.clone()
    }

    fn foreground(&mut self) -> Option<u32> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).1
    }
}

impl DemoProcesses {
    fn run(&self, pid: u32, exe: PathBuf) {
        let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
        s.0.push(Proc { pid, parent: 1, exe: Some(exe) });
        s.1 = Some(pid);
    }

    fn stop(&self, pid: u32) {
        let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
        s.0.retain(|p| p.pid != pid);
        s.1 = None;
    }
}

/// A demo game as the driver sees it.
struct DemoGame {
    id: String,
    exe: PathBuf,
    files: Vec<PathBuf>,
}

/// One file per target, named so the target's filter matches it.
fn save_files(targets: &[savescummer_core::Target]) -> Vec<PathBuf> {
    targets
        .iter()
        .map(|t| match &t.filter {
            Filter::All => t.root.join("save1.dat"),
            // A name with an extension is a file; otherwise a folder.
            Filter::Exact(name) if name.contains('.') => t.root.join(name),
            Filter::Exact(name) => t.root.join(name).join("save.dat"),
            Filter::Pattern(pattern) => t.root.join(pattern.replace('*', "1").replace('?', "a")),
        })
        .collect()
}

fn write_progress(game: &DemoGame, text: &str) {
    for file in &game.files {
        if let Some(parent) = file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(file, format!("{text}\n"));
    }
}

fn games(host: &Host) -> Vec<DemoGame> {
    let inner = host.lock();
    inner
        .games
        .values()
        .filter(|g| g.installed)
        .filter_map(|g| {
            let targets = inner.derived.get(&g.id)?.active.as_ref().ok()?.clone();
            Some(DemoGame { id: g.id.clone(), exe: g.main_executable()?, files: save_files(&targets) })
        })
        .collect()
}

fn stopping(host: &Host) -> bool {
    host.lock().phase == Phase::ShuttingDown
}

/// Sleeps in small steps; false when the host is shutting down.
fn pause(host: &Host, time: Duration) -> bool {
    let mut left = time;
    while !left.is_zero() {
        if stopping(host) {
            return false;
        }
        let step = left.min(Duration::from_millis(200));
        std::thread::sleep(step);
        left -= step;
    }
    !stopping(host)
}

/// Runs an operation and waits for it to finish.
fn operate(host: &Arc<Host>, game: &str, request: ops::Request) {
    let Ok(op) = ops::submit(host, &new_id("demo"), game, request, false) else { return };
    for _ in 0..600 {
        match ops::find(host, &op.id) {
            Some(op) if op.status.is_final() => return,
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Gives every game saves and a short history, then plays them forever.
pub fn drive(host: Arc<Host>, processes: DemoProcesses) {
    let games = games(&host);
    for (n, game) in games.iter().enumerate() {
        write_progress(game, &format!("{}: a fresh run", game.id));
        operate(&host, &game.id, ops::Request::Save { label: Some("Fresh run".into()) });
        write_progress(game, &format!("{}: floor 3", game.id));
        operate(&host, &game.id, ops::Request::Save { label: (n % 2 == 0).then(|| "Before the boss".into()) });
    }
    let mut pid = 40_000;
    let mut floor = 4;
    loop {
        for game in &games {
            pid += 4;
            processes.run(pid, game.exe.clone());
            let played = pause(&host, Duration::from_secs(4))
                && {
                    write_progress(game, &format!("{}: floor {floor}", game.id));
                    operate(&host, &game.id, ops::Request::Save { label: None });
                    pause(&host, Duration::from_secs(5))
                }
                && {
                    write_progress(game, &format!("{}: floor {}", game.id, floor + 1));
                    operate(&host, &game.id, ops::Request::Save { label: Some(format!("Floor {}", floor + 1)) });
                    pause(&host, Duration::from_secs(4))
                }
                && {
                    // Dies, and loads the last checkpoint.
                    write_progress(game, &format!("{}: dead on floor {}", game.id, floor + 2));
                    operate(&host, &game.id, ops::Request::Load { checkpoint: None });
                    pause(&host, Duration::from_secs(5))
                };
            processes.stop(pid);
            floor += 1;
            if !played || !pause(&host, Duration::from_secs(3)) {
                return;
            }
        }
        if games.is_empty() && !pause(&host, Duration::from_secs(5)) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_with_other_data_is_never_wiped() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("host.db"), b"real").unwrap();
        assert!(wipe(dir.path()).is_err());
        assert!(dir.path().join("host.db").exists());

        let demo = dir.path().join("demo");
        wipe(&demo).unwrap();
        fs::write(demo.join("leftover"), b"x").unwrap();
        wipe(&demo).unwrap();
        assert!(!demo.join("leftover").exists(), "a demo folder is wiped");
        assert!(demo.join(MARKER).exists());
    }

    #[test]
    fn save_files_match_their_filters() {
        let target = |filter| savescummer_core::Target {
            root: PathBuf::from("R"),
            filter,
            excludes: vec![],
            presence: savescummer_core::Presence::Present,
        };
        let files = save_files(&[
            target(Filter::All),
            target(Filter::Exact("continue.sav".into())),
            target(Filter::Exact("saves".into())),
            target(Filter::Pattern("C*/SGS?".into())),
        ]);
        let expected = ["R/save1.dat", "R/continue.sav", "R/saves/save.dat", "R/C1/SGSa"];
        assert_eq!(files, expected.iter().map(PathBuf::from).collect::<Vec<_>>());
    }
}
