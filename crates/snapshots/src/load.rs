//! Applying a checkpoint (PLAN-HOST.md, LOAD): four stages, each finished for
//! every file before the next starts, so a failure always leaves a state
//! that can be undone by reversing renames.
//!
//! 1. Copy in: each checkpoint file next to its live counterpart as `.ssnew`.
//! 2. Set aside: each live file the Load replaces or deletes to `.ssold`.
//! 3. Swap in: each `.ssnew` to its real name.
//! 4. Clean up: delete the `.ssold` files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use savescummer_core::common::RecordedTarget;
use savescummer_core::recovery::{Copy, Entry as RecoveryEntry, Original, Rule};
use savescummer_core::{ErrorKind, Failure, Filter, Presence, SUFFIX_NEW, SUFFIX_OLD, Target};

use crate::checkpoint::Hook;
use crate::fsx::{self, CopyError, with_suffix};
use crate::walk::{Entry, walk_target};

/// One file a Load covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadFile {
    pub live: PathBuf,
    /// The checkpoint file restored here; None when the Load deletes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// Identity of the live file this Load replaces or deletes; None when
    /// the Load adds a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    /// Identity of the `.ssnew` copy, known once stage 1 finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<String>,
}

impl LoadFile {
    pub fn new_path(&self) -> PathBuf {
        with_suffix(&self.live, SUFFIX_NEW)
    }

    pub fn old_path(&self) -> PathBuf {
        with_suffix(&self.live, SUFFIX_OLD)
    }
}

/// Everything a Load will do, recorded in the journal before any change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadPlan {
    pub files: Vec<LoadFile>,
    /// Folders the checkpoint has and the live saves don't, parents first.
    pub create_dirs: Vec<PathBuf>,
    /// Matched folders the checkpoint doesn't have, deepest first; removed
    /// after stage 4 when empty.
    pub remove_dirs: Vec<PathBuf>,
}

impl LoadPlan {
    /// How many live files the Load deletes (kept in the recovery point).
    pub fn removed_files(&self) -> usize {
        self.files.iter().filter(|f| f.source.is_none()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.create_dirs.is_empty() && self.remove_dirs.is_empty()
    }
}

fn key(rel: &[String], ci: bool) -> String {
    let text = rel.join("/");
    if ci { text.to_lowercase() } else { text }
}

/// Plans a Load of `checkpoint_dir` into the current targets it has in
/// common. Checks everything before anything is touched.
pub fn plan_load(checkpoint_dir: &Path, pairs: &[(RecordedTarget, Target)], ci: bool) -> Result<LoadPlan, Failure> {
    let mut plan = LoadPlan::default();
    for (recorded, current) in pairs {
        if recorded.absent {
            continue; // absent at Save time: left alone
        }
        match fsx::presence(&current.root) {
            Presence::Present => {}
            Presence::Missing => {
                return Err(Failure::new(ErrorKind::RootMissing, "a save location that held data is missing")
                    .path(&current.root));
            }
            Presence::Unknown => {
                return Err(
                    Failure::new(ErrorKind::TargetUnavailable, "the save location can't be read").path(&current.root)
                );
            }
        }
        let base = checkpoint_dir.join(&recorded.folder);
        if fsx::presence(&base) != Presence::Present {
            return Err(Failure::new(ErrorKind::CheckpointChanged, "the checkpoint is missing a part").path(&base));
        }
        // The checkpoint holds exactly what was matched; the current excludes
        // still apply, so a Load never restores what's now a setting.
        let saved = walk_target(&base, &Filter::All, &current.excludes, ci)
            .map_err(|f| Failure { kind: ErrorKind::CheckpointChanged, ..f })?;
        let saved: Vec<Entry> = saved.into_iter().filter(|e| current.filter.covers(&rel_refs(e), ci)).collect();
        let live = walk_target(&current.root, &current.filter, &current.excludes, ci)?;

        let saved_map: BTreeMap<String, &Entry> = saved.iter().map(|e| (key(&e.rel, ci), e)).collect();
        let live_map: BTreeMap<String, &Entry> = live.iter().map(|e| (key(&e.rel, ci), e)).collect();

        for (k, s) in &saved_map {
            if let Some(l) = live_map.get(k)
                && l.is_dir != s.is_dir
            {
                return Err(Failure::new(ErrorKind::KindConflict, "a file and a folder with the same name")
                    .path(join(&current.root, &l.rel)));
            }
        }

        for s in &saved {
            let live_path = join(&current.root, &s.rel);
            if s.is_dir {
                if !live_map.contains_key(&key(&s.rel, ci)) {
                    plan.create_dirs.push(live_path);
                }
                continue;
            }
            let original = match live_map.get(&key(&s.rel, ci)) {
                Some(_) => Some(fsx::identity(&live_path).ok_or_else(|| {
                    Failure::new(ErrorKind::ReadFailed, "can't identify a save file").path(&live_path)
                })?),
                None => None,
            };
            plan.files.push(LoadFile { live: live_path, source: Some(join(&base, &s.rel)), original, copy: None });
        }
        for l in &live {
            if saved_map.contains_key(&key(&l.rel, ci)) {
                continue;
            }
            let live_path = join(&current.root, &l.rel);
            if l.is_dir {
                plan.remove_dirs.push(live_path);
            } else {
                let original = fsx::identity(&live_path).ok_or_else(|| {
                    Failure::new(ErrorKind::ReadFailed, "can't identify a save file").path(&live_path)
                })?;
                plan.files.push(LoadFile { live: live_path, source: None, original: Some(original), copy: None });
            }
        }
    }
    plan.create_dirs.sort_by_key(|p| p.components().count());
    plan.remove_dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    // Our reserved names must be free, or a leftover would be mistaken for
    // this Load's material.
    for file in &plan.files {
        for reserved in [file.new_path(), file.old_path()] {
            if fs::symlink_metadata(&reserved).is_ok() {
                return Err(
                    Failure::new(ErrorKind::Io, "a leftover from an earlier load is in the way").path(&reserved)
                );
            }
        }
    }
    Ok(plan)
}

fn rel_refs(e: &Entry) -> Vec<&str> {
    e.rel.iter().map(String::as_str).collect()
}

fn join(base: &Path, rel: &[String]) -> PathBuf {
    let mut path = base.to_path_buf();
    for part in rel {
        path.push(part);
    }
    path
}

/// A stage failure: what went wrong, and whether everything was undone.
#[derive(Debug)]
pub struct StageError {
    pub failure: Failure,
    /// False when undoing itself failed: the game must stay blocked and
    /// every file is kept as it is.
    pub undone: bool,
}

/// Stage 1: copy in. On failure the `.ssnew` copies are deleted; the live
/// saves were never touched.
pub fn stage_copy_in(plan: &mut LoadPlan, hook: Hook<'_>) -> Result<(), StageError> {
    for dir in &plan.create_dirs {
        if let Err(e) = fs::create_dir_all(dir) {
            let failure = fsx::failure(fsx::write_kind(&e), &e, dir);
            let undone = undo_copy_in(plan).is_ok();
            return Err(StageError { failure, undone });
        }
    }
    for i in 0..plan.files.len() {
        let Some(source) = plan.files[i].source.clone() else { continue };
        let dest = plan.files[i].new_path();
        if let Some(parent) = dest.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            let failure = fsx::failure(fsx::write_kind(&e), &e, parent);
            let undone = undo_copy_in(plan).is_ok();
            return Err(StageError { failure, undone });
        }
        if let Err(e) = fsx::copy_file(&source, &dest) {
            let failure = match e {
                CopyError::Read(e) => fsx::failure(ErrorKind::CheckpointChanged, &e, &source),
                CopyError::Write(e) => fsx::failure(fsx::write_kind(&e), &e, &dest),
            };
            let undone = undo_copy_in(plan).is_ok();
            return Err(StageError { failure, undone });
        }
        plan.files[i].copy = fsx::identity(&dest);
        hook("load.copy_in", i + 1);
    }
    Ok(())
}

fn undo_copy_in(plan: &LoadPlan) -> Result<(), Failure> {
    let mut first_error = None;
    for file in &plan.files {
        let path = file.new_path();
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                first_error.get_or_insert(fsx::failure(ErrorKind::RollbackFailed, &e, &path));
            }
        }
    }
    for dir in plan.create_dirs.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
    first_error.map_or(Ok(()), Err)
}

/// Stage 2: set aside. On failure the renamed files go back and the copies
/// are deleted; nothing was lost.
pub fn stage_set_aside(plan: &LoadPlan, hook: Hook<'_>) -> Result<(), StageError> {
    let mut done = Vec::new();
    for (i, file) in plan.files.iter().enumerate() {
        if file.original.is_none() {
            continue;
        }
        if let Err(e) = fsx::rename_noreplace(&file.live, &file.old_path()) {
            let failure = fsx::failure(fsx::rename_kind(&e), &e, &file.live);
            let undone = undo_set_aside(plan, &done).is_ok() && undo_copy_in(plan).is_ok();
            return Err(StageError { failure, undone });
        }
        done.push(i);
        hook("load.set_aside", done.len());
    }
    Ok(())
}

fn undo_set_aside(plan: &LoadPlan, done: &[usize]) -> Result<(), Failure> {
    let mut first_error = None;
    for &i in done.iter().rev() {
        let file = &plan.files[i];
        if let Err(e) = fsx::rename_noreplace(&file.old_path(), &file.live) {
            first_error.get_or_insert(fsx::failure(ErrorKind::RollbackFailed, &e, &file.old_path()));
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Stage 3: swap in. On failure the swapped files go back to `.ssnew`, then
/// stage 2 and stage 1 are undone.
pub fn stage_swap_in(plan: &LoadPlan, hook: Hook<'_>) -> Result<(), StageError> {
    let mut done = Vec::new();
    for (i, file) in plan.files.iter().enumerate() {
        if file.source.is_none() {
            continue;
        }
        if let Err(e) = fsx::rename_noreplace(&file.new_path(), &file.live) {
            let failure = Failure::new(ErrorKind::SwapFailed, e.to_string()).path(&file.live);
            let mut undone = true;
            for &j in done.iter().rev() {
                let f: &LoadFile = &plan.files[j];
                if fsx::rename_noreplace(&f.live, &f.new_path()).is_err() {
                    undone = false;
                }
            }
            let set_aside: Vec<usize> =
                plan.files.iter().enumerate().filter(|(_, f)| f.original.is_some()).map(|(i, _)| i).collect();
            undone = undone && undo_set_aside(plan, &set_aside).is_ok() && undo_copy_in(plan).is_ok();
            return Err(StageError { failure, undone });
        }
        done.push(i);
        hook("load.swap_in", done.len());
    }
    Ok(())
}

/// Stage 4: clean up. A failure here doesn't undo the Load; the leftover
/// `.ssold` files are returned and removed later.
pub fn stage_clean_up(plan: &LoadPlan, hook: Hook<'_>) -> Vec<PathBuf> {
    let mut leftovers = Vec::new();
    for (i, file) in plan.files.iter().enumerate() {
        if file.original.is_none() {
            continue;
        }
        let old = file.old_path();
        match fs::remove_file(&old) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => leftovers.push(old),
        }
        hook("load.clean_up", i + 1);
    }
    for dir in &plan.remove_dirs {
        let _ = fs::remove_dir(dir);
    }
    leftovers
}

/// Where each covered file is now, judged by the recorded identities.
pub fn observe(plan: &LoadPlan) -> Vec<RecoveryEntry> {
    plan.files
        .iter()
        .map(|file| {
            let live = fsx::identity(&file.live);
            let old = fsx::identity(&file.old_path());
            let new = fsx::identity(&file.new_path());
            let original = file.original.as_ref().map(|id| {
                if live.as_ref() == Some(id) {
                    Original::AtLive
                } else if old.as_ref() == Some(id) {
                    Original::AtOld
                } else if old.is_none() && (live.is_none() || live == file.copy) {
                    Original::Gone
                } else {
                    Original::Unknown
                }
            });
            let copy = file.source.as_ref().map(|_| match &file.copy {
                Some(id) if new.as_ref() == Some(id) => Copy::AtNew,
                Some(id) if live.as_ref() == Some(id) => Copy::AtLive,
                Some(_) if new.is_none() => Copy::Missing,
                Some(_) => Copy::Unknown,
                // Stage 1 didn't finish: whatever copy exists is partial.
                None if new.is_some() => Copy::AtNew,
                None => Copy::Missing,
            });
            RecoveryEntry { original, copy }
        })
        .collect()
}

/// Applies a recovery rule to an interrupted Load's files. Returns an error
/// if the rule couldn't be carried out completely.
pub fn recover(plan: &LoadPlan, rule: Rule) -> Result<Vec<PathBuf>, Failure> {
    match rule {
        Rule::R1 => undo_copy_in(plan).map(|_| Vec::new()),
        Rule::R2 => {
            let states = observe(plan);
            let mut first_error = None;
            for (file, state) in plan.files.iter().zip(&states) {
                if state.copy == Some(Copy::AtLive)
                    && let Err(e) = fsx::rename_noreplace(&file.live, &file.new_path())
                {
                    first_error.get_or_insert(fsx::failure(ErrorKind::RollbackFailed, &e, &file.live));
                }
            }
            for (file, state) in plan.files.iter().zip(&states) {
                if state.original == Some(Original::AtOld)
                    && let Err(e) = fsx::rename_noreplace(&file.old_path(), &file.live)
                {
                    first_error.get_or_insert(fsx::failure(ErrorKind::RollbackFailed, &e, &file.old_path()));
                }
            }
            if let Some(error) = first_error {
                return Err(error);
            }
            undo_copy_in(plan).map(|_| Vec::new())
        }
        Rule::R3 => Ok(stage_clean_up(plan, &crate::checkpoint::no_hook)),
        Rule::R4 => Ok(Vec::new()),
    }
}

/// Whether every covered name has a live file again: after R4 the game is
/// released only then.
pub fn every_name_live(plan: &LoadPlan) -> bool {
    plan.files.iter().filter(|f| f.source.is_some() || f.original.is_some()).all(|f| {
        fs::symlink_metadata(&f.live).is_ok() || (f.source.is_none() && fs::symlink_metadata(f.old_path()).is_err())
    })
}

/// Leftover `.ssold` files in a target from a finished Load whose stage 4
/// failed; removed quietly at the next start or scan.
pub fn find_leftover_old(root: &Path, filter: &Filter, ci: bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_old(root, 0, filter, ci, &mut found);
    found
}

fn collect_old(dir: &Path, depth: usize, filter: &Filter, ci: bool, found: &mut Vec<PathBuf>) {
    if depth > 12 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let Ok(meta) = fs::symlink_metadata(entry.path()) else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if meta.is_file() && name.to_lowercase().ends_with(SUFFIX_OLD) {
            // Only leftovers of names this target covers.
            let base = &name[..name.len() - SUFFIX_OLD.len()];
            let covered = depth > 0 || filter.covers(&[base], ci);
            if covered {
                found.push(entry.path());
            }
        } else if meta.is_dir() && !meta.file_type().is_symlink() {
            let covered = depth > 0 || filter.covers(&[name.as_str()], ci);
            if covered {
                collect_old(&entry.path(), depth + 1, filter, ci, found);
            }
        }
    }
}
