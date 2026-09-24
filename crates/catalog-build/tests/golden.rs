//! File-based golden test: fixture inputs must reproduce the committed bundle
//! byte for byte (PLAN-CATALOG.md §7.1).

use savescummer_catalog_build::{Inputs, Lock, build};

#[test]
fn golden_bundle_is_reproducible_from_fixtures() {
    let lock = Lock::parse(include_str!("fixtures/catalog/manifest.lock")).unwrap();
    let output = build(&Inputs {
        games_csv: include_str!("fixtures/catalog/games.csv"),
        addendum_yaml: include_str!("fixtures/catalog/addendum.yaml"),
        manifest_yaml: include_str!("fixtures/catalog/manifest.yaml"),
        lock: &lock,
    })
    .expect("fixture build succeeds");
    assert_eq!(
        output.bundle.to_json_pretty().unwrap(),
        include_str!("fixtures/catalog/catalog.json")
    );
    assert!(
        output
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("shadowed")),
        "{:?}",
        output.report.warnings
    );
}
