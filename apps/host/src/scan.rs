//! The scan worker. Scans run on their own thread, one at a time, and never
//! hold up requests or monitoring. A request during a scan joins it and
//! gets that scan's result; a full scan requested during an install scan
//! runs right after it.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use savescummer_ipc::{ScanOrigin, ScanResult};

use savescummer_storage as db;

use crate::host::{Host, now};
use crate::model::SETTING_DRIVES;

#[derive(Debug, Clone)]
struct Job {
    id: u64,
    full: bool,
    user: bool,
    /// Why it runs, for the log. Requests that join a scan add theirs.
    reasons: Vec<&'static str>,
}

#[derive(Default)]
struct Queue {
    running: Option<Job>,
    next: Option<Job>,
    counter: u64,
    results: HashMap<u64, ScanResult>,
    stopped: bool,
}

#[derive(Default)]
pub struct ScanQueue {
    queue: Mutex<Queue>,
    changed: Condvar,
}

impl ScanQueue {
    /// Asks for a scan and returns the id of the scan that will answer it.
    pub fn request(&self, full: bool, user: bool, reason: &'static str) -> u64 {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(running) = &mut q.running
            && (running.full || !full)
        {
            running.user |= user;
            if !running.reasons.contains(&reason) {
                running.reasons.push(reason);
            }
            return running.id;
        }
        if let Some(next) = &mut q.next {
            next.full |= full;
            next.user |= user;
            if !next.reasons.contains(&reason) {
                next.reasons.push(reason);
            }
            return next.id;
        }
        q.counter += 1;
        let id = q.counter;
        q.next = Some(Job { id, full, user, reasons: vec![reason] });
        self.changed.notify_all();
        id
    }

    /// Waits for a scan's result.
    pub fn wait(&self, id: u64, timeout: Duration) -> Option<ScanResult> {
        let deadline = std::time::Instant::now() + timeout;
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(result) = q.results.get(&id) {
                return Some(result.clone());
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() || q.stopped {
                return None;
            }
            q = self.changed.wait_timeout(q, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    pub fn is_running(&self) -> bool {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).running.is_some()
    }

    pub fn stop(&self) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).stopped = true;
        self.changed.notify_all();
    }

    fn take(&self) -> Option<Job> {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if q.stopped {
                return None;
            }
            if let Some(job) = q.next.take() {
                q.running = Some(job.clone());
                return Some(job);
            }
            q = self.changed.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn finish(&self, result: ScanResult) -> Job {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let job = q.running.take().expect("a scan was running");
        q.results.insert(job.id, result);
        // Keep only recent results.
        let oldest = job.id.saturating_sub(64);
        q.results.retain(|id, _| *id > oldest);
        self.changed.notify_all();
        job
    }

    fn running_user(&self) -> bool {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).running.as_ref().is_some_and(|j| j.user)
    }
}

/// The worker loop: runs queued scans until the host stops.
pub fn worker(host: Arc<Host>) {
    while let Some(job) = host.scans.take() {
        let origin = if job.user { ScanOrigin::User } else { ScanOrigin::Background };
        {
            let mut inner = host.lock();
            if job.user {
                inner.scan.running = Some(origin.clone());
                inner.scan.running_full = Some(job.full);
            }
            host.publish(&mut inner);
        }
        let new_games = run(&host, job.full, job.user, &job.reasons.join(", "));
        // A user request may have joined while the scan ran.
        let user = job.user || host.scans.running_user();
        let result = ScanResult {
            origin: if user { ScanOrigin::User } else { ScanOrigin::Background },
            full: job.full,
            new_games,
            finished_at: now(),
        };
        host.scans.finish(result.clone());
        let mut inner = host.lock();
        inner.scan.running = None;
        inner.scan.running_full = None;
        inner.scan.scans += 1;
        if job.full {
            inner.scan.full_scans += 1;
        }
        if user {
            inner.scan.last_user = Some(result);
        }
        host.publish(&mut inner);
        drop(inner);
        crate::artwork::request(&host);
    }
}

/// One scan: an install scan, plus re-checking every checkpoint on disk for
/// a full scan. A user's scan may ask macOS for access to what games wait
/// for; any other only notifies.
pub fn run(host: &Arc<Host>, full: bool, user: bool, reason: &str) -> usize {
    crate::checkpoints::check_store(host);
    let new_games = crate::library::scan(host, full, reason);
    crate::privacy::after_scan(host, user);
    if full {
        crate::checkpoints::verify_all(host);
        crate::checkpoints::clean_up(host);
    }
    if let Some(watcher) = host.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        watcher.set_paths(host.env.watch_locations());
        watcher.set_registry_keys(registry_keys(host));
    }
    remember_drives(host);
    new_games
}

/// Remembers the drives of everything the host relies on (PLAN-HOST, "A
/// drive the host has seen stays expected"), so a drive unplugged later reads
/// as disconnected rather than empty. Kept in the database, so it holds for a
/// host started while the drive is out.
pub fn remember_drives(host: &Host) {
    let mut used = host.env.steam_libraries();
    {
        let inner = host.lock();
        used.push(inner.store.clone());
        for game in inner.games.values() {
            used.extend(game.installs.iter().map(|i| i.install_dir.clone()));
        }
        for derived in inner.derived.values() {
            if let Ok(targets) = &derived.active {
                used.extend(targets.iter().map(|t| t.root.clone()));
            }
        }
    }
    if savescummer_snapshots::remember_drives(&used) {
        let drives = serde_json::to_string(&savescummer_snapshots::remembered_drives()).expect("paths serialize");
        if let Err(e) = host.db().write(|c| db::set_setting(c, SETTING_DRIVES, &drives)) {
            crate::trace(&format!("can't record the drives in use: {e}"));
        }
    }
}

/// The registry keys to watch, in the platform crate's terms. A key that
/// appeared since the last scan (GOG Galaxy installed) is picked up here.
pub fn registry_keys(host: &Host) -> Vec<savescummer_platform::watch::RegistryKey> {
    use savescummer_platform::watch::{Hive, RegistryKey};
    host.env
        .watch_registry_keys()
        .into_iter()
        .map(|k| RegistryKey {
            hive: match k.hive {
                savescummer_scanner::Hive::LocalMachine => Hive::LocalMachine,
                savescummer_scanner::Hive::CurrentUser => Hive::CurrentUser,
            },
            path: k.path,
        })
        .collect()
}

/// The periodic full scan, every 15 minutes, with or without a UI.
pub fn periodic(host: Arc<Host>) {
    let interval = Duration::from_secs(host.opts.scan_interval_secs.max(1));
    loop {
        std::thread::sleep(interval);
        if host.lock().phase != savescummer_ipc::Phase::Ready {
            if host.lock().phase == savescummer_ipc::Phase::ShuttingDown {
                return;
            }
            continue;
        }
        host.scans.request(true, false, "periodic scan");
    }
}
