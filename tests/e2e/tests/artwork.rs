//! Artwork: Steam's local library cache first, then the CDN (a local server
//! here), scaled into the host's cache and named in the game's summary.
//! Broken downloads never reach the cache; a deleted cache is rebuilt.

mod common;

use std::path::Path;
use std::time::Duration;

use common::http::Server;
use common::*;
use image::{DynamicImage, ImageFormat, ImageReader};

fn image_bytes(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let image = match format {
        ImageFormat::Jpeg => DynamicImage::new_rgb8(w, h),
        _ => DynamicImage::new_rgba8(w, h),
    };
    image.write_to(&mut out, format).unwrap();
    out.into_inner()
}

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn dims(path: &str) -> (u32, u32) {
    ImageReader::open(path).unwrap().with_guessed_format().unwrap().into_dimensions().unwrap()
}

fn wait_art(world: &World, game: &str, what: &str, check: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    wait_for(what, Duration::from_secs(20), || {
        let art = world.game(game)["artwork"].clone();
        check(&art).then_some(art)
    })
}

#[test]
fn art_comes_from_steams_cache_first_then_the_cdn() {
    let world = World::new();
    world.steam_install(1001, "Rogue One", "RogueOne.exe");
    // Steam's current layout: hero in a hashed subfolder, logo and the
    // hash-named icon in the app's folder; no header.
    let cache = world.steam.join("appcache").join("librarycache").join("1001");
    put(
        &cache.join("98878a81ca9047352403db7e19e3942239ea8bf1").join("library_hero.jpg"),
        &image_bytes(3840, 1240, ImageFormat::Jpeg),
    );
    put(&cache.join("logo.png"), &image_bytes(1280, 720, ImageFormat::Png));
    put(&cache.join("33ea124ea8c03a9ce7012d34c3b348a351612fca.jpg"), &image_bytes(32, 32, ImageFormat::Jpeg));
    let cdn = Server::start();
    cdn.serve("/1001/header.jpg", 200, image_bytes(460, 215, ImageFormat::Jpeg), None);
    let _host = world.host_with(&["--artwork-url", &cdn.base], &[]);

    let art = wait_art(&world, "steam-1001", "all four images", |a| {
        ["hero", "logo", "header", "icon"].iter().all(|k| a[k].is_string())
    });
    assert_eq!(dims(art["hero"].as_str().unwrap()), (1280, 413), "scaled to the card, same shape");
    assert_eq!(dims(art["logo"].as_str().unwrap()), (640, 360));
    assert_eq!(dims(art["icon"].as_str().unwrap()), (32, 32));
    assert!(art["hero"].as_str().unwrap().starts_with(world.data.to_str().unwrap()), "in the host's own cache");
    let asked: Vec<String> = cdn.all_requests().into_iter().map(|r| r.path).collect();
    assert_eq!(asked, vec!["/1001/header.jpg"], "only what Steam's cache lacked");

    // Custom games get none.
    let saves = world.root.join("custom-saves");
    std::fs::create_dir_all(&saves).unwrap();
    let (custom, _) = world.custom_game("Homebrew", &saves);
    assert!(world.game(&custom)["artwork"].is_null());

    // Deleting the cache is harmless: the next pass rebuilds it.
    std::fs::remove_dir_all(world.data.join("cache")).unwrap();
    world.ok(&["scan"]);
    wait_art(&world, "steam-1001", "the cache rebuilt", |a| a["hero"].as_str().is_some_and(|p| Path::new(p).is_file()));
}

#[test]
fn a_broken_download_never_reaches_the_cache() {
    let world = World::new();
    world.steam_install(1001, "Rogue One", "RogueOne.exe");
    let cdn = Server::start();
    // A captive portal's page instead of an image, and art Steam lacks.
    cdn.serve("/1001/library_hero.jpg", 200, "<html>log in to the wifi</html>", None);
    cdn.serve("/1001/header.jpg", 200, image_bytes(460, 215, ImageFormat::Jpeg), None);
    let host = world.host_with(&["--artwork-url", &cdn.base], &[]);

    let art = wait_art(&world, "steam-1001", "the header arrives", |a| a["header"].is_string());
    assert!(art["hero"].is_null() && art["logo"].is_null());
    let folder = world.data.join("cache").join("artwork").join("1001");
    let files: Vec<String> =
        std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(!files.iter().any(|f| f.starts_with("hero")), "{files:?}");

    // Failures wait quietly: another scan doesn't ask again right away.
    let before = cdn.requests("/1001/library_hero.jpg").len();
    world.ok(&["scan"]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(cdn.requests("/1001/library_hero.jpg").len(), before);
    assert_eq!(cdn.requests("/1001/logo.png").len(), 1, "not found is remembered");

    // The server is fixed; the next host start picks the hero up.
    drop(host);
    cdn.serve("/1001/library_hero.jpg", 200, image_bytes(1920, 620, ImageFormat::Jpeg), None);
    let _host = world.host_with(&["--artwork-url", &cdn.base], &[]);
    wait_art(&world, "steam-1001", "the hero arrives", |a| a["hero"].is_string());
    assert_eq!(cdn.requests("/1001/logo.png").len(), 1, "still remembered after a restart");
}
