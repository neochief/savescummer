use crate::*;
use std::path::{Path, PathBuf};

/// A commit atomically replaces all durable metadata. Implementations must leave
/// the previous state intact on error; no filesystem work occurs in a commit.
pub trait Repository: Send + Sync {
    fn load(&self) -> Result<State>;
    fn commit(&self, state: &State) -> Result<()>;
}
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}
pub trait PathPolicy: Send + Sync {
    fn resolve(&self, path: &Path) -> Result<PathBuf>;
    fn validate(&self, path: &Path, others: &[(Id, PathBuf)]) -> Result<PathBuf>;
    fn same_location(&self, recorded: &Path, current: &Path) -> bool;
}
/// Copies must create their destination exclusively, reject links/reparse points,
/// and retain partial destinations on failure. Rename must never replace a path.
pub trait SnapshotIo: Send + Sync {
    fn accessible_dir(&self, path: &Path) -> Result<bool>;
    fn exists(&self, path: &Path) -> Result<bool>;
    fn identity(&self, path: &Path) -> Result<String>;
    fn modified_ms(&self, path: &Path) -> Result<u64>;
    fn fingerprint(&self, path: &Path) -> Result<String>;
    fn copy(&self, source: &Path, destination: &Path, progress: &mut dyn FnMut(u64)) -> Result<()>;
    fn rename(&self, source: &Path, destination: &Path) -> Result<()>;
    fn saved_candidates(&self, live: &Path) -> Result<Vec<PathBuf>>;
    fn next_saved_path(&self, live: &Path, reserved: &[PathBuf]) -> Result<PathBuf>;
    fn remove(&self, path: &Path) -> Result<()>;
}
