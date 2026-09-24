//! Queries and small commands: history pages, the Flush preview, save sets,
//! labels, settings, opening folders and the catalog.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use savescummer_core::history::RowKind;
use savescummer_core::{ErrorKind, Failure, Presence, labels};
use savescummer_ipc::{
    CatalogInfo, FlushItem, FlushPreview, HistoryEntry, HistoryPage, HostRun, HostRuns, OpStatus, OpenTarget, Opened,
    RowActions, SaveSetInfo, TargetInfo,
};
use savescummer_storage::{self as db, CheckpointRow};

use crate::checkpoints::unavailable_reason;
use crate::host::Host;
use crate::model::{SETTING_LAUNCH, SETTING_PLAY_SOUNDS};

const DEFAULT_PAGE: usize = 50;
const MAX_PAGE: usize = 500;

/// A page position: one game, one host instance, one version of that game's
/// history, and where the last page ended.
fn cursor(instance: &str, game: &str, version: u64, seq: i64) -> String {
    format!("{instance}|{game}|{version}|{seq}")
}

pub fn history(
    host: &Arc<Host>,
    game: &str,
    position: Option<&str>,
    limit: Option<usize>,
) -> Result<HistoryPage, Failure> {
    let inner = host.lock();
    let game_id = host.find_game(&inner, game)?;
    let version = inner.caches.get(&game_id).map(|c| c.history_version).unwrap_or(0);
    let before = match position {
        None => None,
        Some(text) => {
            let parts: Vec<&str> = text.split('|').collect();
            let valid =
                parts.len() == 4 && parts[0] == host.instance && parts[1] == game_id && parts[2] == version.to_string();
            if !valid {
                return Err(
                    Failure::new(ErrorKind::Reload, "the history changed; read it again from the start").game(&game_id)
                );
            }
            Some(parts[3].parse::<i64>().map_err(|_| Failure::new(ErrorKind::InvalidRequest, "bad page position"))?)
        }
    };
    let limit = limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let ci = host.env.case_insensitive();
    let game_blocked = inner.blocked.contains_key(&game_id);
    let game_busy = inner.busy.contains_key(&game_id) || inner.store_moving;
    let config_ok = inner.derived.get(&game_id).is_some_and(|d| d.active.is_ok());
    let counting: Vec<String> = inner.deletes.values().filter_map(|d| d.op.checkpoint.clone()).collect();

    let storage = host.db();
    let rows = db::history_page(storage.conn(), &game_id, before, limit + 1).map_err(io)?;
    let has_more = rows.len() > limit;
    let rows = &rows[..rows.len().min(limit)];
    let mut checkpoints: HashMap<String, Option<CheckpointRow>> = HashMap::new();
    let mut lookup = |id: &Option<String>| -> Option<CheckpointRow> {
        let id = id.as_ref()?;
        checkpoints.entry(id.clone()).or_insert_with(|| db::checkpoint(storage.conn(), id).ok().flatten()).clone()
    };
    let mut entries = Vec::new();
    for row in rows {
        let (own, about) = match row.kind {
            RowKind::Saved => (lookup(&row.checkpoint_id), lookup(&row.checkpoint_id)),
            RowKind::Loaded | RowKind::Reverted => (lookup(&row.recovery_id), lookup(&row.checkpoint_id)),
            _ => (None, None),
        };
        let reverted_at = match (&row.kind, &row.reverted_row) {
            (RowKind::Reverted, Some(r)) => db::row(storage.conn(), r).ok().flatten().map(|r| r.at),
            _ => None,
        };
        let unavailable = own.as_ref().and_then(|c| unavailable_reason(c, &inner, ci));
        let deleting = own.as_ref().is_some_and(|c| counting.contains(&c.id));
        let operable = own.is_some() && unavailable.is_none() && !game_blocked && !game_busy && config_ok;
        let actions = match row.kind {
            RowKind::Saved => {
                RowActions { load: operable, revert: false, delete: own.is_some() && !deleting && !game_blocked }
            }
            RowKind::Loaded | RowKind::Reverted => {
                RowActions { load: false, revert: operable, delete: own.is_some() && !deleting && !game_blocked }
            }
            _ => RowActions { load: false, revert: false, delete: false },
        };
        let (label, saved_at) = match row.kind {
            RowKind::Saved | RowKind::Loaded => {
                (about.as_ref().and_then(|c| c.label.clone()), about.as_ref().map(|c| c.created_at.clone()))
            }
            RowKind::Reverted => (None, about.as_ref().map(|c| c.created_at.clone())),
            _ => (None, None),
        };
        entries.push(HistoryEntry {
            id: row.id.clone(),
            kind: row.kind,
            at: row.at.clone(),
            checkpoint: row.owned_checkpoint().map(str::to_string),
            restored: matches!(row.kind, RowKind::Loaded | RowKind::Reverted)
                .then(|| row.checkpoint_id.clone())
                .flatten(),
            label,
            saved_at,
            reverted_at,
            removed_files: matches!(row.kind, RowKind::Loaded | RowKind::Reverted).then_some(row.removed),
            cloud_replaced: row.cloud_replaced,
            unavailable,
            deleting,
            actions,
        });
    }
    let next = if has_more { rows.last().map(|r| cursor(&host.instance, &game_id, version, r.seq)) } else { None };
    Ok(HistoryPage { rows: entries, next })
}

fn io(e: impl std::fmt::Display) -> Failure {
    Failure::new(ErrorKind::Io, e.to_string())
}

pub fn flush_preview(
    host: &Arc<Host>,
    game: &str,
    position: Option<&str>,
    limit: Option<usize>,
) -> Result<FlushPreview, Failure> {
    let (game_id, store, folder) = {
        let inner = host.lock();
        let id = host.find_game(&inner, game)?;
        let folder = host.game_store_dir(&inner, &id).expect("known game");
        (id, inner.store.clone(), folder)
    };
    let records: Vec<CheckpointRow> =
        db::all_live_checkpoints(host.db().conn()).map_err(io)?.into_iter().filter(|c| c.game_id == game_id).collect();
    let mut items: Vec<FlushItem> = records
        .iter()
        .map(|c| FlushItem {
            path: store.join(&c.folder).to_string_lossy().into_owned(),
            kind: if c.state == "deleting" { "temporary".into() } else { c.kind.clone() },
            label: c.label.clone(),
        })
        .collect();
    let mut size: u64 = records.iter().map(|c| c.size).sum();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if savescummer_snapshots::is_reserved_folder(&name) {
                size += savescummer_snapshots::folder_size(&entry.path());
                items.push(FlushItem {
                    path: entry.path().to_string_lossy().into_owned(),
                    kind: "temporary".into(),
                    label: None,
                });
            }
        }
    }
    let count = |kind: &str| items.iter().filter(|i| i.kind == kind).count();
    let (saved, recovery, temporary) = (count("saved"), count("recovery"), count("temporary"));
    let offset: usize = position.and_then(|p| p.parse().ok()).unwrap_or(0);
    let limit = limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let page: Vec<FlushItem> = items.iter().skip(offset).take(limit).cloned().collect();
    let next = (offset + limit < items.len()).then(|| (offset + limit).to_string());
    Ok(FlushPreview { saved, recovery, temporary, size, items: page, next })
}

fn target_info(t: &savescummer_core::Target) -> TargetInfo {
    TargetInfo {
        root: t.root.to_string_lossy().into_owned(),
        filter: t.filter.clone(),
        excludes: t.excludes.clone(),
        presence: t.presence,
    }
}

pub fn save_set(host: &Arc<Host>, game: &str) -> Result<SaveSetInfo, Failure> {
    let mut inner = host.lock();
    let game_id = host.find_game(&inner, game)?;
    crate::library::derive_one(host, &mut inner, &game_id);
    let game = inner.game(&game_id)?;
    let (catalog, catalog_problem) = match &game.outcome {
        Some(savescummer_catalog::Outcome::Resolved { save_set }) => (
            Some(
                save_set
                    .iter()
                    .map(|t| {
                        let mut t = t.clone();
                        t.presence = savescummer_snapshots::presence(&t.root);
                        target_info(&t)
                    })
                    .collect(),
            ),
            None,
        ),
        Some(savescummer_catalog::Outcome::Unsupported { reason }) => (None, Some(reason.clone())),
        None => (None, None),
    };
    let derived = inner.derived.get(&game_id).cloned().unwrap_or_default();
    Ok(SaveSetInfo {
        catalog,
        catalog_problem,
        location: game.location.as_ref().map(|l| l.text.clone()),
        active: derived.active.as_ref().map(|ts| ts.iter().map(target_info).collect()).unwrap_or_default(),
        config_error: derived.active.err(),
        warnings: derived.warnings,
        context: game.context.as_ref().map(|c| serde_json::to_value(c).expect("context serializes")),
    })
}

/// Sets or clears a saved checkpoint's label. Not an operation: it changes
/// no files and doesn't wait for the game's lock.
pub fn set_label(host: &Arc<Host>, checkpoint: &str, label: Option<&str>) -> Result<serde_json::Value, Failure> {
    let label = label.and_then(labels::normalize);
    let record = db::checkpoint(host.db().conn(), checkpoint).map_err(io)?;
    let changed = db::set_label(host.db().conn(), checkpoint, label.as_deref()).map_err(io)?;
    if !changed {
        return Err(Failure::new(ErrorKind::Gone, "the checkpoint no longer exists"));
    }
    let game = record.map(|r| r.game_id).unwrap_or_default();
    let mut inner = host.lock();
    host.refresh_cache(&mut inner, &game);
    host.bump_labels(&mut inner, &game);
    host.publish(&mut inner);
    Ok(serde_json::json!({ "checkpoint": checkpoint, "label": label }))
}

pub fn settings(
    host: &Arc<Host>,
    play_sounds: Option<bool>,
    launch: Option<bool>,
) -> Result<serde_json::Value, Failure> {
    if let Some(on) = launch {
        let exe = std::env::current_exe().map_err(io)?;
        if host.opts.no_integrations {
            return Err(Failure::new(ErrorKind::InvalidRequest, "sign-in changes are off (--no-integrations)"));
        }
        savescummer_platform::autostart::set(on, &exe, host.opts.data_dir.as_deref())
            .map_err(|e| Failure::new(ErrorKind::InvalidRequest, e))?;
        let _ = db::set_setting(host.db().conn(), SETTING_LAUNCH, if on { "1" } else { "0" });
        host.lock().launch_on_startup = on;
    }
    if let Some(on) = play_sounds {
        db::set_setting(host.db().conn(), SETTING_PLAY_SOUNDS, if on { "1" } else { "0" }).map_err(io)?;
        host.lock().play_sounds = on;
    }
    let mut inner = host.lock();
    host.publish(&mut inner);
    Ok(serde_json::json!({ "play_sounds": inner.play_sounds, "launch_on_startup": inner.launch_on_startup }))
}

/// Resolves what to open to a real folder; a folder that doesn't exist opens
/// its nearest existing parent.
pub fn open(host: &Arc<Host>, target: &OpenTarget, resolve_only: bool) -> Result<Opened, Failure> {
    let path: PathBuf = {
        let mut inner = host.lock();
        match target {
            OpenTarget::TargetRoot { game, target } => {
                let id = host.find_game(&inner, game)?;
                crate::library::derive_one(host, &mut inner, &id);
                let derived = inner.derived.get(&id).cloned().unwrap_or_default();
                match derived.active {
                    Ok(targets) => targets
                        .get(*target)
                        .map(|t| t.root.clone())
                        .ok_or_else(|| Failure::new(ErrorKind::NotFound, "no such target"))?,
                    Err(f) => f.paths.first().map(PathBuf::from).ok_or(f)?,
                }
            }
            OpenTarget::Checkpoints { game } => {
                let id = host.find_game(&inner, game)?;
                host.game_store_dir(&inner, &id).expect("known game")
            }
            OpenTarget::Checkpoint { checkpoint } => {
                let record = db::checkpoint(host.db().conn(), checkpoint)
                    .map_err(io)?
                    .ok_or_else(|| Failure::new(ErrorKind::NotFound, "no such checkpoint"))?;
                inner.store.join(record.folder)
            }
            OpenTarget::Executable { game } => {
                let id = host.find_game(&inner, game)?;
                let exe = inner
                    .game(&id)?
                    .main_executable()
                    .ok_or_else(|| Failure::new(ErrorKind::NotFound, "no executable"))?;
                exe.parent().map(|p| p.to_path_buf()).unwrap_or(exe)
            }
        }
    };
    let existing = nearest_existing(&path);
    let opened = if resolve_only { false } else { savescummer_platform::open_folder(&existing).is_ok() };
    Ok(Opened { path: existing.to_string_lossy().into_owned(), opened })
}

fn nearest_existing(path: &std::path::Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if savescummer_snapshots::presence(ancestor) == Presence::Present && ancestor.is_dir() {
            return ancestor.to_path_buf();
        }
    }
    path.to_path_buf()
}

pub fn catalog(host: &Arc<Host>) -> CatalogInfo {
    let catalog = host.catalog.read().unwrap_or_else(|e| e.into_inner());
    CatalogInfo {
        repo: catalog.bundle.source.repo.clone(),
        revision: catalog.bundle.source.revision.clone(),
        games: catalog.bundle.games.len(),
        source: catalog.source.clone(),
    }
}

/// Re-reads a downloaded bundle from the data folder, and rescans when its
/// revision changed. Fetching bundles from the network isn't built yet.
pub fn catalog_refresh(host: &Arc<Host>) -> Result<serde_json::Value, Failure> {
    let path = host.data_dir.join("catalog").join("catalog.json");
    let changed = match std::fs::read_to_string(&path) {
        Ok(text) => {
            let bundle = savescummer_catalog::Bundle::parse(&text)
                .map_err(|e| Failure::new(ErrorKind::InvalidRequest, e.to_string()))?;
            let mut catalog = host.catalog.write().unwrap_or_else(|e| e.into_inner());
            let changed = catalog.bundle.source.revision != bundle.source.revision || *catalog.bundle != bundle;
            if changed {
                catalog.bundle = Arc::new(bundle);
                catalog.source = "downloaded".into();
            }
            changed
        }
        Err(_) => false,
    };
    if changed {
        let job = host.scans.request(true, true, "the catalog changed");
        host.scans.wait(job, std::time::Duration::from_secs(600));
    }
    let info = catalog(host);
    Ok(serde_json::json!({ "changed": changed, "catalog": info }))
}

#[allow(dead_code)]
fn status_final(status: OpStatus) -> bool {
    status.is_final()
}

/// When the host was running, newest first.
pub fn host_runs(host: &Arc<Host>, limit: Option<usize>) -> Result<HostRuns, Failure> {
    let limit = limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let rows = db::runs(host.db().conn(), limit).map_err(|e| Failure::new(ErrorKind::Io, e.to_string()))?;
    let runs = rows
        .into_iter()
        .map(|r| HostRun {
            current: r.id == host.instance,
            id: r.id,
            started_at: r.started_at,
            last_seen_at: r.last_seen_at,
            ended_at: r.ended_at,
        })
        .collect();
    Ok(HostRuns { runs })
}
