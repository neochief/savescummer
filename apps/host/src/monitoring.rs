//! The monitor thread: keeps the ACTIVE STACK, writes Game started / Game
//! closed markers, re-resolves a Steam game whose account changed, and runs
//! the Steam Cloud check at the first start after a Load.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use savescummer_core::history::RowKind;
use savescummer_core::{Filter, Presence};
use savescummer_ipc::Phase;
use savescummer_monitor::{Event, GameProcesses, Monitor};
use savescummer_snapshots as snap;
use savescummer_storage::{self as db, HistoryRow};

use crate::host::{Host, Inner, new_id, now};

/// Observe persisted games before startup recovery, without scanning live saves.
pub fn observe_startup(host: &Host, monitor: &mut Monitor) -> Vec<Event> {
    monitor.set_games(&games(host));
    let events = monitor.poll();
    let mut inner = host.lock();
    inner.processes = monitor.processes();
    inner.processes_observed = monitor.observation_available();
    events
}

pub fn run(host: Arc<Host>, mut monitor: Monitor, mut initial: Vec<Event>) {
    // Games already running go on the stack in the order the user left them.
    {
        let inner = host.lock();
        initial.sort_by_cached_key(|event| match event {
            Event::Started { game, .. } => {
                let focused = inner.last_focused.get(game).cloned();
                (focused.is_none(), focused.map(std::cmp::Reverse))
            }
            _ => (true, None),
        });
    }
    for event in initial {
        handle(&host, event);
    }
    let refreshing = Arc::new(AtomicBool::new(false));
    let recovery_refreshing = Arc::new(AtomicBool::new(false));
    let interval = Duration::from_millis(host.opts.poll_ms.max(20));
    let mut last_refresh = Instant::now();
    loop {
        if host.lock().phase == Phase::ShuttingDown {
            return;
        }
        if host.monitor_dirty.swap(false, Ordering::SeqCst) {
            monitor.set_games(&games(&host));
        }
        let events = monitor.poll();
        let processes = monitor.processes();
        let changed = {
            let mut inner = host.lock();
            let changed = inner.processes != processes || inner.processes_observed != monitor.observation_available();
            inner.processes_observed = monitor.observation_available();
            if inner.processes != processes {
                inner.processes = processes;
            }
            changed
        };
        // Publish a Started event only after its process IDs are available
        // to operation checks, including the held-file check in expert mode.
        for event in events {
            handle(&host, event);
        }
        {
            let mut inner = host.lock();
            inner.front = monitor.focused().map(str::to_string);
            if inner.stack.settle(monitor.focused(), Instant::now()) || changed {
                host.publish(&mut inner);
            }
        }
        // Saves appear while a game runs: keep Save's availability current.
        if last_refresh.elapsed() >= Duration::from_secs(2) {
            last_refresh = Instant::now();
            let running = host.lock().stack.entries().to_vec();
            if !running.is_empty() && !refreshing.swap(true, Ordering::SeqCst) {
                let worker = host.clone();
                let flag = refreshing.clone();
                std::thread::spawn(move || {
                    for game in running {
                        let _ = crate::library::refresh_game(&worker, &game);
                    }
                    flag.store(false, Ordering::SeqCst);
                });
            }
            if !host.lock().blocked.is_empty() && !recovery_refreshing.swap(true, Ordering::SeqCst) {
                let worker = host.clone();
                let flag = recovery_refreshing.clone();
                std::thread::spawn(move || {
                    crate::recovery::refresh_unavailable_paths(&worker);
                    flag.store(false, Ordering::SeqCst);
                });
            }
        }
        std::thread::sleep(interval);
    }
}

fn games(host: &Host) -> Vec<GameProcesses> {
    let inner = host.lock();
    inner
        .games
        .values()
        .filter(|g| g.installed)
        .map(|g| GameProcesses {
            game: g.id.clone(),
            executables: g.executables(),
            // Any program inside a known game's install folder is the game.
            install_dir: if g.is_custom() { None } else { g.installs.first().map(|i| i.install_dir.clone()) },
        })
        .collect()
}

fn marker(game: &str, kind: RowKind, session: &str, visible: bool) -> HistoryRow {
    HistoryRow {
        seq: 0,
        id: new_id("row"),
        game_id: game.to_string(),
        kind,
        at: now(),
        session: Some(session.to_string()),
        checkpoint_id: None,
        recovery_id: None,
        reverted_row: None,
        removed: 0,
        cloud_check: false,
        cloud_replaced: false,
        visible,
    }
}

/// "Name (id)" for the log.
fn display_name(inner: &Inner, game: &str) -> String {
    match inner.games.get(game) {
        Some(g) => format!("{} ({game})", g.name),
        None => game.to_string(),
    }
}

/// A game waiting for macOS to allow access: seen running, but kept off the
/// stack and out of the history until it's granted.
fn waits_for_access(inner: &Inner, game: &str) -> bool {
    inner.derived.get(game).is_some_and(|d| d.access.is_some())
}

/// Puts running games that just became active on the stack, without a start
/// marker: when they started isn't known to the stack.
pub fn activate_running(inner: &mut Inner) {
    let waiting: Vec<String> =
        inner.processes.keys().filter(|g| !inner.stack.contains(g) && !waits_for_access(inner, g)).cloned().collect();
    for game in waiting {
        inner.stack.started(&game);
        inner.sessions.insert(game.clone(), new_id("session"));
        crate::trace(&format!("game running and now allowed: {}", display_name(inner, &game)));
    }
}

fn handle(host: &Arc<Host>, event: Event) {
    let waiting = match &event {
        Event::Started { game, .. } | Event::Exited { game } | Event::Focused { game } => {
            let inner = host.lock();
            waits_for_access(&inner, game) && !inner.stack.contains(game)
        }
    };
    if waiting {
        if let Event::Started { game, .. } = &event {
            crate::trace(&format!("game started, waiting for access: {}", display_name(&host.lock(), game)));
        }
        return;
    }
    match event {
        Event::Started { game, observed } => {
            let session = new_id("session");
            let name = {
                let mut inner = host.lock();
                inner.stack.started(&game);
                inner.sessions.insert(game.clone(), session.clone());
                // Only an observed start gets a marker: a game already running
                // when the host started has no known start time.
                if observed {
                    let _ = host.db().write(|c| {
                        db::insert_row(c, &marker(&game, RowKind::GameStarted, &session, false)).map(|_| ())
                    });
                }
                // The game runs under whoever is logged into Steam now.
                let changed_account =
                    inner.games.get(&game).is_some_and(|g| crate::library::steam_account_changed(host, g));
                if changed_account {
                    crate::library::reresolve(host, &mut inner, &game);
                    host.bump_history(&mut inner, &game);
                }
                crate::library::refresh_presence(host, &mut inner, std::slice::from_ref(&game));
                host.publish(&mut inner);
                display_name(&inner, &game)
            };
            if observed {
                crate::trace(&format!("game started: {name}"));
                cloud_check(host, &game);
            } else {
                crate::trace(&format!("game already running when the host started: {name} (no start time recorded)"));
            }
        }
        Event::Exited { game } => {
            let mut inner = host.lock();
            inner.processes.remove(&game);
            inner.stack.exited(&game, Instant::now());
            crate::trace(&format!("game closed: {}", display_name(&inner, &game)));
            if let Some(session) = inner.sessions.remove(&game) {
                let storage = host.db();
                let visible: bool = storage
                    .conn()
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM history WHERE session = ?1 AND visible = 1 AND kind NOT IN ('game_started', 'game_closed'))",
                        [&session],
                        |r| r.get(0),
                    )
                    .unwrap_or(false);
                drop(storage);
                let _ = host
                    .db()
                    .write(|c| db::insert_row(c, &marker(&game, RowKind::GameClosed, &session, visible)).map(|_| ()));
                if visible {
                    host.bump_history(&mut inner, &game);
                }
            }
            host.publish(&mut inner);
            drop(inner);
            let worker = host.clone();
            std::thread::spawn(move || {
                let _ = crate::library::refresh_game(&worker, &game);
            });
        }
        Event::Focused { game } => {
            let mut inner = host.lock();
            inner.stack.focused(&game);
            // Kept across exits and restarts: it orders the library.
            let at = now();
            if host.db().write(|c| db::set_last_focused(c, &game, &at)).is_ok() {
                inner.last_focused.insert(game, at);
            }
            host.publish(&mut inner);
        }
    }
}

/// At the first start after a Load, compares the restored files with the
/// checkpoint. If Steam replaced a file or brought a deleted one back, the
/// Loaded row says so. Nothing is changed.
fn cloud_check(host: &Arc<Host>, game: &str) {
    let pending = db::pending_cloud_checks(host.db().conn(), game).unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    let store = host.lock().store.clone();
    let ci = host.env.case_insensitive();
    let targets = host.lock().derived.get(game).and_then(|d| d.active.clone().ok()).unwrap_or_default();
    // Only the newest restore describes the files as they should be now.
    let newest = pending.last().expect("not empty");
    let mut replaced = false;
    if let Some(source) =
        newest.checkpoint_id.as_deref().and_then(|id| db::checkpoint(host.db().conn(), id).ok().flatten())
    {
        let base = store.join(&source.folder);
        for recorded in source.targets.iter().filter(|t| !t.absent) {
            let Some(live) = targets.iter().find(|t| {
                savescummer_core::common::same_path(&t.root, &recorded.root, ci) && t.filter.same(&recorded.filter, ci)
            }) else {
                continue;
            };
            if live.presence != Presence::Present {
                continue;
            }
            let expected =
                listing(snap::walk_target(&base.join(&recorded.folder), &Filter::All, &live.excludes, ci).ok());
            let actual = listing(snap::walk_target(&live.root, &live.filter, &live.excludes, ci).ok());
            if expected != actual {
                replaced = true;
            }
        }
    }
    let _ = host.db().write(|c| {
        for row in &pending {
            let this = row.id == newest.id && replaced;
            db::finish_cloud_check(c, &row.id, this)?;
        }
        Ok(())
    });
    if replaced {
        let mut inner = host.lock();
        host.bump_history(&mut inner, game);
        host.publish(&mut inner);
    }
}

fn listing(entries: Option<Vec<snap::Entry>>) -> Vec<(String, u64, Option<std::time::SystemTime>)> {
    let mut out: Vec<_> = entries
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| (e.rel_text().to_lowercase(), e.size, e.modified.map(truncate)))
        .collect();
    out.sort();
    out
}

/// File systems keep different time precision; compare to the second.
fn truncate(time: std::time::SystemTime) -> std::time::SystemTime {
    let secs = time.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    std::time::UNIX_EPOCH + Duration::from_secs(secs)
}
