//! The end-to-end harness: a temporary "machine" (known folders, a Steam
//! library, a fixture catalog), the real host started on it, the real CLI
//! driving it with machine-readable output, and fake games the real monitor
//! finds through the OS. Every test cleans up its processes and files, even
//! when it fails.

#![allow(dead_code)]

pub mod http;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

// What the tests need from the OS, one file per OS.
#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(not(windows), path = "unix.rs")]
mod os;

use os::registry;
#[allow(unused_imports)] // Each test binary uses its own part of the harness.
pub use os::{link_dir, unlink_dir};

pub const HOST: &str = env!("CARGO_BIN_EXE_e2e-host");
pub const CLI: &str = env!("CARGO_BIN_EXE_e2e-cli");
pub const FAKE_GAME: &str = env!("CARGO_BIN_EXE_fake-game");
pub const FAKE_UI: &str = env!("CARGO_BIN_EXE_fake-ui");

/// Steam account ids used by the fixtures.
pub const ACCOUNT_A: u32 = 44258119;
pub const ACCOUNT_B: u32 = 1;
pub const ID64_A: u64 = 76561198004523847;

pub struct World {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub data: PathBuf,
    pub home: PathBuf,
    pub appdata: PathBuf,
    pub localappdata: PathBuf,
    pub documents: PathBuf,
    pub programfiles: PathBuf,
    pub steam: PathBuf,
    pub env_file: PathBuf,
    pub catalog_file: PathBuf,
    /// This world's uninstall registry: a scratch key under HKCU.
    pub uninstall_key: String,
    _registry: registry::Scratch,
}

/// The fixture catalog: a few games that exercise the save-set shapes.
pub fn fixture_catalog() -> Value {
    json!({
        "schema": 1,
        "source": { "repo": "fixture", "revision": "fixture-1" },
        "games": [
            {
                "id": "steam-1001",
                "name": "Rogue One",
                "info": "Exit to the main menu before loading.",
                "detect": { "steam": 1001 },
                "installDirs": ["Rogue One"],
                "executables": { "windows": ["RogueOne.exe"] },
                "save": [
                    { "path": "{INSTALL_DIR}/saves" },
                    { "path": "{APPDATA}/RogueOne/profile.dat" }
                ],
                "exclude": [ { "path": "{INSTALL_DIR}/saves/options.ini" } ]
            },
            {
                "id": "steam-1002",
                "name": "Cloud Game",
                "detect": { "steam": 1002 },
                "installDirs": ["Cloud Game"],
                "executables": { "windows": ["CloudGame.exe"] },
                "save": [
                    { "when": { "store": "steam" }, "path": "{STEAM_USERDATA}/1002/remote" },
                    { "path": "{DOCUMENTS}/My Games/CloudGame/{STEAM_ID64}" }
                ]
            },
            {
                "id": "steam-1004",
                "name": "Twin Game",
                "detect": { "steam": 1004, "gog": 2004 },
                "installDirs": ["Twin Game"],
                "executables": { "windows": ["TwinGame.exe"] },
                "save": [ { "path": "{INSTALL_DIR}/save" } ]
            },
            {
                "id": "steam-1005",
                "name": "Pattern Game",
                "detect": { "steam": 1005 },
                "installDirs": ["Pattern Game"],
                "executables": { "windows": ["PatternGame.exe"] },
                "save": [ { "path": "{LOCALAPPDATA}/PatternGame/Slot*.save" } ]
            }
        ]
    })
}

impl World {
    pub fn new() -> World {
        guard();
        let dir = tempfile::Builder::new().prefix("ss-e2e-").tempdir().expect("temp dir");
        let root = dunce::canonicalize(dir.path()).expect("canonical temp dir");
        let home = root.join("home");
        let appdata = home.join("AppData").join("Roaming");
        let localappdata = home.join("AppData").join("Local");
        let documents = home.join("Documents");
        let programfiles = root.join("Program Files");
        let steam = root.join("Steam");
        for folder in [&appdata, &localappdata, &documents, &programfiles, &steam.join("steamapps")] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let registry = registry::Scratch::new();
        let world = World {
            data: root.join("data"),
            env_file: root.join("env.json"),
            catalog_file: root.join("catalog.json"),
            uninstall_key: registry.path.clone(),
            _registry: registry,
            home,
            appdata,
            localappdata,
            documents,
            programfiles,
            steam,
            root,
            _dir: dir,
        };
        world.write_env();
        world.set_catalog(&fixture_catalog());
        world.set_steam_user(ACCOUNT_A);
        world
    }

    fn write_env(&self) {
        let env = json!({
            "platform": "windows",
            "folders": {
                "home": self.home,
                "appdata": self.appdata,
                "localappdata": self.localappdata,
                "locallow": self.home.join("AppData").join("LocalLow"),
                "documents": self.documents,
                "public": self.root.join("Public"),
                "programdata": self.root.join("ProgramData"),
                "programfiles": self.programfiles,
                "windir": self.root.join("Windows"),
                "saved_games": self.home.join("Saved Games"),
                "steam_root": self.steam,
            },
            "steam_active_user_file": self.root.join("steam-active-user"),
            "query_os": false,
            "gog_games": [],
            "epic_manifests": self.root.join("epic"),
            "loose_roots": [ { "path": self.programfiles, "store": "standalone" } ],
            "uninstall_keys": [ { "hive": "current_user", "path": self.uninstall_key } ],
        });
        std::fs::write(&self.env_file, serde_json::to_string_pretty(&env).unwrap()).unwrap();
    }

    pub fn set_catalog(&self, catalog: &Value) {
        std::fs::write(&self.catalog_file, serde_json::to_string_pretty(catalog).unwrap()).unwrap();
    }

    pub fn set_gog_games(&self, games: Value) {
        let mut env: Value = serde_json::from_str(&std::fs::read_to_string(&self.env_file).unwrap()).unwrap();
        env["gog_games"] = games;
        std::fs::write(&self.env_file, serde_json::to_string_pretty(&env).unwrap()).unwrap();
    }

    /// What a standalone installer does: an uninstall entry with its folder.
    pub fn register_uninstall(&self, key: &str, install: &Path) {
        registry::set_value(&format!(r"{}\{key}", self.uninstall_key), "InstallLocation", &install.to_string_lossy());
    }

    /// What its uninstaller does last: the entry disappears.
    pub fn unregister_uninstall(&self, key: &str) {
        registry::delete_tree(&format!(r"{}\{key}", self.uninstall_key));
    }

    /// The Steam account the running client is logged into.
    pub fn set_steam_user(&self, account: u32) {
        std::fs::write(self.root.join("steam-active-user"), account.to_string()).unwrap();
    }

    /// Installs a Steam game: an app manifest plus the fake game as its exe.
    pub fn steam_install(&self, appid: u64, dir: &str, exe: &str) -> PathBuf {
        let install = self.steam.join("steamapps").join("common").join(dir);
        copy_game(&install.join(exe));
        self.steam_manifest(appid, dir, 4);
        install
    }

    /// Writes a Steam app manifest with the given `StateFlags`: 4 is fully
    /// installed, 1026 an install still downloading.
    pub fn steam_manifest(&self, appid: u64, dir: &str, flags: u32) {
        std::fs::write(
            self.steam.join("steamapps").join(format!("appmanifest_{appid}.acf")),
            format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"installdir\"\t\t\"{dir}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n}}\n"
            ),
        )
        .unwrap();
    }

    pub fn steam_uninstall(&self, appid: u64, dir: &str) {
        let _ = std::fs::remove_file(self.steam.join("steamapps").join(format!("appmanifest_{appid}.acf")));
        let _ = std::fs::remove_dir_all(self.steam.join("steamapps").join("common").join(dir));
    }

    pub fn host(&self) -> HostProcess {
        self.host_with(&[], &[])
    }

    /// Starts the real host on this world and waits for its ready line.
    pub fn host_with(&self, args: &[&str], env: &[(&str, &str)]) -> HostProcess {
        let mut all = self.host_args();
        all.extend(args.iter().map(|a| a.to_string()));
        self.spawn_host(all, env)
    }

    /// Starts the host the way a user launches the app: without
    /// `--minimized`, so it shows the UI.
    pub fn host_launched(&self, env: &[(&str, &str)]) -> HostProcess {
        self.spawn_host(self.launch_args(), env)
    }

    /// The arguments of a user launch: the test host's, without `--minimized`.
    pub fn launch_args(&self) -> Vec<String> {
        self.host_args().into_iter().filter(|a| a != "--minimized").collect()
    }

    /// Starts the host with catalog updates on, fetched from `url`.
    pub fn host_updating(&self, url: &str, args: &[&str]) -> HostProcess {
        let mut all: Vec<String> = self.host_args().into_iter().filter(|a| a != "--no-catalog-update").collect();
        all.extend(["--catalog-url".to_string(), url.to_string()]);
        all.extend(args.iter().map(|a| a.to_string()));
        self.spawn_host(all, &[])
    }

    fn spawn_host(&self, args: Vec<String>, env: &[(&str, &str)]) -> HostProcess {
        let mut command = Command::new(HOST);
        command.args(&args);
        for (k, v) in env {
            command.env(k, v);
        }
        // The host is a GUI program: its own output is in the data
        // folder's host.log, which failures below show.
        let data = args.iter().rposition(|a| a == "--data-dir").map(|i| PathBuf::from(&args[i + 1]));
        let log = data.unwrap_or_else(|| self.data.clone()).join("host.log");
        command.stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
        let mut child = command.spawn().expect("start the host");
        let stdout = child.stdout.take().unwrap();
        // Owned at once, so a failing wait below still kills it.
        let host = HostProcess { child: Some(child) };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        let line = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(line) => line,
            Err(_) => panic!(
                "the host printed no ready line; its log:\n{}",
                std::fs::read_to_string(&log).unwrap_or_default()
            ),
        };
        let ready: Value = serde_json::from_str(&line).expect("the ready line is JSON");
        assert_eq!(
            ready["ready"],
            true,
            "host not ready: {line}
{}",
            std::fs::read_to_string(&log).unwrap_or_default()
        );
        host
    }

    /// A test host: its own world, no OS integrations, and started the way
    /// clients start one (`--minimized`), so it never shows a UI.
    pub fn host_args(&self) -> Vec<String> {
        vec![
            "--data-dir".into(),
            self.data.to_string_lossy().into_owned(),
            "--minimized".into(),
            "--no-integrations".into(),
            "--no-catalog-update".into(),
            "--catalog".into(),
            self.catalog_file.to_string_lossy().into_owned(),
            "--env".into(),
            self.env_file.to_string_lossy().into_owned(),
            "--poll-ms".into(),
            "100".into(),
            "--delete-countdown-ms".into(),
            "1500".into(),
            // Never Steam's real CDN; a closed port fails at once.
            "--artwork-url".into(),
            "http://127.0.0.1:9".into(),
        ]
    }

    /// Runs the CLI against this world's host with JSON output.
    pub fn cli(&self, args: &[&str]) -> Output {
        let output = Command::new(CLI)
            .arg("--data-dir")
            .arg(&self.data)
            .arg("--json")
            .arg("--no-start")
            .args(args)
            .env("SAVESCUMMER_HOST_EXE", HOST)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run the CLI");
        Output::from(output_within(output, CLI_LIMIT, &format!("the CLI {args:?}")))
    }

    /// Starts the CLI without waiting for it; `finish` collects its output.
    pub fn cli_background(&self, args: &[&str]) -> Background {
        let child = Command::new(CLI)
            .arg("--data-dir")
            .arg(&self.data)
            .arg("--json")
            .arg("--no-start")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run the CLI");
        Background { child: Some(child) }
    }

    /// Runs the CLI and expects success; returns its last JSON line.
    pub fn ok(&self, args: &[&str]) -> Value {
        let out = self.cli(args);
        assert_eq!(out.code, 0, "{args:?} failed ({}): {}\n{}", out.code, out.stdout, out.stderr);
        out.last()
    }

    pub fn state(&self) -> Value {
        self.ok(&["status"])
    }

    pub fn game(&self, id: &str) -> Value {
        let state = self.state();
        state["games"].as_array().unwrap().iter().find(|g| g["id"] == id).cloned().unwrap_or(Value::Null)
    }

    pub fn history(&self, game: &str) -> Vec<Value> {
        let out = self.cli(&["history", game, "--all"]);
        assert_eq!(out.code, 0, "history failed: {}", out.stderr);
        out.lines
    }

    /// History row kinds, newest first.
    pub fn kinds(&self, game: &str) -> Vec<String> {
        self.history(game).iter().map(|r| r["kind"].as_str().unwrap().to_string()).collect()
    }

    /// Adds a custom game whose executable is a copy of the fake game.
    pub fn custom_game(&self, name: &str, saves: &Path) -> (String, PathBuf) {
        let exe = self.root.join("games").join(name).join(format!("{name}.exe"));
        copy_game(&exe);
        let added =
            self.ok(&["add-game", "--name", name, "--exe", exe.to_str().unwrap(), "--saves", saves.to_str().unwrap()]);
        (added["game"].as_str().unwrap().to_string(), exe)
    }

    pub fn wait_game(&self, id: &str, what: &str, check: impl Fn(&Value) -> bool) -> Value {
        wait_for(what, Duration::from_secs(20), || {
            let game = self.game(id);
            check(&game).then_some(game)
        })
    }

    pub fn wait_state(&self, what: &str, check: impl Fn(&Value) -> bool) -> Value {
        wait_for(what, Duration::from_secs(20), || {
            let state = self.state();
            check(&state).then_some(state)
        })
    }
}

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub lines: Vec<Value>,
}

impl Output {
    fn from(output: std::process::Output) -> Output {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let lines = stdout.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        Output {
            code: output.status.code().unwrap_or(-1),
            stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            lines,
        }
    }

    pub fn last(&self) -> Value {
        self.lines.last().cloned().unwrap_or(Value::Null)
    }

    /// The error kind of a refused request or a failed operation.
    pub fn error_kind(&self) -> String {
        let last = self.last();
        last["error"]["kind"].as_str().unwrap_or_default().to_string()
    }
}

/// The host process; killed on drop so a failing test leaves nothing behind.
pub struct HostProcess {
    child: Option<Child>,
}

impl HostProcess {
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    /// Waits for the host to exit on its own (a test crash point).
    pub fn wait_exit(&mut self, timeout: Duration) -> Option<i32> {
        let child = self.child.as_mut()?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = child.try_wait() {
                self.child = None;
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A running fake game; killed on drop.
pub struct Game {
    child: Option<Child>,
    pub quit: PathBuf,
}

impl Game {
    pub fn quit(&mut self) {
        let _ = std::fs::write(&self.quit, "");
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn wait_exit(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        self.quit();
    }
}

/// Copies the fake game to a path, where it acts as that game.
pub fn copy_game(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(FAKE_GAME, path).expect("copy the fake game");
}

/// A CLI run in the background; killed if the test fails first.
pub struct Background {
    child: Option<Child>,
}

impl Background {
    pub fn is_running(&mut self) -> bool {
        self.child.as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_none())
    }

    pub fn finish(mut self) -> Output {
        Output::from(output_within(self.child.take().unwrap(), CLI_LIMIT, "a background CLI"))
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn launch(exe: &Path, args: &[&str]) -> Game {
    let quit = exe.with_extension(format!("quit-{}", unique()));
    let _ = std::fs::remove_file(&quit);
    let child = Command::new(exe)
        .args(args)
        .arg("--quit-file")
        .arg(&quit)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("launch the fake game");
    Game { child: Some(child), quit }
}

fn unique() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

/// Polls until `check` returns a value, with a bounded timeout.
pub fn wait_for<T>(what: &str, timeout: Duration, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every file under a folder as `relative path=content`, sorted.
pub fn tree(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = format!("{prefix}{}", e.file_name().to_string_lossy());
            if e.path().is_dir() {
                walk(&e.path(), &format!("{name}/"), out);
            } else {
                out.push(format!("{name}={}", std::fs::read_to_string(e.path()).unwrap_or_default()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

pub fn s(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

/// The longest a CLI call may take. Operations on test saves take well
/// under a second; a call still waiting after this is a hung host.
pub const CLI_LIMIT: Duration = Duration::from_secs(120);

/// Waits for a child's exit and output, killing it after `limit`, so a hung
/// host fails the test instead of hanging the suite.
pub fn output_within(mut child: Child, limit: Duration, what: &str) -> std::process::Output {
    use std::io::Read;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait for a child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} didn't finish within {limit:?}: the host is probably hung");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    std::process::Output { status, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() }
}

/// Once per test binary: every process it starts dies with it, and the
/// whole binary is stopped if it runs far longer than it should. Why: a
/// test killed on a timeout, or one stuck on a hung host, must never leave
/// hosts or fake games running, and a hang must end as a failure.
fn guard() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        os::kill_children_on_exit();
        let limit = std::env::var("SAVESCUMMER_E2E_WATCHDOG_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(900u64);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(limit));
            eprintln!("e2e watchdog: this test binary ran for {limit}s; stopping it (set SAVESCUMMER_E2E_WATCHDOG_SECS to change)");
            std::process::exit(99);
        });
    });
}
