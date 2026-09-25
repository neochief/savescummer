//! The pipeline: lookup, addendum merge, identity, and every hard failure
//! and warning of PLAN-CATALOG.md 3.5.

mod common;

use common::*;
use savescummer_catalog::{Executables, Platform};
use savescummer_catalog_build::build::{closest_names, slug};
use savescummer_catalog_build::{IssueKind, Options};

/// A small manifest shared by the merge tests.
const MANIFEST: &str = r#"
Known Game:
  steam:
    id: 100
  installDir:
    Known Game: {}
  launch:
    "<base>/known.exe":
      - when: [{ os: windows }]
  files:
    "<winAppData>/Known/save":
      tags: [save]
      when: [{ os: windows }]
    "<winAppData>/Known/save/settings.ini":
      tags: [config]
      when: [{ os: windows }]
Gog Game:
  gog:
    id: 200
  files:
    "<base>/saves": {}
No Store Game:
  files:
    "<home>/.nostore/save.dat": {}
Slay the Spire:
  files:
    "<base>/saves": {}
"#;

#[test]
fn manifest_wins_and_addendum_fills_gaps() {
    let addendum = r#"
"Known Game":
  detect:
    steam: 999
    gog: 300
    uninstall: ["{1234-ABCD}_is1"]
  executables:
    windows: ["other.exe"]
    linux: ["known.sh"]
  save:
    - when: { os: windows }
      path: "{APPDATA}/Elsewhere"
    - when: { os: linux }
      path: "{XDG_DATA_HOME}/Known/save"
"#;
    let outcome = run(&keep(&["Known Game"]), addendum, MANIFEST);
    let game = game(&outcome, "Known Game");
    // detect: per store key, the manifest's steam id wins, gog is filled.
    assert_eq!(game.detect.steam, vec![100]);
    assert_eq!(game.detect.gog, vec![300]);
    assert_eq!(game.detect.uninstall, vec!["{1234-ABCD}_is1"]);
    // executables: per OS.
    assert_eq!(game.executables.windows, vec!["known.exe"]);
    assert_eq!(game.executables.linux, vec!["known.sh"]);
    // save: the manifest covers windows, the addendum keeps linux.
    assert_eq!(
        game.save,
        vec![win("{APPDATA}/Known/save"), rule("{XDG_DATA_HOME}/Known/save", Some(Platform::Linux), None)]
    );
    assert_eq!(game.exclude, vec![win("{APPDATA}/Known/save/settings.ini")]);

    // Everything the manifest shadowed is named, so it can be deleted.
    let shadowed = issues_of(&outcome.report.warnings, IssueKind::AddendumShadowed);
    assert_eq!(shadowed.len(), 1);
    let message = &shadowed[0].message;
    assert!(message.contains("detect.steam"), "{message}");
    assert!(message.contains("executables.windows"), "{message}");
    assert!(message.contains("save (windows)"), "{message}");
    assert!(!message.contains("detect.gog"), "{message}");
}

#[test]
fn a_fully_shadowed_addendum_changes_nothing() {
    // Void War's lifecycle (3.8): once the manifest has the game, deleting
    // the addendum entry must not change the output.
    let addendum = r#"
"Known Game":
  detect:
    steam: 100
  save:
    - when: { os: windows }
      path: "{APPDATA}/Known/save"
"#;
    let with = run(&keep(&["Known Game"]), addendum, MANIFEST);
    let without = run(&keep(&["Known Game"]), "", MANIFEST);
    assert_eq!(with.bundle, without.bundle);
    assert_eq!(issues_of(&with.report.warnings, IssueKind::AddendumShadowed).len(), 1);
    assert!(without.report.warnings.is_empty());
}

#[test]
fn override_replaces_the_manifest_field_and_is_listed() {
    let addendum = r#"
"Known Game":
  override:
    save:
      - when: { os: windows }
        path: "{APPDATA}/Known/save/slot_{STEAM_ID64}.dat"
    exclude: []
"#;
    let outcome = run(&keep(&["Known Game"]), addendum, MANIFEST);
    let game = game(&outcome, "Known Game");
    assert_eq!(game.save, vec![win("{APPDATA}/Known/save/slot_{STEAM_ID64}.dat")]);
    assert!(game.exclude.is_empty());
    // Other fields still come from the manifest.
    assert_eq!(game.detect.steam, vec![100]);
    let overrides = &outcome.report.review.overrides;
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].game, "Known Game");
    assert_eq!(overrides[0].fields, vec!["save", "exclude"]);
    assert!(outcome.report.warnings.is_empty());
}

#[test]
fn override_silences_dropped_target_warnings_for_the_replaced_save() {
    let manifest = r#"
Broad Game:
  steam:
    id: 5
  files:
    "<base>/save*": {}
"#;
    let without = run(&keep(&["Broad Game"]), "", manifest);
    assert_eq!(issues_of(&without.report.errors, IssueKind::NoUsableSave).len(), 1);
    assert_eq!(issues_of(&without.report.warnings, IssueKind::DroppedTarget).len(), 1);

    let addendum = "\"Broad Game\":\n  override:\n    save:\n      - path: \"{INSTALL_DIR}/saves/*.dat\"\n";
    let with = run(&keep(&["Broad Game"]), addendum, manifest);
    assert_eq!(game(&with, "Broad Game").save, vec![any("{INSTALL_DIR}/saves/*.dat")]);
    assert!(with.report.warnings.is_empty());
}

#[test]
fn addendum_only_games_and_the_retired_dir_key() {
    let addendum = r#"
"New Game":
  detect:
    steam: 2853590
  executables:
    windows: ["New Game.exe"]
  save:
    - when: { os: windows }
      path: "{APPDATA}/New_Game/*.sav"
"#;
    let outcome = run(&keep(&["New Game"]), addendum, MANIFEST);
    let game = game(&outcome, "New Game");
    assert_eq!(game.id, "steam-2853590");
    assert_eq!(game.executables, Executables { windows: vec!["New Game.exe".into()], ..Default::default() });
    assert_eq!(outcome.report.stats.from_addendum_only, 1);

    let old = addendum.replace("path:", "dir:");
    let outcome = run(&keep(&["New Game"]), &old, MANIFEST);
    assert!(outcome.bundle.is_none());
    assert_eq!(issues_of(&outcome.report.errors, IssueKind::AddendumSchema).len(), 1);

    let typo = "\"New Game\":\n  saves: []\n";
    assert_eq!(issues_of(&run(&keep(&["New Game"]), typo, MANIFEST).report.errors, IssueKind::AddendumSchema).len(), 1);
}

#[test]
fn addendum_without_a_keep_row_is_a_warning() {
    let mut csv = keep(&["Known Game"]);
    csv.push_str("\"Gog Game\",\"Remove\",\"\"\n");
    let addendum = "\"Gog Game\":\n  detect:\n    gog: 1\n\"Nowhere\":\n  detect:\n    steam: 1\n";
    let outcome = run(&csv, addendum, MANIFEST);
    assert!(outcome.bundle.is_some());
    let games: Vec<_> = issues_of(&outcome.report.warnings, IssueKind::AddendumWithoutKeepRow)
        .into_iter()
        .map(|i| i.game.clone().unwrap())
        .collect();
    assert_eq!(games, vec!["Gog Game", "Nowhere"]);
    assert_eq!(outcome.bundle.unwrap().games.len(), 1);
}

#[test]
fn identity_prefers_steam_then_gog_then_addendum_then_slug() {
    let addendum = "\"No Store Game\":\n  id: custom-id\n";
    let outcome = run(&keep(&["Known Game", "Gog Game", "No Store Game", "Slay the Spire"]), addendum, MANIFEST);
    assert_eq!(game(&outcome, "Known Game").id, "steam-100");
    assert_eq!(game(&outcome, "Gog Game").id, "gog-200");
    assert_eq!(game(&outcome, "No Store Game").id, "custom-id");
    assert_eq!(game(&outcome, "Slay the Spire").id, "slay-the-spire");
    // Games are sorted by id.
    let ids: Vec<_> = outcome.bundle.unwrap().games.iter().map(|g| g.id.clone()).collect();
    assert_eq!(ids, vec!["custom-id", "gog-200", "slay-the-spire", "steam-100"]);

    assert_eq!(slug("Tom Clancy's The Division 2"), "tom-clancy-s-the-division-2");
    assert_eq!(slug("  !Hi -- There!  "), "hi-there");
}

#[test]
fn info_comes_from_games_csv() {
    let csv = format!("{CSV_HEADER}\"Known Game\",\"Keep\",\"**Bold**, with \"\"quotes\"\"\nand a newline.\"\n");
    let outcome = run(&csv, "", MANIFEST);
    assert_eq!(game(&outcome, "Known Game").info.as_deref(), Some("**Bold**, with \"quotes\"\nand a newline."));
}

#[test]
fn extra_columns_and_a_bom_are_fine() {
    let csv =
        "\u{feff}\"Name\",\"Category\",\"Product fit\",\"Info\",\"Notes\"\n\"Known Game\",\"x\",\"Keep\",\"i\",\"n\"\n";
    let outcome = run(csv, "", MANIFEST);
    assert_eq!(game(&outcome, "Known Game").info.as_deref(), Some("i"));
}

#[test]
fn unknown_names_fail_with_suggestions() {
    let outcome = run(&keep(&["Slay The Spire!"]), "", MANIFEST);
    assert!(outcome.bundle.is_none());
    let errors = issues_of(&outcome.report.errors, IssueKind::UnresolvedName);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("\"Slay the Spire\""), "{}", errors[0].message);

    let names = ["Six Ages: Ride Like the Wind", "Ace Lightning", "Known Game"];
    assert_eq!(
        closest_names("Six Ages 2: Lights Going Out", names.into_iter(), 1),
        vec!["Six Ages: Ride Like the Wind"]
    );
}

#[test]
fn duplicate_names_fail() {
    let csv = format!("{CSV_HEADER}\"Known Game\",\"Keep\",\"\"\n\"known game\",\"Remove\",\"\"\n");
    let outcome = run(&csv, "", MANIFEST);
    assert!(outcome.bundle.is_none());
    assert_eq!(issues_of(&outcome.report.errors, IssueKind::DuplicateName).len(), 1);
}

#[test]
fn invalid_product_fit_fails() {
    let csv = format!("{CSV_HEADER}\"Known Game\",\"Maybe\",\"\"\n");
    let outcome = run(&csv, "", MANIFEST);
    assert!(outcome.bundle.is_none());
    let errors = issues_of(&outcome.report.errors, IssueKind::InvalidProductFit);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("Maybe"));
}

#[test]
fn ignored_rows_are_left_out_with_a_warning() {
    // Even a game that couldn't be built (not in the manifest) is fine once ignored.
    let csv = format!("{CSV_HEADER}\"Known Game\",\"Keep\",\"\"\n\"Missing Game\",\"Ignored\",\"\"\n");
    let outcome = run(&csv, "", MANIFEST);
    let bundle = outcome.bundle.expect("builds");
    assert_eq!(bundle.games.len(), 1);
    assert!(outcome.report.errors.is_empty());
    let warnings = issues_of(&outcome.report.warnings, IssueKind::IgnoredGame);
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].game.as_deref(), Some("Missing Game"));
    assert_eq!(outcome.report.stats.ignored_rows, 1);
}

#[test]
fn manifest_hash_mismatch_fails() {
    let csv = keep(&["Known Game"]);
    let lock = lock_for("something else");
    let outcome = savescummer_catalog_build::build(&savescummer_catalog_build::Inputs {
        games_csv: &csv,
        addendum: "",
        manifest: MANIFEST.as_bytes(),
        lock: &lock,
    });
    assert!(outcome.bundle.is_none());
    assert_eq!(outcome.report.errors.len(), 1);
    assert_eq!(outcome.report.errors[0].kind, IssueKind::ManifestHash);
}

#[test]
fn no_usable_save_fails_with_its_category() {
    let manifest = r#"
Config Only:
  files:
    "<base>/setup.ini":
      tags: [config]
No Files:
  steam:
    id: 1
Store Only:
  files:
    "<winLocalAppData>/Packages/X/LocalCache":
      when: [{ os: windows, store: microsoft }]
"#;
    let outcome = run(&keep(&["Config Only", "No Files", "Store Only"]), "", manifest);
    assert!(outcome.bundle.is_none());
    let messages: Vec<_> =
        issues_of(&outcome.report.errors, IssueKind::NoUsableSave).iter().map(|i| i.message.clone()).collect();
    assert_eq!(messages.len(), 3);
    assert!(messages[0].contains("config-only"));
    assert!(messages[1].contains("no files section"));
    assert!(messages[2].contains("MS-Store-only"));
}

#[test]
fn allow_unbuildable_leaves_those_games_out_with_a_warning() {
    let manifest = "No Files:\n  steam:\n    id: 1\nGood:\n  files:\n    \"<base>/save\": {}\n";
    let outcome = run_with(&keep(&["No Files", "Missing", "Good"]), "", manifest, Options { allow_unbuildable: true });
    let bundle = outcome.bundle.expect("builds");
    assert_eq!(bundle.games.len(), 1);
    assert!(outcome.report.errors.is_empty());
    assert_eq!(issues_of(&outcome.report.warnings, IssueKind::NoUsableSave).len(), 1);
    assert_eq!(issues_of(&outcome.report.warnings, IssueKind::UnresolvedName).len(), 1);

    // Other hard errors stay errors.
    let csv = format!("{CSV_HEADER}\"Good\",\"Keep\",\"\"\n\"good\",\"Keep\",\"\"\n");
    let outcome = run_with(&csv, "", manifest, Options { allow_unbuildable: true });
    assert!(outcome.bundle.is_none());
}

#[test]
fn dropped_broad_targets_warn_even_when_others_remain() {
    let manifest = r#"
Mixed:
  files:
    "<base>/save*":
      when: [{ os: windows }]
    "<xdgData>/Mixed/save":
      when: [{ os: linux }]
"#;
    let outcome = run(&keep(&["Mixed"]), "", manifest);
    assert!(outcome.bundle.is_some());
    let warnings = issues_of(&outcome.report.warnings, IssueKind::DroppedTarget);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("<base>/save*"));
}

#[test]
fn rules_for_an_os_that_never_resolves_their_placeholder_drop_with_a_warning() {
    // The Long Dark's shape: `{XDG_DATA_HOME}` only resolves on Linux and
    // `{DOCUMENTS}` only on Windows (PLAN-CATALOG.md 4.4).
    let manifest = r#"
Foreign:
  files:
    "<xdgData>/Foreign/save":
      when: [{ os: mac }]
    "<xdgData>/Foreign/linux":
      when: [{ os: linux }]
"#;
    let addendum = r#"
"Foreign":
  exclude:
    - when: { os: macos }
      path: "{DOCUMENTS}/Foreign/save/settings.ini"
"#;
    let outcome = run(&keep(&["Foreign"]), addendum, manifest);
    let built = game(&outcome, "Foreign");
    assert_eq!(built.save, vec![rule("{XDG_DATA_HOME}/Foreign/linux", Some(Platform::Linux), None)]);
    assert!(built.exclude.is_empty());
    let warnings = issues_of(&outcome.report.warnings, IssueKind::DroppedTarget);
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    assert!(warnings[0].message.contains("{XDG_DATA_HOME} doesn't resolve on macos"));
    assert!(warnings[1].message.contains("{DOCUMENTS} doesn't resolve on macos"));

    // The addendum's fix replaces the row, and the warning goes away.
    let fix = r#"
"Foreign":
  override:
    save:
      - when: { os: macos }
        path: "{HOME}/.local/share/Foreign/save"
      - when: { os: linux }
        path: "{XDG_DATA_HOME}/Foreign/linux"
"#;
    let fixed = run(&keep(&["Foreign"]), fix, manifest);
    assert_eq!(game(&fixed, "Foreign").save.len(), 2);
    assert!(fixed.report.warnings.is_empty());
}

#[test]
fn review_lists_multi_target_games_and_config_save_folders() {
    let manifest = r#"
Paradox:
  files:
    "<winDocuments>/Paradox Interactive/Game":
      tags: [config, save]
      when: [{ os: windows }]
    "<home>/.local/share/Paradox Interactive/Game":
      tags: [config, save]
      when: [{ os: linux }]
"#;
    let outcome = run(&keep(&["Paradox"]), "", manifest);
    let review = &outcome.report.review;
    assert_eq!(review.multi_target.len(), 1);
    assert_eq!(review.multi_target[0].targets, 2);
    assert_eq!(review.config_save_folders.len(), 2);

    // An override narrowing the folder takes it off the list.
    let addendum =
        "\"Paradox\":\n  override:\n    save:\n      - path: \"{DOCUMENTS}/Paradox Interactive/Game/save games\"\n";
    let outcome = run(&keep(&["Paradox"]), addendum, manifest);
    assert!(outcome.report.review.config_save_folders.is_empty());
    assert!(outcome.report.review.multi_target.is_empty());
}

#[test]
fn highfleet_worked_example() {
    // PLAN-CATALOG.md 3.7, from the pinned manifest's real entry.
    let manifest = r#"
HighFleet:
  cloud:
    steam: true
  files:
    "<base>/Config.ini":
      tags:
        - config
      when:
        - os: windows
    "<base>/Saves":
      tags:
        - save
      when:
        - os: windows
    "<base>/SavesSkirmish":
      tags:
        - save
      when:
        - os: windows
    "<base>/Ships":
      tags:
        - save
      when:
        - os: windows
    "<root>/steamapps/common/HighFleet/Config.ini":
      tags:
        - config
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Saves":
      tags:
        - save
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/SavesSkirmish":
      tags:
        - save
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Ships":
      tags:
        - save
      when:
        - store: steam
  gog:
    id: 1589167087
  installDir:
    HighFleet: {}
  launch:
    "<base>/Highfleet.exe":
      - when:
          - os: windows
            store: steam
  steam:
    id: 1434950
"#;
    let csv = format!("{CSV_HEADER}\"HighFleet\",\"Keep\",\"Exit to main menu before saving...\"\n");
    let outcome = run(&csv, "", manifest);
    let json = outcome.bundle.expect("builds").to_json();
    let expected = r#"{
  "schema": 1,
  "source": {
    "repo": "test/manifest",
    "revision": "abc123"
  },
  "games": [
    {
      "id": "steam-1434950",
      "name": "HighFleet",
      "info": "Exit to main menu before saving...",
      "detect": {
        "steam": 1434950,
        "gog": 1589167087
      },
      "installDirs": [
        "HighFleet"
      ],
      "executables": {
        "windows": [
          "Highfleet.exe"
        ]
      },
      "save": [
        {
          "when": {
            "os": "windows"
          },
          "path": "{INSTALL_DIR}/Saves"
        },
        {
          "when": {
            "os": "windows"
          },
          "path": "{INSTALL_DIR}/SavesSkirmish"
        },
        {
          "when": {
            "os": "windows"
          },
          "path": "{INSTALL_DIR}/Ships"
        }
      ]
    }
  ]
}
"#;
    assert_eq!(json, expected);
    assert!(outcome.report.warnings.is_empty());
    assert_eq!(outcome.report.review.multi_target.len(), 1);
}
