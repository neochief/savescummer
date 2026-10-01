//! Skipping dev build steps whose inputs haven't changed.
//!
//! A step's KEY hashes everything it reads: source files by content, and the
//! files earlier steps built by size and modification time (they're big, and
//! only our own builds write them). When the key and the step's outputs are
//! the same as after its last successful run, the step is skipped.
//!
//! Inputs are taken broadly so nothing is missed: Cargo's own dep-info for
//! Rust (every file a binary was built from, `include_str!` and build-script
//! inputs included), and every git-tracked or new file under a source folder
//! otherwise. Release builds and test runs never use the cache.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::paths::{self, Mode};

pub struct Key {
    hasher: Sha256,
    /// Each file hashed by content: its hash and modification time.
    contents: BTreeMap<PathBuf, (String, SystemTime)>,
}

impl Key {
    /// Starts a key for `step`. It includes xtask itself, since its code
    /// decides what each step does.
    pub fn new(step: &str) -> anyhow::Result<Self> {
        let mut key = Key { hasher: Sha256::new(), contents: BTreeMap::new() };
        key.text(step);
        key.stamp(&std::env::current_exe()?);
        Ok(key)
    }

    pub fn text(&mut self, text: &str) -> &mut Self {
        self.hasher.update(text.len().to_le_bytes());
        self.hasher.update(text.as_bytes());
        self
    }

    /// A file's content, every file's in a folder, or that it's missing.
    pub fn file(&mut self, path: &Path) -> anyhow::Result<&mut Self> {
        self.text(&path.to_string_lossy());
        if path.is_dir() {
            // Build scripts can watch a folder, which means every file in it.
            let mut inside = crate::package::files(path)?;
            inside.sort();
            for file in inside {
                self.file(&file)?;
            }
            return Ok(self);
        }
        match fs::read(path) {
            Ok(bytes) => {
                let hash = Sha256::digest(&bytes);
                self.hasher.update(hash);
                let modified = fs::metadata(path).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
                self.contents.insert(path.to_path_buf(), (hex::encode(hash), modified));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.text("missing");
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        }
        Ok(self)
    }

    /// Every git-tracked or new (not ignored) file under these folders, by
    /// content. Paths are relative to the repository root.
    pub fn sources(&mut self, folders: &[&str]) -> anyhow::Result<&mut Self> {
        let root = paths::root();
        let output = Command::new("git")
            .current_dir(&root)
            .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"])
            .args(folders)
            .output()
            .context("listing source files with git")?;
        anyhow::ensure!(output.status.success(), "git ls-files failed: {}", String::from_utf8_lossy(&output.stderr));
        let mut files: Vec<&str> =
            std::str::from_utf8(&output.stdout)?.split('\0').filter(|name| !name.is_empty()).collect();
        files.sort_unstable();
        files.dedup();
        for name in files {
            self.file(&root.join(name))?;
        }
        Ok(self)
    }

    /// The Cargo workspace setup every Rust step depends on: the lock file,
    /// all manifests (features live there), Cargo's config, the toolchain pin,
    /// `RUSTFLAGS`, and our `SAVESCUMMER_*` variables (the code reads them
    /// with `option_env!`).
    pub fn cargo_setup(&mut self) -> anyhow::Result<&mut Self> {
        let root = paths::root();
        for file in workspace_manifests()? {
            self.file(&file)?;
        }
        self.file(&root.join("Cargo.lock"))?;
        self.file(&root.join(".cargo").join("config.toml"))?;
        self.file(&root.join("rust-toolchain.toml"))?;
        let mut vars: Vec<(String, String)> =
            std::env::vars().filter(|(name, _)| name == "RUSTFLAGS" || name.starts_with("SAVESCUMMER_")).collect();
        vars.sort();
        for (name, value) in vars {
            self.text(&format!("{name}={value}"));
        }
        Ok(self)
    }

    /// Every source file Cargo recorded in a binary's dep-info (`<bin>.d`),
    /// by content, except files under `skip` (outputs of earlier steps that
    /// are covered by their own inputs) and Cargo's own output folder.
    /// Without the dep-info (never built) the key can't match, so the step runs.
    pub fn dep_info(&mut self, dep_info: &Path, skip: &[PathBuf]) -> anyhow::Result<&mut Self> {
        let Ok(text) = fs::read_to_string(dep_info) else {
            self.text("no dep-info");
            return Ok(self);
        };
        let target = normalize(&paths::target());
        let mut files: Vec<PathBuf> = dep_info_files(&text)
            .into_iter()
            .map(|file| normalize(&file))
            .filter(|file| !file.starts_with(&target) && !skip.iter().any(|dir| file.starts_with(dir)))
            .collect();
        files.sort();
        files.dedup();
        for file in files {
            self.file(&file)?;
        }
        Ok(self)
    }

    /// A built file by size and modification time.
    pub fn stamp(&mut self, path: &Path) -> &mut Self {
        self.text(&path.to_string_lossy());
        self.text(&stamp(path));
        self
    }

    fn finish(&self) -> String {
        hex::encode(self.hasher.clone().finalize())
    }
}

/// A cached step: run it with [`Step::run`], which skips it when nothing changed.
pub struct Step<'a> {
    name: &'a str,
    mode: Mode,
    enabled: bool,
    outputs: Vec<PathBuf>,
}

impl<'a> Step<'a> {
    /// `enabled` is false for release builds and test runs, which always run
    /// every step. `outputs` are files the step makes; they must still be
    /// there, untouched, to skip it.
    pub fn new(name: &'a str, mode: Mode, enabled: bool, outputs: Vec<PathBuf>) -> Self {
        Step { name, mode, enabled: enabled && mode == Mode::Dev, outputs }
    }

    /// Runs `work` unless `key` (built by `inputs`) and the outputs match the
    /// last successful run. `inputs` is called again after `work`, since a
    /// build can change what its inputs are (Cargo's dep-info).
    pub fn run(
        &self,
        inputs: impl Fn() -> anyhow::Result<Key>,
        work: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if !self.enabled {
            return work();
        }
        let record = self.mode.dir().join("cache").join(format!("{}.json", self.name));
        let saved: Option<serde_json::Value> = fs::read(&record).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let key = inputs()?;
        if let Some(saved) = &saved
            && saved["key"].as_str() == Some(key.finish().as_str())
            && saved["outputs"] == self.output_stamps()
        {
            println!("{}: unchanged, skipped", self.name);
            return Ok(());
        }
        let _ = fs::remove_file(&record);
        if let Some(saved) = &saved {
            refresh_changed(&key, saved)?;
        }
        let started = SystemTime::now();
        work()?;
        let key = inputs()?;
        // A source edited while the step ran may or may not be in its output:
        // don't record, so the next run checks again.
        if key.contents.values().any(|(_, modified)| *modified > started) {
            return Ok(());
        }
        let files: serde_json::Map<String, serde_json::Value> =
            key.contents.iter().map(|(path, (hash, _))| (path.to_string_lossy().into_owned(), json!(hash))).collect();
        let saved = json!({
            "key": key.finish(),
            "outputs": self.output_stamps(),
            "finished_ns": nanos(SystemTime::now()),
            "files": files,
        });
        fs::create_dir_all(record.parent().expect("record is in a folder"))?;
        fs::write(&record, serde_json::to_vec_pretty(&saved)?).with_context(|| format!("writing {}", record.display()))
    }

    /// Each output file's stamp; a folder stands for every file in it.
    fn output_stamps(&self) -> serde_json::Value {
        let mut files = Vec::new();
        for path in &self.outputs {
            if path.is_dir() {
                let mut inside = crate::package::files(path).unwrap_or_default();
                inside.sort();
                files.extend(inside);
            } else {
                files.push(path.clone());
            }
        }
        files.iter().map(|path| json!([paths::show(path), stamp(path)])).collect()
    }
}

/// Cargo (and other build tools) only notice a changed file whose
/// modification time is newer than their last build. A file restored from a
/// backup or another copy can change content and keep an old time, and the
/// tool would then keep its old output. Each input whose content differs from
/// the last run, but whose time is no newer than that run, gets the current
/// time first.
fn refresh_changed(key: &Key, saved: &serde_json::Value) -> anyhow::Result<()> {
    let Some(finished) = saved["finished_ns"].as_u64() else { return Ok(()) };
    let finished = UNIX_EPOCH + std::time::Duration::from_nanos(finished);
    for (path, (hash, modified)) in &key.contents {
        if saved["files"][&*path.to_string_lossy()].as_str() != Some(hash.as_str()) && *modified <= finished {
            println!("{} changed but has an older time; marking it as modified now", paths::show(path));
            fs::File::options()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(SystemTime::now()))
                .with_context(|| format!("updating the time of {}", path.display()))?;
        }
    }
    Ok(())
}

fn nanos(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
}

/// Size and modification time, or "missing".
fn stamp(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(meta) => {
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).unwrap_or_default();
            format!("{}:{}", meta.len(), modified.as_nanos())
        }
        Err(_) => "missing".into(),
    }
}

/// The workspace's `Cargo.toml` and every member's.
pub fn workspace_manifests() -> anyhow::Result<Vec<PathBuf>> {
    let root = paths::root();
    let workspace = root.join("Cargo.toml");
    let text = fs::read_to_string(&workspace).with_context(|| format!("reading {}", workspace.display()))?;
    let table: toml::Table = toml::from_str(&text).with_context(|| format!("parsing {}", workspace.display()))?;
    let members = table["workspace"]["members"].as_array().context("the workspace lists no members")?;
    let mut files = vec![workspace];
    for member in members {
        files.push(root.join(member.as_str().context("a workspace member isn't a path")?).join("Cargo.toml"));
    }
    Ok(files)
}

/// The prerequisites in a Makefile-style dep-info file: `target: a b c`, with
/// spaces in paths escaped as `\ `.
fn dep_info_files(text: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for line in text.lines() {
        // "C:\…\x.exe: …": the first ": " ends the target (a drive's colon has no space after it).
        let Some((_, deps)) = line.split_once(": ") else { continue };
        let mut current = String::new();
        let mut chars = deps.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&' ') => {
                    current.push(' ');
                    chars.next();
                }
                ' ' => {
                    if !current.is_empty() {
                        files.push(PathBuf::from(std::mem::take(&mut current)));
                    }
                }
                _ => current.push(c),
            }
        }
        if !current.is_empty() {
            files.push(PathBuf::from(current));
        }
    }
    files
}

/// Resolves `.` and `..` without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_dep_info() {
        let text = "/t/x: /a/src/lib.rs /a/src/../../my\\ file.json\n\n/a/src/lib.rs:\n";
        let files: Vec<PathBuf> = dep_info_files(text).iter().map(|f| normalize(f)).collect();
        assert_eq!(files, [PathBuf::from("/a/src/lib.rs"), PathBuf::from("/my file.json")]);
    }
}
