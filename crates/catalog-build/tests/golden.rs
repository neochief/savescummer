//! Golden build: the fixture inputs under `tests/fixtures/catalog/` must
//! produce the committed `catalog.json` byte for byte, every time
//! (PLAN-CATALOG.md 3.6).

use std::fs;
use std::path::PathBuf;

use savescummer_catalog::Bundle;
use savescummer_catalog_build::{Options, Paths, build_from_files, file_matches};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("catalog")
}

fn paths() -> Paths {
    let dir = fixtures();
    Paths {
        games: dir.join("games.csv"),
        addendum: dir.join("addendum.yaml"),
        manifest: dir.join("manifest.yaml"),
        lock: dir.join("manifest.lock"),
    }
}

#[test]
fn fixture_inputs_produce_the_golden_bundle() {
    let outcome = build_from_files(&paths(), Options::default()).unwrap();
    assert!(outcome.report.errors.is_empty(), "{:#?}", outcome.report.errors);
    let text = outcome.bundle.expect("builds").to_json();
    let expected = fs::read_to_string(fixtures().join("catalog.json")).unwrap();
    assert_eq!(text, expected);
    // What the builder writes is what the host loads.
    Bundle::parse(&text).unwrap();
    // The one warning is Nuclear Throne's broad macOS glob.
    assert_eq!(outcome.report.warnings.len(), 1);
    assert!(outcome.report.warnings[0].message.contains("Application Support/*"));
}

#[test]
fn regeneration_is_byte_identical() {
    let first = build_from_files(&paths(), Options::default()).unwrap();
    let second = build_from_files(&paths(), Options::default()).unwrap();
    assert_eq!(first.bundle.unwrap().to_json(), second.bundle.unwrap().to_json());
    assert_eq!(first.report.to_json(), second.report.to_json());
}

#[test]
fn check_ignores_line_endings_but_not_content() {
    let expected = fs::read_to_string(fixtures().join("catalog.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.json");
    fs::write(&file, expected.replace('\n', "\r\n")).unwrap();
    assert!(file_matches(&file, &expected));
    fs::write(&file, expected.replace("HighFleet", "LowFleet")).unwrap();
    assert!(!file_matches(&file, &expected));
    assert!(!file_matches(&dir.path().join("missing.json"), &expected));
}
