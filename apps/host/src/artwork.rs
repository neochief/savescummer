//! Disposable Steam artwork, isolated from the scanner and operation executor.
use anyhow::{Context, Result, bail};
use savescummer_core::{GameArtwork, Id, State};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const RETRY_MIN: Duration = Duration::from_secs(30);
const RETRY_MAX: Duration = Duration::from_secs(15 * 60);

pub(crate) trait Downloader: Send + 'static {
    fn icon(&mut self, app: u32) -> Result<Vec<u8>>;
}

#[derive(Default)]
pub(crate) struct SteamDownloader {
    client: Option<reqwest::blocking::Client>,
}
impl SteamDownloader {
    fn get(&mut self, url: &str) -> Result<Vec<u8>> {
        if self.client.is_none() {
            self.client = Some(
                reqwest::blocking::Client::builder()
                    .user_agent(concat!("SaveScummer/", env!("CARGO_PKG_VERSION")))
                    .https_only(true)
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(15))
                    .build()?,
            );
        }
        let response = self
            .client
            .as_ref()
            .unwrap()
            .get(url)
            .send()?
            .error_for_status()?;
        let mut bytes = Vec::new();
        response.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            bail!("Steam artwork response is too large");
        }
        Ok(bytes)
    }
}
impl Downloader for SteamDownloader {
    fn icon(&mut self, app: u32) -> Result<Vec<u8>> {
        // Steam's public community page supplies the community icon hash without
        // requiring an account/API key. Do not follow arbitrary URLs from HTML.
        let html = String::from_utf8(self.get(&format!("https://steamcommunity.com/app/{app}/"))?)?;
        let hash = icon_hash(&html, app).context("Steam community icon is unavailable")?;
        self.get(&format!(
            "https://shared.fastly.steamstatic.com/community_assets/images/apps/{app}/{hash}.jpg"
        ))
    }
}
fn icon_hash(html: &str, app: u32) -> Option<String> {
    for prefix in [
        "community_assets/images/apps",
        "steamcommunity/public/images/apps",
    ] {
        let marker = format!("/{prefix}/{app}/");
        for tail in html.split(&marker).skip(1) {
            if let Some(hash) = tail.get(..40)
                && hash.bytes().all(|b| b.is_ascii_hexdigit())
                && tail.get(40..44) == Some(".jpg")
            {
                return Some(hash.to_owned());
            }
        }
    }
    None
}

fn decode(bytes: &[u8]) -> Result<image::DynamicImage> {
    if bytes.len() as u64 > MAX_BYTES {
        bail!("icon is too large");
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}
fn valid_cache(path: &Path) -> bool {
    (|| -> Result<()> {
        let file = fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        decode(&bytes)?;
        Ok(())
    })()
    .is_ok()
}
fn cache_icon(root: &Path, app: u32, bytes: &[u8]) -> Result<PathBuf> {
    // Normalize and bound the cached image so UI display is a small local read.
    let image = decode(bytes)?.thumbnail(128, 128);
    let path = icon_path(root, app);
    let directory = path.parent().unwrap();
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    image.write_to(temporary.as_file_mut(), image::ImageFormat::Png)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path).map_err(|e| e.error)?;
    Ok(path)
}
fn icon_path(root: &Path, app: u32) -> PathBuf {
    root.join("artwork/steam")
        .join(app.to_string())
        .join("icon.png")
}

#[derive(Default)]
struct Shared {
    targets: BTreeMap<Id, u32>,
    icons: BTreeMap<u32, PathBuf>,
    pending: BTreeSet<u32>,
    in_flight: Option<u32>,
    revision: u64,
    stopping: bool,
}
type Queue = Arc<(Mutex<Shared>, Condvar)>;
pub(crate) struct Artwork {
    queue: Queue,
}
impl Artwork {
    pub fn start(root: PathBuf, downloader: impl Downloader) -> std::io::Result<Self> {
        let queue = Queue::default();
        let worker = queue.clone();
        std::thread::Builder::new()
            .name("steam-artwork".into())
            .spawn(move || work(worker, root, downloader))?;
        Ok(Self { queue })
    }
    pub fn set_games(&self, targets: BTreeMap<Id, u32>) {
        let (lock, wake) = &*self.queue;
        let mut state = lock.lock().unwrap();
        if state.targets != targets {
            state.targets = targets;
            state.revision += 1;
        }
        Self::enqueue(&mut state);
        wake.notify_one();
    }
    fn enqueue(state: &mut Shared) {
        state.pending.extend(
            state
                .targets
                .values()
                .copied()
                .filter(|id| Some(*id) != state.in_flight),
        );
    }
    pub fn check(&self) {
        let (lock, wake) = &*self.queue;
        Self::enqueue(&mut lock.lock().unwrap());
        wake.notify_one();
    }
    pub fn project(&self, state: &mut State) {
        let shared = self.queue.0.lock().unwrap();
        state.artwork_revision = shared.revision;
        state.artwork = shared
            .targets
            .iter()
            .filter(|(id, _)| state.games.contains_key(*id))
            .map(|(id, app)| {
                (
                    id.clone(),
                    GameArtwork {
                        steam_app_id: *app,
                        icon_path: shared.icons.get(app).cloned(),
                    },
                )
            })
            .collect();
    }
    pub fn revision(&self) -> u64 {
        self.queue.0.lock().unwrap().revision
    }
}
impl Drop for Artwork {
    fn drop(&mut self) {
        self.queue.0.lock().unwrap().stopping = true;
        self.queue.1.notify_one();
    }
}
fn publish(queue: &Queue, app: u32, path: Option<PathBuf>) {
    let mut state = queue.0.lock().unwrap();
    if state.icons.get(&app) != path.as_ref() {
        if let Some(path) = path {
            state.icons.insert(app, path);
        } else {
            state.icons.remove(&app);
        }
        state.revision += 1;
    }
}
fn work(queue: Queue, root: PathBuf, mut downloader: impl Downloader) {
    let mut retries = BTreeMap::<u32, (Instant, Duration)>::new();
    loop {
        let pending = {
            let (lock, wake) = &*queue;
            let mut state = lock.lock().unwrap();
            loop {
                if state.stopping {
                    return;
                }
                let active: BTreeSet<_> = state.targets.values().copied().collect();
                retries.retain(|id, _| active.contains(id));
                state.pending.retain(|id| active.contains(id));
                state.pending.extend(
                    retries
                        .iter()
                        .filter(|(_, (due, _))| *due <= Instant::now())
                        .map(|(id, _)| *id),
                );
                if !state.pending.is_empty() {
                    break;
                }
                state = wake.wait_timeout(state, Duration::from_secs(1)).unwrap().0;
            }
            std::mem::take(&mut state.pending)
        };
        // Check the whole batch before networking so one offline request cannot
        // delay publication of other games' already cached icons.
        let mut downloads = vec![];
        for app in pending {
            let path = icon_path(&root, app);
            if valid_cache(&path) {
                publish(&queue, app, Some(path));
                retries.remove(&app);
            } else {
                publish(&queue, app, None);
                if retries
                    .get(&app)
                    .is_none_or(|(due, _)| *due <= Instant::now())
                {
                    downloads.push(app);
                }
            }
        }
        for app in downloads {
            {
                let mut state = queue.0.lock().unwrap();
                if state.stopping {
                    return;
                }
                if !state.targets.values().any(|id| *id == app) {
                    continue;
                }
                state.in_flight = Some(app);
                state.pending.remove(&app);
            }
            match downloader
                .icon(app)
                .and_then(|bytes| cache_icon(&root, app, &bytes))
            {
                Ok(path) => {
                    publish(&queue, app, Some(path));
                    retries.remove(&app);
                }
                Err(error) => {
                    eprintln!("Steam icon {app}: {error}");
                    let delay = retries
                        .get(&app)
                        .map_or(RETRY_MIN, |(_, delay)| (*delay * 2).min(RETRY_MAX));
                    retries.insert(app, (Instant::now() + delay, delay));
                }
            }
            queue.0.lock().unwrap().in_flight = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    pub(super) fn png() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(32, 32)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    struct Fake {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }
    impl Downloader for Fake {
        fn icon(&mut self, _: u32) -> Result<Vec<u8>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                bail!("offline");
            }
            Ok(png())
        }
    }
    fn targets() -> BTreeMap<Id, u32> {
        BTreeMap::from([("game".into(), 42)])
    }
    fn wait_for(mut test: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(3);
        while !test() {
            assert!(Instant::now() < end, "artwork worker did not complete");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn ready(worker: &Artwork) -> bool {
        worker.queue.0.lock().unwrap().icons.contains_key(&42)
    }

    #[test]
    fn downloads_repairs_and_reuses_cache_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker = Artwork::start(
            temp.path().into(),
            Fake {
                calls: calls.clone(),
                fail: false,
            },
        )
        .unwrap();
        worker.set_games(targets());
        wait_for(|| ready(&worker));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let path = icon_path(temp.path(), 42);
        assert!(valid_cache(&path));
        for bad in [Some(b"broken".as_slice()), Some(b"".as_slice()), None] {
            let previous = calls.load(Ordering::SeqCst);
            if let Some(bytes) = bad {
                fs::write(&path, bytes).unwrap();
            } else {
                fs::remove_file(&path).unwrap();
            }
            worker.check();
            wait_for(|| {
                calls.load(Ordering::SeqCst) > previous && valid_cache(&path) && ready(&worker)
            });
        }
        drop(worker);
        let previous = calls.load(Ordering::SeqCst);
        let restarted = Artwork::start(
            temp.path().into(),
            Fake {
                calls: calls.clone(),
                fail: true,
            },
        )
        .unwrap();
        restarted.set_games(targets());
        wait_for(|| ready(&restarted));
        assert_eq!(calls.load(Ordering::SeqCst), previous);
    }

    #[test]
    fn offline_failure_is_throttled_and_partial_files_are_not_icons() {
        let temp = tempfile::tempdir().unwrap();
        let path = icon_path(temp.path(), 42);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path.with_extension("part"), png()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker = Artwork::start(
            temp.path().into(),
            Fake {
                calls: calls.clone(),
                fail: true,
            },
        )
        .unwrap();
        worker.set_games(targets());
        wait_for(|| calls.load(Ordering::SeqCst) == 1);
        for _ in 0..50 {
            worker.check();
        }
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!ready(&worker));
        assert!(!path.exists());
    }

    #[test]
    fn slow_download_does_not_block_projection_or_duplicate_requests() {
        struct Slow {
            calls: Arc<AtomicUsize>,
            release: mpsc::Receiver<()>,
        }
        impl Downloader for Slow {
            fn icon(&mut self, _: u32) -> Result<Vec<u8>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.release.recv_timeout(Duration::from_secs(3))?;
                Ok(png())
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let worker = Artwork::start(
            temp.path().into(),
            Slow {
                calls: calls.clone(),
                release: rx,
            },
        )
        .unwrap();
        worker.set_games(targets());
        wait_for(|| calls.load(Ordering::SeqCst) == 1);
        for _ in 0..20 {
            worker.set_games(targets());
            worker.check();
        }
        let mut state = State::default();
        worker.project(&mut state);
        assert_eq!(state.revision, 0);
        tx.send(()).unwrap();
        wait_for(|| ready(&worker));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shared_app_ids_download_once_and_new_scan_games_are_checked() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker = Artwork::start(
            temp.path().into(),
            Fake {
                calls: calls.clone(),
                fail: false,
            },
        )
        .unwrap();
        let mut games = targets();
        games.insert("other-installation".into(), 42);
        worker.set_games(games.clone());
        wait_for(|| ready(&worker));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        games.insert("new-game".into(), 43);
        worker.set_games(games);
        wait_for(|| worker.queue.0.lock().unwrap().icons.contains_key(&43));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn invalid_download_cannot_replace_a_good_cache_entry() {
        let temp = tempfile::tempdir().unwrap();
        let path = cache_icon(temp.path(), 42, &png()).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(cache_icon(temp.path(), 42, b"<html>error</html>").is_err());
        assert_eq!(before, fs::read(path).unwrap());
    }

    #[test]
    fn resolves_only_matching_app_and_valid_icon_hash() {
        let hash = "9a3073278e87e3781b69cbf27526d29f0f2a1ff5";
        let html = format!(
            "<img src=\"https://shared.fastly.steamstatic.com/community_assets/images/apps/2853590/{hash}.jpg\">"
        );
        assert_eq!(icon_hash(&html, 2853590).as_deref(), Some(hash));
        assert!(icon_hash(&html, 42).is_none());
        assert!(icon_hash(&html.replace(hash, "../../file"), 2853590).is_none());
    }

    #[test]
    #[ignore = "explicit live Steam smoke test; normal tests are offline"]
    fn live_void_war_icon() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = SteamDownloader::default().icon(2853590).unwrap();
        let path = cache_icon(temp.path(), 2853590, &bytes).unwrap();
        assert!(valid_cache(&path));
    }
}
