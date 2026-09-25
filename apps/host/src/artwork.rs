//! ARTWORK (PLAN-HOST.md): Steam art for known games, scaled down and kept
//! in the host's cache, so the UI draws the sidebar from small local files
//! and never downloads anything.
//!
//! - Sources, in order: Steam's local library cache (both its current
//!   per-app folders with their hashed subfolders and the older flat
//!   layout), then Steam's public CDN. The small icon only exists locally:
//!   its CDN name is a hash only Steam's app info knows.
//! - The cache holds one folder per Steam app id. A file there is used as
//!   long as its image header reads; it is only ever written through a
//!   temporary file, after the image decoded.
//! - Everything runs on one background thread. Requests at host start,
//!   after every scan and when a UI attaches coalesce into one pass.
//! - Failures are quiet: a network error waits before the next try, and art
//!   the CDN doesn't have is asked for again only after a week.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use image::{DynamicImage, ImageFormat, ImageReader};

use savescummer_ipc::{Artwork, Phase};

use crate::fetch::{self, Fetched};
use crate::host::Host;

/// Steam's public image server; no login or key.
pub const CDN: &str = "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps";

/// Steam's images are a few hundred KB; anything far larger isn't one.
const MAX_IMAGE: u64 = 16 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
/// After a network failure, the first wait before trying that image again.
const RETRY_FIRST: Duration = Duration::from_secs(5 * 60);
const RETRY_MAX: Duration = Duration::from_secs(6 * 3600);
/// Art the CDN answered "not found" for is asked for again after this.
const MISSING_FOR: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Hero,
    Logo,
    Header,
    Icon,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Hero, Kind::Logo, Kind::Header, Kind::Icon];

    fn cached_name(self) -> &'static str {
        match self {
            Kind::Hero => "hero.jpg",
            Kind::Logo => "logo.png",
            Kind::Header => "header.jpg",
            Kind::Icon => "icon.png",
        }
    }

    fn format(self) -> ImageFormat {
        match self {
            Kind::Hero | Kind::Header => ImageFormat::Jpeg,
            // Logos and icons keep their transparency.
            Kind::Logo | Kind::Icon => ImageFormat::Png,
        }
    }

    /// The largest size the UI draws it at, with room for high-DPI screens.
    fn max_size(self) -> (u32, u32) {
        match self {
            // A sidebar card is a thin strip of the hero art.
            Kind::Hero => (1280, 414),
            Kind::Logo => (640, 360),
            Kind::Header => (460, 215),
            Kind::Icon => (64, 64),
        }
    }

    /// File names in Steam's per-app cache folder and its subfolders.
    fn steam_names(self) -> &'static [&'static str] {
        match self {
            Kind::Hero => &["library_hero.jpg"],
            Kind::Logo => &["logo.png"],
            Kind::Header => &["header.jpg", "library_header.jpg"],
            Kind::Icon => &[],
        }
    }

    /// The older flat layout: `<appid>_<suffix>`.
    fn flat_suffix(self) -> &'static str {
        match self {
            Kind::Hero => "library_hero.jpg",
            Kind::Logo => "logo.png",
            Kind::Header => "header.jpg",
            Kind::Icon => "icon.jpg",
        }
    }

    fn cdn_name(self) -> Option<&'static str> {
        match self {
            Kind::Hero => Some("library_hero.jpg"),
            Kind::Logo => Some("logo.png"),
            Kind::Header => Some("header.jpg"),
            Kind::Icon => None,
        }
    }
}

/// Where the host keeps its art: beside a data folder given on the command
/// line (tests, development), else the platform's cache folder.
pub fn cache_dir(host: &Host) -> PathBuf {
    match &host.opts.data_dir {
        Some(_) => host.data_dir.join("cache").join("artwork"),
        None => savescummer_platform::cache_dir().join("artwork"),
    }
}

/// The Steam app id a game's art comes from: its catalog entry's.
pub fn steam_app(host: &Host, catalog_id: Option<&str>) -> Option<u64> {
    let catalog_id = catalog_id?;
    let catalog = host.catalog.read().unwrap_or_else(|e| e.into_inner());
    catalog.bundle.games.iter().find(|g| g.id == catalog_id).and_then(|g| g.detect.steam.first().copied())
}

/// Asks for a pass over every known game's art. Cheap; passes coalesce.
pub fn request(host: &Host) {
    if let Some(tx) = host.artwork_tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(());
    }
}

/// The art already in the cache, for the first published state.
pub fn load_cached(host: &Arc<Host>) {
    let dir = cache_dir(host);
    let apps = apps(host);
    let mut found = HashMap::new();
    for app in apps {
        let art = read_set(&dir.join(app.to_string()));
        if art != Artwork::default() {
            found.insert(app, art);
        }
    }
    let mut inner = host.lock();
    inner.artwork = found;
    host.publish(&mut inner);
}

/// Every Steam app id the library's known games have.
fn apps(host: &Arc<Host>) -> BTreeSet<u64> {
    let ids: Vec<Option<String>> = host.lock().games.values().map(|g| g.catalog_id.clone()).collect();
    ids.iter().filter_map(|id| steam_app(host, id.as_deref())).collect()
}

fn usable(path: &Path) -> bool {
    ImageReader::open(path).and_then(|r| r.with_guessed_format()).is_ok_and(|r| r.into_dimensions().is_ok())
}

fn read_set(dir: &Path) -> Artwork {
    let get = |kind: Kind| {
        let path = dir.join(kind.cached_name());
        usable(&path).then(|| path.to_string_lossy().into_owned())
    };
    Artwork { hero: get(Kind::Hero), logo: get(Kind::Logo), header: get(Kind::Header), icon: get(Kind::Icon) }
}

/// Steam's own copy of an app's image, if its library cache has one.
fn steam_local(steam_root: &Path, app: u64, kind: Kind) -> Option<PathBuf> {
    let cache = steam_root.join("appcache").join("librarycache");
    let folder = cache.join(app.to_string());
    let mut candidates = Vec::new();
    for name in kind.steam_names() {
        candidates.push(folder.join(name));
    }
    if let Ok(entries) = fs::read_dir(&folder) {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                // Hashed subfolders hold one (often localized) image each.
                for name in kind.steam_names() {
                    candidates.push(path.join(name));
                }
            } else if kind == Kind::Icon && is_icon_name(&entry.file_name().to_string_lossy()) {
                candidates.push(path);
            }
        }
    }
    candidates.push(cache.join(format!("{app}_{}", kind.flat_suffix())));
    candidates.into_iter().find(|p| p.is_file())
}

/// The icon is the one image named by its 40-digit hash.
fn is_icon_name(name: &str) -> bool {
    name.strip_suffix(".jpg").is_some_and(|stem| stem.len() == 40 && stem.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Scales `image` down to fit `kind` and publishes it as `target`.
fn publish(image: DynamicImage, kind: Kind, target: &Path) -> Result<(), String> {
    let (max_w, max_h) = kind.max_size();
    let image = if image.width() > max_w || image.height() > max_h {
        image.resize(max_w, max_h, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let image = match kind.format() {
        ImageFormat::Jpeg => DynamicImage::ImageRgb8(image.to_rgb8()),
        _ => DynamicImage::ImageRgba8(image.to_rgba8()),
    };
    let dir = target.parent().ok_or("no folder")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = target.with_extension("tmp");
    image.save_with_format(&tmp, kind.format()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|e| e.to_string())
}

/// Network failures, remembered so a pass after every scan doesn't hammer a
/// server that is down.
#[derive(Default)]
struct Backoff {
    next: HashMap<(u64, Kind), (Instant, Duration)>,
}

impl Backoff {
    fn waiting(&self, key: (u64, Kind)) -> bool {
        self.next.get(&key).is_some_and(|(at, _)| Instant::now() < *at)
    }

    fn failed(&mut self, key: (u64, Kind)) {
        let wait = self.next.get(&key).map(|(_, w)| (*w * 2).min(RETRY_MAX)).unwrap_or(RETRY_FIRST);
        self.next.insert(key, (Instant::now() + wait, wait));
    }

    fn succeeded(&mut self, key: (u64, Kind)) {
        self.next.remove(&key);
    }
}

/// A marker that the CDN had no such image, recently.
fn missing_marker(dir: &Path, kind: Kind) -> PathBuf {
    dir.join(format!("{}.missing", kind.cached_name()))
}

fn recently_missing(dir: &Path, kind: Kind) -> bool {
    fs::metadata(missing_marker(dir, kind))
        .and_then(|m| m.modified())
        .is_ok_and(|at| SystemTime::now().duration_since(at).unwrap_or_default() < MISSING_FOR)
}

/// Makes sure one image is in the cache. True when it is.
fn ensure(host: &Host, dir: &Path, app: u64, kind: Kind, backoff: &mut Backoff) -> bool {
    let target = dir.join(kind.cached_name());
    if usable(&target) {
        return true;
    }
    if let Some(root) = &host.env.folders.steam_root
        && let Some(source) = steam_local(root, app, kind)
        && let Ok(bytes) = fs::read(&source)
        && let Ok(image) = decode(&bytes)
        && publish(image, kind, &target).is_ok()
    {
        return true;
    }
    let Some(name) = kind.cdn_name() else { return false };
    if backoff.waiting((app, kind)) || recently_missing(dir, kind) {
        return false;
    }
    let base = host.opts.artwork_url.as_deref().unwrap_or(CDN).trim_end_matches('/');
    let url = format!("{base}/{app}/{name}");
    match fetch::get(&url, None, MAX_IMAGE, TIMEOUT) {
        Ok(Fetched::Body { bytes, .. }) => match decode(&bytes).and_then(|image| publish(image, kind, &target)) {
            Ok(()) => {
                backoff.succeeded((app, kind));
                let _ = fs::remove_file(missing_marker(dir, kind));
                true
            }
            // Not an image (a captive portal's page, a cut-off download).
            Err(_) => {
                backoff.failed((app, kind));
                false
            }
        },
        Ok(Fetched::Missing) => {
            let _ = fs::create_dir_all(dir);
            let _ = fs::write(missing_marker(dir, kind), b"");
            false
        }
        Ok(Fetched::NotModified) | Err(_) => {
            backoff.failed((app, kind));
            false
        }
    }
}

/// One pass: every known game's art, publishing as each game's set changes.
fn pass(host: &Arc<Host>, backoff: &mut Backoff) {
    let root = cache_dir(host);
    for app in apps(host) {
        if host.lock().phase == Phase::ShuttingDown {
            return;
        }
        let dir = root.join(app.to_string());
        for kind in Kind::ALL {
            ensure(host, &dir, app, kind, backoff);
        }
        let art = read_set(&dir);
        let mut inner = host.lock();
        let known = inner.artwork.get(&app).cloned().unwrap_or_default();
        if art != known {
            if art == Artwork::default() {
                inner.artwork.remove(&app);
            } else {
                inner.artwork.insert(app, art);
            }
            host.publish(&mut inner);
        }
    }
}

/// The artwork thread: a pass per request, until the host shuts down.
pub fn run(host: Arc<Host>) {
    let (tx, rx) = mpsc::channel();
    *host.artwork_tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let mut backoff = Backoff::default();
    load_cached(&host);
    // Host start is one of the moments art is checked.
    pass(&host, &mut backoff);
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(()) => {
                // Coalesce a burst of requests into one pass.
                while rx.try_recv().is_ok() {}
                pass(&host, &mut backoff);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if host.lock().phase == Phase::ShuttingDown {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_the_hash_named_images() {
        assert!(is_icon_name("33ea124ea8c03a9ce7012d34c3b348a351612fca.jpg"));
        assert!(!is_icon_name("header.jpg"));
        assert!(!is_icon_name("33ea124ea8c03a9ce7012d34c3b348a351612fca.png"));
    }

    fn png(path: &Path, w: u32, h: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        DynamicImage::new_rgba8(w, h).save_with_format(path, ImageFormat::Png).unwrap();
    }

    #[test]
    fn steam_local_finds_every_layout() {
        let steam = tempfile::tempdir().unwrap();
        let cache = steam.path().join("appcache").join("librarycache");
        png(&cache.join("10").join("logo.png"), 4, 4);
        png(&cache.join("10").join("0123456789abcdef0123456789abcdef01234567.jpg"), 4, 4);
        png(&cache.join("20").join("98878a81ca9047352403db7e19e3942239ea8bf1").join("library_hero.jpg"), 4, 4);
        png(&cache.join("20").join("7f6e572bde792b139018cae5a361d91d5e1ad812").join("library_header.jpg"), 4, 4);
        png(&cache.join("30_library_hero.jpg"), 4, 4);
        let found = |app, kind| steam_local(steam.path(), app, kind).is_some();
        assert!(found(10, Kind::Logo) && found(10, Kind::Icon) && !found(10, Kind::Hero));
        assert!(found(20, Kind::Hero) && found(20, Kind::Header) && !found(20, Kind::Icon));
        assert!(found(30, Kind::Hero));
    }

    #[test]
    fn published_art_is_scaled_down_and_keeps_its_shape() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hero.jpg");
        publish(DynamicImage::new_rgb8(3840, 1240), Kind::Hero, &target).unwrap();
        let (w, h) = ImageReader::open(&target).unwrap().into_dimensions().unwrap();
        assert_eq!((w, h), (1280, 413));
        let small = dir.path().join("icon.png");
        publish(DynamicImage::new_rgba8(32, 32), Kind::Icon, &small).unwrap();
        assert_eq!(ImageReader::open(&small).unwrap().into_dimensions().unwrap(), (32, 32), "never scaled up");
        assert!(!dir.path().join("hero.tmp").exists());
    }

    #[test]
    fn backoff_doubles_up_to_a_cap() {
        let mut backoff = Backoff::default();
        let key = (1, Kind::Hero);
        assert!(!backoff.waiting(key));
        backoff.failed(key);
        assert!(backoff.waiting(key));
        for _ in 0..20 {
            backoff.failed(key);
        }
        assert_eq!(backoff.next[&key].1, RETRY_MAX);
        backoff.succeeded(key);
        assert!(!backoff.waiting(key));
    }
}
