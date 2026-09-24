//! Checkpoint folders: copying a save set into the store, the record that
//! makes a checkpoint self-describing, change signatures and deletion by
//! rename.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use savescummer_core::common::RecordedTarget;
use savescummer_core::{ErrorKind, Failure, Filter, Presence, Target};

use crate::fsx::{self, CopyError};
use crate::walk::walk_target;

/// The record written as `checkpoint.json` in every checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub format: u32,
    pub game_id: String,
    pub game_name: String,
    pub kind: String,
    pub created_at: String,
    /// The operation that wrote it, so recovery can recognize a Save that was
    /// published but not recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub targets: Vec<RecordedTarget>,
}

pub const META_FILE: &str = "checkpoint.json";
/// Reserved folder name prefixes in the store: a Save being copied, a folder
/// being deleted, a store move in progress. Never checkpoints.
pub const TEMP_PREFIX: &str = ".ss-tmp-";
pub const DISPOSAL_PREFIX: &str = ".ss-del-";

pub fn is_reserved_folder(name: &str) -> bool {
    name.starts_with(TEMP_PREFIX) || name.starts_with(DISPOSAL_PREFIX)
}

/// A hook the host uses to crash at a named point in tests.
pub type Hook<'a> = &'a dyn Fn(&str, usize);

pub fn no_hook(_: &str, _: usize) {}

/// Copies what every target matches into `dest` (which must not exist),
/// one subfolder per target, and writes the record. Returns the recorded
/// targets and the bytes copied.
pub fn copy_save_set(
    targets: &[Target],
    dest: &Path,
    meta: &mut CheckpointMeta,
    ci: bool,
    hook: Hook<'_>,
) -> Result<u64, Failure> {
    fs::create_dir(dest).map_err(|e| fsx::failure(fsx::write_kind(&e), &e, dest))?;
    let mut used: BTreeSet<String> = BTreeSet::new();
    used.insert(META_FILE.to_lowercase());
    let mut total = 0u64;
    let mut copied = 0usize;
    meta.targets.clear();
    for target in targets {
        let folder = unique_folder(&target_label(target), &mut used);
        let presence = fsx::presence(&target.root);
        let absent = match presence {
            Presence::Present => false,
            Presence::Missing => true,
            Presence::Unknown => {
                return Err(
                    Failure::new(ErrorKind::TargetUnavailable, "the save location can't be read").path(&target.root)
                );
            }
        };
        if !absent {
            let entries = walk_target(&target.root, &target.filter, &target.excludes, ci)?;
            let base = dest.join(&folder);
            fs::create_dir(&base).map_err(|e| fsx::failure(fsx::write_kind(&e), &e, &base))?;
            for entry in entries {
                let mut from = target.root.clone();
                let mut to = base.clone();
                for part in &entry.rel {
                    from.push(part);
                    to.push(part);
                }
                if entry.is_dir {
                    fs::create_dir_all(&to).map_err(|e| fsx::failure(fsx::write_kind(&e), &e, &to))?;
                } else {
                    if let Some(parent) = to.parent() {
                        fs::create_dir_all(parent).map_err(|e| fsx::failure(fsx::write_kind(&e), &e, parent))?;
                    }
                    total += fsx::copy_file(&from, &to).map_err(|e| match e {
                        CopyError::Read(e) => {
                            let kind = if fsx::is_unavailable(&e) {
                                ErrorKind::TargetUnavailable
                            } else {
                                ErrorKind::ReadFailed
                            };
                            fsx::failure(kind, &e, &from)
                        }
                        CopyError::Write(e) => fsx::failure(fsx::write_kind(&e), &e, &to),
                    })?;
                    copied += 1;
                    hook("copy.file", copied);
                }
            }
        }
        meta.targets.push(RecordedTarget {
            root: target.root.clone(),
            filter: target.filter.clone(),
            excludes: target.excludes.clone(),
            absent,
            folder,
        });
    }
    write_meta(dest, meta)?;
    Ok(total)
}

pub fn write_meta(dir: &Path, meta: &CheckpointMeta) -> Result<(), Failure> {
    let path = dir.join(META_FILE);
    let text = serde_json::to_string_pretty(meta).expect("meta serializes");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| fsx::failure(fsx::write_kind(&e), &e, &path))?;
    use std::io::Write;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| fsx::failure(fsx::write_kind(&e), &e, &path))
}

pub fn read_meta(dir: &Path) -> Option<CheckpointMeta> {
    let text = fs::read_to_string(dir.join(META_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// A readable name for a target's subfolder.
fn target_label(target: &Target) -> String {
    let raw = match &target.filter {
        Filter::Exact(name) => name.clone(),
        _ => target.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "root".into()),
    };
    sanitize(&raw)
}

pub fn sanitize(raw: &str) -> String {
    let cleaned: String =
        raw.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() || trimmed.starts_with(".ss-") { format!("_{trimmed}") } else { trimmed }
}

fn unique_folder(label: &str, used: &mut BTreeSet<String>) -> String {
    let mut candidate = label.to_string();
    let mut n = 2;
    while used.contains(&candidate.to_lowercase()) {
        candidate = format!("{label} ({n})");
        n += 1;
    }
    used.insert(candidate.to_lowercase());
    candidate
}

/// Publishes a finished copy under a free name: never overwrites, even if a
/// folder appears there at the last moment.
pub fn publish(temp: &Path, parent: &Path, name: &str) -> Result<PathBuf, Failure> {
    for n in 1.. {
        let candidate = if n == 1 { parent.join(name) } else { parent.join(format!("{name} ({n})")) };
        match fsx::rename_noreplace(temp, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if fs::symlink_metadata(&candidate).is_ok() => {
                let _ = e;
                continue;
            }
            Err(e) => return Err(fsx::failure(fsx::write_kind(&e), &e, &candidate)),
        }
    }
    unreachable!()
}

/// A change signature of a checkpoint folder: paths, kinds, sizes and
/// modification times of everything inside. Change detection, not a
/// content checksum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub hash: String,
    pub size: u64,
    pub identity: Option<String>,
}

pub fn signature(dir: &Path) -> std::io::Result<Signature> {
    let meta = fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        // A checkpoint replaced by a link is a changed checkpoint.
        return Ok(Signature { hash: "link".into(), size: 0, identity: fsx::identity(dir) });
    }
    let mut lines = Vec::new();
    let mut size = 0u64;
    listing(dir, "", &mut lines, &mut size)?;
    lines.sort();
    let mut hasher = Sha256::new();
    for line in &lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    Ok(Signature { hash: hex::encode(hasher.finalize()), size, identity: fsx::identity(dir) })
}

fn listing(dir: &Path, prefix: &str, lines: &mut Vec<String>, size: &mut u64) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let meta = fs::symlink_metadata(entry.path())?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        if meta.file_type().is_symlink() {
            lines.push(format!("{rel}|link|0|{modified}"));
        } else if meta.is_dir() {
            lines.push(format!("{rel}|dir|0|0"));
            listing(&entry.path(), &rel, lines, size)?;
        } else {
            *size += meta.len();
            lines.push(format!("{rel}|file|{}|{modified}", meta.len()));
        }
    }
    Ok(())
}

/// Measures a folder's total file size (for leftover folders).
pub fn folder_size(dir: &Path) -> u64 {
    let mut lines = Vec::new();
    let mut size = 0;
    let _ = listing(dir, "", &mut lines, &mut size);
    size
}

/// Deletes a folder by renaming it to a reserved disposal name first, so a
/// locked folder is never left half-deleted under its real name. Returns
/// the disposal path if the rename worked but removal didn't finish.
pub fn dispose(dir: &Path, token: &str) -> Result<(), DisposeError> {
    let parent = dir.parent().unwrap_or(Path::new("."));
    let disposal = parent.join(format!("{DISPOSAL_PREFIX}{token}"));
    fsx::rename_noreplace(dir, &disposal)
        .map_err(|e| DisposeError::Rename(fsx::failure(fsx::rename_kind(&e), &e, dir)))?;
    remove_disposal(&disposal).map_err(|f| DisposeError::Remove(disposal.clone(), f))
}

#[derive(Debug)]
pub enum DisposeError {
    /// Nothing was touched.
    Rename(Failure),
    /// Renamed, but not fully removed; leftovers are cleaned up later.
    Remove(PathBuf, Failure),
}

pub fn remove_disposal(path: &Path) -> Result<(), Failure> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(fsx::failure(ErrorKind::DeleteIncomplete, &e, path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use savescummer_core::Presence;

    fn meta() -> CheckpointMeta {
        CheckpointMeta {
            format: 1,
            game_id: "g".into(),
            game_name: "G".into(),
            kind: "saved".into(),
            created_at: "t".into(),
            operation: None,
            targets: vec![],
        }
    }

    fn target(root: &Path, filter: Filter) -> Target {
        Target { root: root.to_path_buf(), filter, excludes: vec![], presence: Presence::Present }
    }

    #[test]
    fn copies_every_target_and_records_absent_ones() {
        let live = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        fs::create_dir_all(live.path().join("saves")).unwrap();
        fs::write(live.path().join("saves/1.sav"), b"one").unwrap();
        fs::write(live.path().join("saves/Player.log"), b"log").unwrap();
        let targets = [
            target(live.path(), Filter::Exact("saves".into())),
            target(&live.path().join("missing"), Filter::All),
            target(live.path(), Filter::Exact("saves".into())),
        ];
        let dest = store.path().join("cp");
        let mut m = meta();
        let size = copy_save_set(&targets, &dest, &mut m, true, &no_hook).unwrap();
        assert_eq!(size, 6, "two targets naming the same folder copy it twice");
        assert_eq!(fs::read(dest.join("saves/saves/1.sav")).unwrap(), b"one");
        assert!(!dest.join("saves/saves/Player.log").exists(), "logs are never copied");
        assert!(m.targets[1].absent);
        assert_eq!(m.targets[2].folder, "saves (2)");
        assert_eq!(read_meta(&dest).unwrap(), m);
    }

    #[test]
    fn signature_sees_edits_and_replacements() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), b"1").unwrap();
        let before = signature(dir.path()).unwrap();
        assert_eq!(signature(dir.path()).unwrap(), before);
        fs::write(dir.path().join("a"), b"22").unwrap();
        assert_ne!(signature(dir.path()).unwrap().hash, before.hash);
    }

    #[test]
    fn publish_never_overwrites() {
        let store = tempfile::tempdir().unwrap();
        fs::create_dir(store.path().join("name")).unwrap();
        fs::create_dir(store.path().join("tmp")).unwrap();
        let published = publish(&store.path().join("tmp"), store.path(), "name").unwrap();
        assert_eq!(published, store.path().join("name (2)"));
    }

    #[test]
    fn dispose_renames_then_removes() {
        let store = tempfile::tempdir().unwrap();
        let cp = store.path().join("cp");
        fs::create_dir_all(cp.join("x")).unwrap();
        fs::write(cp.join("x/f"), b"f").unwrap();
        dispose(&cp, "1").unwrap();
        assert!(!cp.exists());
        assert!(!store.path().join(".ss-del-1").exists());
    }
}
