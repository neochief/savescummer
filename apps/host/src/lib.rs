//! SaveScummer, the host: the app itself. It finds games, watches them run,
//! makes and restores backups, keeps the history, reacts to hotkeys, shows
//! the UI when the user launches the app, and serves the protocol the UI and
//! the CLI both use.
//!
//! Startup order: open the database, resolve interrupted operations, scan,
//! start monitoring, then accept operations. Clients can connect earlier
//! and see the host starting.

pub mod artwork;
pub mod catalog_update;
pub mod checkpoints;
pub mod demo;
pub mod feedback;
pub mod fetch;
pub mod host;
pub mod library;
pub mod log;
pub mod model;
pub mod monitoring;
pub mod ops;
pub mod options;
pub mod queries;
pub mod recovery;
pub mod scan;
pub mod server;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;

use savescummer_catalog::Bundle;
use savescummer_ipc::{EventBody, Phase};
use savescummer_scanner::Environment;
use savescummer_storage::{self as db, Storage};

use crate::host::{CatalogState, Host, Inner};
use crate::model::{SETTING_PLAY_SOUNDS, SETTING_STORE};
use crate::options::Options;

/// The catalog built into the host, the fallback when nothing newer exists.
pub const EMBEDDED_CATALOG: &str = include_str!("../../../catalog/catalog.json");

/// The host is a GUI program (see PLAN-HOST, PROCESSES): no console window,
/// whoever starts it. Everything it has to say goes to the host log; the
/// ready line and `--version` also go to standard output, which reaches a
/// caller that captured it.
pub fn main() -> ExitCode {
    // A panic's message would otherwise go to a console nobody has.
    std::panic::set_hook(Box::new(|info| log::line(&format!("panic: {info}"))));
    let mut opts = match Options::try_parse() {
        Ok(opts) => opts,
        Err(e) if matches!(e.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) => {
            print!("{}", e.render());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            // The options didn't parse: log where `--data-dir` points if it
            // is there at all, else in the default data folder.
            early_log(&raw_data_dir().unwrap_or_else(savescummer_platform::data_dir));
            let text = e.render().to_string();
            ready_line(false, &format!("bad options: {}", text.lines().next().unwrap_or_default()), None);
            return ExitCode::from(2);
        }
    };
    if opts.demo {
        opts = match demo::prepare(opts, EMBEDDED_CATALOG) {
            Ok(opts) => opts,
            Err(e) => {
                // Never write into a folder --demo refused.
                early_log(&savescummer_platform::data_dir());
                ready_line(false, &format!("--demo: {e}"), None);
                return ExitCode::from(2);
            }
        };
    }
    let data_dir = opts.data_dir.clone().unwrap_or_else(savescummer_platform::data_dir);
    if let Err(e) = fs::create_dir_all(&data_dir) {
        early_log(&savescummer_platform::data_dir());
        ready_line(false, &format!("can't create the data folder {}: {e}", data_dir.display()), None);
        return ExitCode::from(2);
    }
    // Logged from here on, even when another host owns the folder.
    log::init(&data_dir);
    if let Some(mode) = &opts.autostart {
        return autostart(mode == "on", &opts);
    }
    // One host per user and data folder.
    let lock_path = data_dir.join("host.lock");
    let lock = match fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(e) => {
            ready_line(false, &format!("can't open {}: {e}", lock_path.display()), None);
            return ExitCode::from(2);
        }
    };
    if lock.try_lock().is_err() {
        // The user launched the app again: the running host shows the UI.
        if !opts.minimized {
            hand_over(&data_dir);
        }
        ready_line(false, "another host is already running for this data folder", None);
        return ExitCode::from(3);
    }
    trace(&format!("host {} starting", env!("CARGO_PKG_VERSION")));
    let code = run(opts, data_dir);
    drop(lock);
    code
}

/// Asks the host already running for this data folder to show the UI.
fn hand_over(data_dir: &Path) {
    let endpoint = savescummer_ipc::endpoint(data_dir);
    let result = savescummer_ipc::Client::connect(&endpoint)
        .map_err(|e| e.to_string())
        .and_then(|mut client| client.request(None, savescummer_ipc::Command::ShowUi).map_err(|e| e.to_string()));
    match result {
        Ok(response) if response.ok => trace("asked the running host to show the UI"),
        Ok(response) => trace(&format!("the running host didn't show the UI: {:?}", response.error)),
        Err(e) => trace(&format!("can't reach the running host to show the UI: {e}")),
    }
}

fn autostart(on: bool, opts: &Options) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            trace(&format!("launch on startup: {e}"));
            return ExitCode::from(2);
        }
    };
    match savescummer_platform::autostart::set(on, &exe, opts.data_dir.as_deref()) {
        Ok(()) => {
            let text = format!("launch on startup: {}", if on { "on" } else { "off" });
            trace(&text);
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            trace(&format!("launch on startup: {e}"));
            ExitCode::from(2)
        }
    }
}

/// The machine-readable line tooling waits for.
fn ready_line(ready: bool, error: &str, host: Option<&Host>) {
    let line = match host {
        Some(host) => serde_json::json!({
            "ready": ready,
            "instance": host.instance,
            "endpoint": host.endpoint,
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
        }),
        None => serde_json::json!({ "ready": ready, "error": error }),
    };
    trace(&format!("ready line: {line}"));
    println!("{line}");
}

/// `--data-dir` from the raw arguments, for options that didn't parse.
fn raw_data_dir() -> Option<PathBuf> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    args.iter().enumerate().rev().find_map(|(i, arg)| {
        let text = arg.to_string_lossy();
        match text.strip_prefix("--data-dir=") {
            Some(value) => Some(PathBuf::from(value)),
            None if text == "--data-dir" => args.get(i + 1).map(PathBuf::from),
            None => None,
        }
    })
}

/// Logs into `data_dir` for failures before the real data folder is usable.
fn early_log(data_dir: &Path) {
    let _ = fs::create_dir_all(data_dir);
    log::init(data_dir);
}

fn load_catalog(opts: &Options, data_dir: &Path) -> Result<CatalogState, String> {
    // The built-in catalog, or the one a test names instead.
    let (built_in, source) = match &opts.catalog {
        Some(path) => {
            let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            (Bundle::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?, "file")
        }
        None => (Bundle::parse(EMBEDDED_CATALOG).map_err(|e| format!("the built-in catalog: {e}"))?, "embedded"),
    };
    // A verified downloaded bundle wins; any failure falls back silently.
    let (bundle, source) = match catalog_update::load_downloaded(data_dir) {
        Some(bundle) => (bundle, "downloaded"),
        None => (built_in, source),
    };
    Ok(CatalogState { bundle: Arc::new(bundle), source: source.into(), checked_at: None, problem: None })
}

fn run(opts: Options, data_dir: PathBuf) -> ExitCode {
    let storage = match Storage::open(&data_dir.join("host.db")) {
        Ok(s) => s,
        Err(e) => {
            ready_line(false, &format!("can't open the database: {e}"), None);
            return ExitCode::from(2);
        }
    };
    let env = match &opts.env {
        Some(path) => match Environment::from_file(path) {
            Ok(env) => env,
            Err(e) => {
                ready_line(false, &e, None);
                return ExitCode::from(2);
            }
        },
        None => Environment::detect(),
    };
    let catalog = match load_catalog(&opts, &data_dir) {
        Ok(c) => c,
        Err(e) => {
            ready_line(false, &e, None);
            return ExitCode::from(2);
        }
    };
    let setting = |key: &str| db::setting(storage.conn(), key).ok().flatten();
    let store = setting(SETTING_STORE).map(PathBuf::from).unwrap_or_else(|| data_dir.join("checkpoints"));
    let play_sounds = setting(SETTING_PLAY_SOUNDS).is_none_or(|v| v == "1");
    let launch = std::env::current_exe().map(|exe| savescummer_platform::autostart::is_enabled(&exe)).unwrap_or(false);
    let notices = db::notices(storage.conn()).unwrap_or_default();
    let mut inner = Inner::new(store, play_sounds, launch);
    for (game, kind, _) in notices {
        inner.notices.insert(game, kind);
    }
    let endpoint = savescummer_ipc::endpoint(&data_dir);
    let host = Host::new(opts.clone(), data_dir, env, endpoint.clone(), catalog, storage, inner);
    if let Err(e) = host.db().write(|c| db::start_run(c, &host.instance, &host::now())) {
        trace(&format!("can't record this run: {e}"));
    }
    {
        let mut inner = host.lock();
        library::load_games(&host, &mut inner);
        host.publish(&mut inner);
    }
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    *host.shutdown.lock().unwrap_or_else(|e| e.into_inner()) = Some(shutdown_tx);

    // Serve early: a client can connect and see the host starting.
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            ready_line(false, &e.to_string(), None);
            return ExitCode::from(2);
        }
    };
    let bound = {
        let _guard = runtime.enter();
        savescummer_ipc::transport::Listener::bind(&endpoint)
    };
    let listener = match bound {
        Ok(listener) => listener,
        Err(e) => {
            ready_line(false, &format!("another host owns {endpoint}: {e}"), None);
            return ExitCode::from(3);
        }
    };
    runtime.spawn(server::serve(host.clone(), listener));

    trace("resolving interrupted operations");
    recovery::resolve_all(&host);
    trace("first scan");
    scan::run(&host, true, "startup");
    trace("scan done");
    {
        let mut inner = host.lock();
        inner.scan.scans += 1;
        inner.scan.full_scans += 1;
    }
    {
        let worker = host.clone();
        std::thread::spawn(move || scan::worker(worker));
        let periodic = host.clone();
        std::thread::spawn(move || scan::periodic(periodic));
        let heartbeat = host.clone();
        std::thread::spawn(move || run_heartbeat(heartbeat));
        let catalog = host.clone();
        std::thread::spawn(move || catalog_update::run(catalog));
        let art = host.clone();
        std::thread::spawn(move || artwork::run(art));
    }
    // The demo plays its games through a scripted process list.
    let demo_processes = demo::DemoProcesses::default();
    let source: Box<dyn savescummer_monitor::ProcessSource> =
        if opts.demo { Box::new(demo_processes.clone()) } else { savescummer_monitor::system_source() };
    let monitor = savescummer_monitor::Monitor::new(source);
    {
        let monitor_host = host.clone();
        std::thread::spawn(move || monitoring::run(monitor_host, monitor));
    }
    // Let the monitor take its first look before accepting operations, so a
    // game already running is on the stack.
    std::thread::sleep(Duration::from_millis(opts.poll_ms.min(500) + 50));
    {
        let mut inner = host.lock();
        ops::mark_ready(&mut inner);
        host.publish(&mut inner);
    }
    if !opts.no_integrations {
        feedback::start(&host);
    }
    if !opts.no_integrations || opts.watch {
        let watch_host = Arc::downgrade(&host);
        let watcher = savescummer_platform::watch::Watcher::new(Box::new(move || {
            if let Some(host) = watch_host.upgrade() {
                host.scans.request(false, false, "a store folder or registry key changed");
            }
        }));
        watcher.set_paths(host.env.watch_locations());
        watcher.set_registry_keys(scan::registry_keys(&host));
        *host.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
    }
    ready_line(true, "", Some(&host));
    if !opts.minimized {
        feedback::show_ui(&host);
    }
    if opts.demo {
        let demo_host = host.clone();
        std::thread::spawn(move || demo::drive(demo_host, demo_processes));
    }

    // The main thread belongs to the OS's event loop where the OS needs one
    // there (macOS); elsewhere it just waits for shutdown.
    savescummer_platform::integration::run_main_loop(move || {
        let _ = shutdown_rx.recv();
    });
    shutdown(&host);
    runtime.shutdown_timeout(Duration::from_millis(500));
    ExitCode::SUCCESS
}

/// Exit, safely: stop accepting operations, let running ones reach a safe
/// point, run remaining delete countdowns early, tell clients, then exit.
fn shutdown(host: &Arc<Host>) {
    {
        let mut inner = host.lock();
        inner.phase = Phase::ShuttingDown;
        host.publish(&mut inner);
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let inner = host.lock();
        if inner.busy.is_empty() && inner.deletes.is_empty() && !inner.store_moving {
            break;
        }
        drop(inner);
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = host.events_tx.send(EventBody::Shutdown);
    // Give watchers a moment to receive the notice.
    std::thread::sleep(Duration::from_millis(150));
    host.scans.stop();
    feedback::stop(host);
    *host.watcher.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let _ = db::end_run(host.db().conn(), &host.instance, &host::now());
    trace("host stopped");
}

/// How often a running host records that it is still alive. A crash leaves
/// the run's end unknown within this much.
const HEARTBEAT: Duration = Duration::from_secs(300);

fn run_heartbeat(host: Arc<Host>) {
    loop {
        std::thread::sleep(HEARTBEAT);
        if host.lock().phase == Phase::ShuttingDown {
            return;
        }
        let _ = db::touch_run(host.db().conn(), &host.instance, &host::now());
    }
}

/// Startup and shutdown progress, for diagnosing a host that doesn't
/// become ready. Goes to the host log (see [`log`]).
pub fn trace(step: &str) {
    log::line(step);
}
