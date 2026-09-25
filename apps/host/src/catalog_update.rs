//! Catalog updates (PLAN-CATALOG.md 6): a newer bundle is fetched from the
//! repository, verified, and swapped in atomically; the previous one is kept
//! as the last good copy. Any failure keeps the active catalog, silently for
//! the user; the CLI's `catalog` command shows the last problem.
//!
//! Files in `<data dir>/catalog/`:
//!
//! - `catalog.json` — the newest downloaded bundle;
//! - `catalog.previous.json` — the one before it, the last good copy;
//! - `catalog.meta.json` — the URL, ETag and SHA-256 of both.
//!
//! A bundle file is used only when its SHA-256 is one the meta file
//! recorded, so a file cut short by a crash or edited by hand is never read.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use savescummer_catalog::Bundle;
use savescummer_ipc::Phase;

use crate::fetch::{self, Fetched};
use crate::host::{self, Host};

/// Where released catalogs live: the bundle on the main branch, rebuilt by
/// `cargo xtask catalog` whenever the game list or the addendum changes.
pub const CATALOG_URL: &str = "https://raw.githubusercontent.com/neochief/savescummer/main/catalog/catalog.json";

/// Far above today's bundle (~130 KB), far below anything worth reading.
const MAX_BUNDLE: u64 = 16 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
/// How often a running host looks for a newer catalog.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);

const CURRENT: &str = "catalog.json";
const PREVIOUS: &str = "catalog.previous.json";
const META: &str = "catalog.meta.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Meta {
    url: String,
    etag: Option<String>,
    sha256: String,
    previous_sha256: Option<String>,
    fetched_at: String,
}

fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("catalog")
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read_meta(dir: &Path) -> Option<Meta> {
    serde_json::from_slice(&fs::read(dir.join(META)).ok()?).ok()
}

/// A bundle file whose hash the meta file recorded, parsed.
fn verified(path: &Path, meta: &Meta) -> Option<Bundle> {
    let bytes = fs::read(path).ok()?;
    let hash = sha256(&bytes);
    if hash != meta.sha256 && meta.previous_sha256.as_deref() != Some(hash.as_str()) {
        return None;
    }
    Bundle::parse(std::str::from_utf8(&bytes).ok()?).ok()
}

/// The newest usable downloaded bundle: the current one, else the last good
/// one. None sends the host to its built-in catalog.
pub fn load_downloaded(data_dir: &Path) -> Option<Bundle> {
    let dir = dir(data_dir);
    let meta = read_meta(&dir)?;
    verified(&dir.join(CURRENT), &meta).or_else(|| verified(&dir.join(PREVIOUS), &meta))
}

/// Writes `bytes` beside `path` and renames it into place.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Makes `bytes` the current bundle and the current one the last good copy.
fn store(data_dir: &Path, url: &str, bytes: &[u8], etag: Option<String>) -> std::io::Result<()> {
    let dir = dir(data_dir);
    fs::create_dir_all(&dir)?;
    let old = read_meta(&dir);
    let current = dir.join(CURRENT);
    let hash = sha256(bytes);
    // The intact current bundle becomes the last good copy, unless it is
    // the one being stored again.
    let demote = old.as_ref().is_some_and(|m| m.sha256 != hash && verified(&current, m).is_some());
    let previous_sha256 = match &old {
        Some(meta) if demote => Some(meta.sha256.clone()),
        Some(meta) => meta.previous_sha256.clone(),
        None => None,
    };
    // The meta file names both hashes before any file moves, so every
    // moment of the swap leaves a verifiable file.
    let mut meta = Meta { url: url.to_string(), etag: None, sha256: hash, previous_sha256, fetched_at: host::now() };
    write_atomic(&dir.join(META), &serde_json::to_vec_pretty(&meta).unwrap_or_default())?;
    if demote {
        fs::rename(&current, dir.join(PREVIOUS))?;
    }
    write_atomic(&current, bytes)?;
    // The ETag only once the file it describes is in place.
    meta.etag = etag;
    write_atomic(&dir.join(META), &serde_json::to_vec_pretty(&meta).unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `--no-catalog-update`.
    Off,
    /// The newest bundle is the one in use.
    Current,
    /// A newer bundle is in use now; a full scan was requested.
    Updated {
        revision: String,
    },
    Failed(String),
}

fn url(host: &Host) -> String {
    host.opts.catalog_url.clone().unwrap_or_else(|| CATALOG_URL.to_string())
}

/// Looks for a newer catalog once, and switches to it when there is one.
pub fn check(host: &Arc<Host>) -> Outcome {
    if host.opts.no_catalog_update {
        return Outcome::Off;
    }
    let outcome = fetch_and_apply(host);
    let mut catalog = host.catalog.write().unwrap_or_else(|e| e.into_inner());
    catalog.checked_at = Some(host::now());
    catalog.problem = match &outcome {
        Outcome::Failed(problem) => Some(problem.clone()),
        _ => None,
    };
    drop(catalog);
    match &outcome {
        Outcome::Failed(problem) => crate::trace(&format!("catalog update failed: {problem}")),
        Outcome::Updated { revision } => crate::trace(&format!("catalog updated to {revision}")),
        _ => {}
    }
    outcome
}

fn fetch_and_apply(host: &Arc<Host>) -> Outcome {
    let url = url(host);
    let dir = dir(&host.data_dir);
    // Ask for changes only when our copy of that URL is intact; otherwise
    // fetch it whole.
    let etag = read_meta(&dir)
        .filter(|meta| meta.url == url && verified(&dir.join(CURRENT), meta).is_some())
        .and_then(|meta| meta.etag);
    let (bytes, etag) = match fetch::get(&url, etag.as_deref(), MAX_BUNDLE, TIMEOUT) {
        Ok(Fetched::NotModified) => return Outcome::Current,
        Ok(Fetched::Missing) => return Outcome::Failed(format!("{url}: not found")),
        Ok(Fetched::Body { bytes, etag }) => (bytes, etag),
        Err(e) => return Outcome::Failed(e),
    };
    let bundle = match std::str::from_utf8(&bytes)
        .map_err(|e| e.to_string())
        .and_then(|text| Bundle::parse(text).map_err(|e| e.to_string()))
    {
        Ok(bundle) => bundle,
        Err(e) => return Outcome::Failed(format!("the downloaded catalog is invalid: {e}")),
    };
    if let Err(e) = store(&host.data_dir, &url, &bytes, etag) {
        return Outcome::Failed(format!("can't store the downloaded catalog: {e}"));
    }
    let revision = bundle.source.revision.clone();
    let mut catalog = host.catalog.write().unwrap_or_else(|e| e.into_inner());
    if *catalog.bundle == bundle {
        catalog.source = "downloaded".into();
        return Outcome::Current;
    }
    catalog.bundle = Arc::new(bundle);
    catalog.source = "downloaded".into();
    drop(catalog);
    host.scans.request(true, false, "a catalog update");
    Outcome::Updated { revision }
}

/// Checks at start and then daily, until the host shuts down.
pub fn run(host: Arc<Host>) {
    if host.opts.no_catalog_update {
        return;
    }
    loop {
        check(&host);
        let next = Instant::now() + CHECK_EVERY;
        while Instant::now() < next {
            std::thread::sleep(Duration::from_secs(1));
            if host.lock().phase == Phase::ShuttingDown {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(revision: &str) -> Vec<u8> {
        format!(r#"{{"schema":1,"source":{{"repo":"r","revision":"{revision}"}},"games":[]}}"#).into_bytes()
    }

    #[test]
    fn the_last_good_copy_survives_a_damaged_current_one() {
        let data = tempfile::tempdir().unwrap();
        store(data.path(), "u", &bundle("one"), Some("\"a\"".into())).unwrap();
        assert_eq!(load_downloaded(data.path()).unwrap().source.revision, "one");
        store(data.path(), "u", &bundle("two"), None).unwrap();
        assert_eq!(load_downloaded(data.path()).unwrap().source.revision, "two");
        fs::write(data.path().join("catalog").join(CURRENT), b"{\"schema\":1,").unwrap();
        assert_eq!(load_downloaded(data.path()).unwrap().source.revision, "one");
        fs::write(data.path().join("catalog").join(PREVIOUS), bundle("edited by hand")).unwrap();
        assert!(load_downloaded(data.path()).is_none(), "only recorded hashes are read");
    }

    #[test]
    fn storing_the_same_bundle_again_keeps_the_last_good_copy() {
        let data = tempfile::tempdir().unwrap();
        store(data.path(), "u", &bundle("one"), None).unwrap();
        store(data.path(), "u", &bundle("two"), None).unwrap();
        store(data.path(), "u", &bundle("two"), Some("\"b\"".into())).unwrap();
        fs::remove_file(data.path().join("catalog").join(CURRENT)).unwrap();
        assert_eq!(load_downloaded(data.path()).unwrap().source.revision, "one");
    }

    #[test]
    fn a_swap_cut_short_leaves_a_readable_bundle() {
        let data = tempfile::tempdir().unwrap();
        store(data.path(), "u", &bundle("one"), None).unwrap();
        let dir = data.path().join("catalog");
        // As if the host died after renaming the current file away.
        fs::rename(dir.join(CURRENT), dir.join(PREVIOUS)).unwrap();
        assert_eq!(load_downloaded(data.path()).unwrap().source.revision, "one");
    }
}
