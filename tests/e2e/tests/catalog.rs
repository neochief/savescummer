//! Catalog updates: a newer bundle arrives from the network and the library
//! follows it; a broken download, a server error or a bundle for another
//! build never replaces a working catalog; the last good copy survives
//! restarts. A local server stands in for GitHub.

mod common;

use std::time::Duration;

use common::http::Server;
use common::*;
use serde_json::{Value, json};

/// The fixture catalog at another revision, with one more game.
fn catalog_with_new_game(revision: &str) -> Value {
    let mut catalog = fixture_catalog();
    catalog["source"]["revision"] = json!(revision);
    catalog["games"].as_array_mut().unwrap().push(json!({
        "id": "steam-1006",
        "name": "Late Arrival",
        "detect": { "steam": 1006 },
        "installDirs": ["Late Arrival"],
        "executables": { "windows": ["LateArrival.exe"] },
        "save": [ { "path": "{INSTALL_DIR}/saves" } ]
    }));
    catalog
}

fn wait_checked(world: &World) -> Value {
    wait_for("the host looked for a newer catalog", Duration::from_secs(20), || {
        let info = world.ok(&["catalog"]);
        info["checked_at"].is_string().then_some(info)
    })
}

#[test]
fn a_newer_catalog_adds_a_game_and_is_kept_across_restarts() {
    let world = World::new();
    world.steam_install(1006, "Late Arrival", "LateArrival.exe");
    let server = Server::start();
    let bundle = serde_json::to_vec(&catalog_with_new_game("fixture-2")).unwrap();
    server.serve("/catalog.json", 200, bundle, Some("\"v2\""));

    let host = world.host_updating(&server.url("/catalog.json"), &[]);
    // The game the fixture doesn't know appears once the update arrives.
    world.wait_game("steam-1006", "the new catalog's game appears", |g| g["installed"] == true);
    let info = world.ok(&["catalog"]);
    assert_eq!(info["revision"], "fixture-2");
    assert_eq!(info["source"], "downloaded");
    assert_eq!(info["updates"], true);

    // Asking again sends the ETag; nothing changed, nothing rescanned.
    let refreshed = world.ok(&["catalog", "--refresh"]);
    assert_eq!(refreshed["changed"], false);
    let requests = server.requests("/catalog.json");
    assert_eq!(requests.last().unwrap().if_none_match.as_deref(), Some("\"v2\""));

    // Restarted while the server fails: the downloaded catalog stays.
    drop(host);
    server.serve("/catalog.json", 500, "oops", None);
    let _host = world.host_updating(&server.url("/catalog.json"), &[]);
    assert_eq!(world.ok(&["catalog"])["revision"], "fixture-2");
    assert_eq!(world.game("steam-1006")["installed"], true);
    let info = wait_checked(&world);
    assert!(info["problem"].as_str().unwrap().contains("500"), "{info}");
    assert_eq!(info["revision"], "fixture-2");
}

#[test]
fn a_broken_download_never_replaces_a_working_catalog() {
    let world = World::new();
    world.steam_install(1006, "Late Arrival", "LateArrival.exe");
    let server = Server::start();
    server.serve("/catalog.json", 200, r#"{"schema":1,"source":"#, None);
    let _host = world.host_updating(&server.url("/catalog.json"), &[]);

    let info = wait_checked(&world);
    assert_eq!(info["source"], "file", "the built-in catalog stays");
    assert!(info["problem"].as_str().unwrap().contains("invalid"), "{info}");

    // A bundle for a newer build (another schema) is refused the same way.
    let mut future = catalog_with_new_game("fixture-3");
    future["schema"] = json!(99);
    server.serve("/catalog.json", 200, serde_json::to_vec(&future).unwrap(), None);
    let refused = world.ok(&["catalog", "--refresh"]);
    assert_eq!(refused["changed"], false);
    assert!(refused["catalog"]["problem"].as_str().unwrap().contains("schema"), "{refused}");
    // A game with an unknown store id is invalid too.
    let mut bad = catalog_with_new_game("fixture-4");
    bad["games"][0]["save"] = json!([]);
    bad["games"][0]["detect"] = json!({});
    server.serve("/catalog.json", 200, serde_json::to_vec(&bad).unwrap(), None);
    let refused = world.ok(&["catalog", "--refresh"]);
    assert_eq!(refused["changed"], false, "{refused}");
    assert!(world.game("steam-1006").is_null());

    // A good one is taken, and the library is rescanned before the answer.
    let good = serde_json::to_vec(&catalog_with_new_game("fixture-5")).unwrap();
    server.serve("/catalog.json", 200, good, None);
    let updated = world.ok(&["catalog", "--refresh"]);
    assert_eq!(updated["changed"], true);
    assert!(updated["catalog"]["problem"].is_null());
    assert_eq!(world.game("steam-1006")["installed"], true);
}

#[test]
fn an_isolated_host_never_fetches() {
    let world = World::new();
    let server = Server::start();
    let _host = world.host_with(&["--catalog-url", &server.url("/catalog.json")], &[]);
    let info = world.ok(&["catalog", "--refresh"]);
    assert_eq!(info["changed"], false);
    assert_eq!(info["catalog"]["updates"], false);
    std::thread::sleep(Duration::from_millis(300));
    assert!(server.all_requests().is_empty());
}
