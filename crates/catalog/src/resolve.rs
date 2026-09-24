//! The decision module. The host observes the machine through [`Probe`], passes
//! the result to [`resolve`], and persists the chosen directory. No OS APIs,
//! database or host types appear here.

use crate::{
    model::{Game, Platform, Store},
    placeholders,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Every environment observation the resolver needs. Tests implement this
/// against in-memory maps and never touch a real filesystem.
pub trait Probe {
    fn is_dir(&self, path: &Path) -> bool;
    fn install_identity(&self, install_dir: &Path) -> Option<String>;
    /// Newest modification among files matching `globs`, or all files when empty.
    fn newest_activity(&self, dir: &Path, globs: &[String]) -> Option<u64>;
    /// Directory names directly under `dir`. Only Proton prefix profile
    /// selection uses this; the default is empty so simple probes stay tiny.
    fn child_dirs(&self, _dir: &Path) -> Vec<String> {
        Vec::new()
    }
}

/// Produced by the store-discovery scanner, passed in by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    /// Catalog id (for example `steam-1434950`), before install-level splitting.
    pub catalog_id: String,
    pub store: Store,
    /// The build being run: Windows native/Proton, macOS or Linux.
    pub platform: Platform,
    pub install_dir: PathBuf,
    pub executables: Vec<PathBuf>,
    /// Set for a Windows build running through Proton.
    pub proton_prefix: Option<PathBuf>,
    /// Stable install identity (volume/file id), when the scanner could obtain it.
    pub identity: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Environment {
    /// Known folders keyed by placeholder name (`APPDATA`, `HOME`, ...).
    pub folders: BTreeMap<String, PathBuf>,
    /// Resolved `<root>/userdata/<current steam user>` directory.
    pub steam_userdata: Option<PathBuf>,
    /// Sticky value persisted by the host. Kept while it still exists.
    pub current_pick: Option<PathBuf>,
    /// Detected store user id for `{STORE_USER_ID}` (the Steam id for Steam).
    pub store_user_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The persisted pick still exists.
    Sticky,
    /// Exactly one candidate directory exists.
    OnlyExisting,
    /// Newest save activity decided among several existing candidates.
    NewestActivity,
    /// The exact save-directory name rule decided.
    ExactName,
    /// Ambiguity fell through to the first candidate.
    FirstCandidate,
    /// A safe ancestor stood in for the whole candidate set.
    Ancestor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Chosen {
        /// Install-level id; [`assign_games`] may rewrite it.
        game_id: String,
        catalog_id: String,
        install_identity: Option<String>,
        executables: Vec<PathBuf>,
        data_dir: PathBuf,
        candidates: Vec<PathBuf>,
        reason: Reason,
    },
    Unsupported {
        game_id: String,
        catalog_id: String,
        install_identity: Option<String>,
        executables: Vec<PathBuf>,
        candidates: Vec<PathBuf>,
        reason: String,
    },
}

/// One record per install, or one record for installs that share a save dir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRecord {
    pub game_id: String,
    pub catalog_id: String,
    pub data_dir: Option<PathBuf>,
    pub executables: Vec<PathBuf>,
    pub candidates: Vec<PathBuf>,
    pub install_ids: Vec<String>,
    pub unsupported: Option<String>,
}

const EXACT_NAMES: &[&str] = &[
    "save",
    "saves",
    "savegame",
    "savegames",
    "saved games",
    "save data",
];

pub fn resolve(
    game: &Game,
    install: &Install,
    environment: &Environment,
    probe: &dyn Probe,
) -> Decision {
    let candidates = filtered_candidates(game, install, environment, probe);
    if let Some(pick) = &environment.current_pick
        && probe.is_dir(pick)
    {
        return Decision::Chosen {
            game_id: install.catalog_id.clone(),
            catalog_id: install.catalog_id.clone(),
            install_identity: install.identity.clone(),
            executables: install.executables.clone(),
            data_dir: pick.clone(),
            candidates,
            reason: Reason::Sticky,
        };
    }
    if candidates.is_empty() {
        return Decision::Unsupported {
            game_id: install.catalog_id.clone(),
            catalog_id: install.catalog_id.clone(),
            install_identity: install.identity.clone(),
            executables: install.executables.clone(),
            candidates,
            reason: "no save candidate applies to this install".into(),
        };
    }
    let (data_dir, reason) = choose(&candidates, install, environment, probe);
    Decision::Chosen {
        game_id: install.catalog_id.clone(),
        catalog_id: install.catalog_id.clone(),
        install_identity: install.identity.clone(),
        executables: install.executables.clone(),
        data_dir,
        candidates,
        reason,
    }
}

fn filtered_candidates(
    game: &Game,
    install: &Install,
    environment: &Environment,
    probe: &dyn Probe,
) -> Vec<PathBuf> {
    let mut resolved = Vec::new();
    for candidate in &game.save {
        let applies = candidate
            .when
            .as_ref()
            .map(|when| when.applies(install.platform, install.store))
            .unwrap_or(true);
        if !applies {
            continue;
        }
        if let Some(path) = placeholders::resolve(&candidate.dir, install, environment, probe)
            && !resolved.contains(&path)
        {
            resolved.push(path);
        }
    }
    resolved
}

fn choose(
    candidates: &[PathBuf],
    install: &Install,
    environment: &Environment,
    probe: &dyn Probe,
) -> (PathBuf, Reason) {
    let existing: Vec<&PathBuf> = candidates
        .iter()
        .filter(|path| probe.is_dir(path))
        .collect();
    match existing.len() {
        0 => {
            if let Some(path) = exact_name(&candidates.iter().collect::<Vec<_>>()) {
                return (path, Reason::ExactName);
            }
            (candidates[0].clone(), Reason::FirstCandidate)
        }
        1 => (existing[0].clone(), Reason::OnlyExisting),
        _ => {
            if let Some(path) = newest_activity(&existing, probe) {
                return (path, Reason::NewestActivity);
            }
            if let Some(path) = exact_name(&existing) {
                return (path, Reason::ExactName);
            }
            if let Some(path) = ancestor(candidates, install, environment) {
                return (path, Reason::Ancestor);
            }
            (existing[0].clone(), Reason::FirstCandidate)
        }
    }
}

/// The unique candidate with the newest activity, or `None` when the maximum is
/// tied or unavailable (so the ladder continues).
fn newest_activity(existing: &[&PathBuf], probe: &dyn Probe) -> Option<PathBuf> {
    let mut best: Option<(u64, &PathBuf)> = None;
    let mut tied = false;
    for path in existing {
        let Some(activity) = probe.newest_activity(path, &[]) else {
            continue;
        };
        match best {
            None => best = Some((activity, path)),
            Some((current, _)) if activity > current => best = Some((activity, path)),
            Some((current, _)) if activity == current => tied = true,
            _ => {}
        }
    }
    if tied {
        return None;
    }
    best.map(|(_, path)| path.clone())
}

fn exact_name(existing: &[&PathBuf]) -> Option<PathBuf> {
    existing
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    EXACT_NAMES
                        .iter()
                        .any(|exact| name.eq_ignore_ascii_case(exact))
                })
        })
        .map(|path| (*path).clone())
}

/// A candidate that is a strict ancestor of every other candidate and sits at
/// least one named segment below a known-folder/install root.
fn ancestor(
    candidates: &[PathBuf],
    install: &Install,
    environment: &Environment,
) -> Option<PathBuf> {
    let mut best: Option<&PathBuf> = None;
    for path in candidates {
        if !path_starts_with_any_protected(path, install, environment)
            && candidates
                .iter()
                .filter(|other| *other != path)
                .all(|other| other.starts_with(path))
            && best.is_none_or(|current| path.as_os_str().len() > current.as_os_str().len())
        {
            best = Some(path);
        }
    }
    best.cloned()
}

fn path_starts_with_any_protected(
    path: &Path,
    install: &Install,
    environment: &Environment,
) -> bool {
    if path == install.install_dir {
        return true;
    }
    if path.parent().is_none() {
        return true;
    }
    environment
        .folders
        .values()
        .chain(std::iter::once(&install.install_dir))
        .any(|root| root == path)
}

/// Merges decisions whose chosen directory is identical and assigns
/// install-level ids: the first install per catalog id keeps the base id, the
/// others get `#<identity>` (`#<index>` when identity is unavailable).
pub fn assign_games(decisions: &[Decision]) -> Vec<GameRecord> {
    let mut chosen: BTreeMap<String, Vec<Named>> = BTreeMap::new();
    let mut unsupported = Vec::new();
    for decision in decisions {
        match decision {
            Decision::Chosen {
                catalog_id,
                install_identity,
                executables,
                data_dir,
                candidates,
                ..
            } => chosen.entry(catalog_id.clone()).or_default().push(Named {
                id: catalog_id.clone(),
                catalog_id: catalog_id.clone(),
                identity: install_identity.clone(),
                executables: executables.clone(),
                data_dir: data_dir.clone(),
                candidates: candidates.clone(),
            }),
            Decision::Unsupported {
                game_id,
                catalog_id,
                install_identity,
                executables,
                candidates,
                reason,
            } => unsupported.push(GameRecord {
                game_id: game_id.clone(),
                catalog_id: catalog_id.clone(),
                data_dir: None,
                executables: executables.clone(),
                candidates: candidates.clone(),
                install_ids: install_identity.iter().cloned().collect(),
                unsupported: Some(reason.clone()),
            }),
        }
    }
    let mut named = Vec::new();
    for (catalog_id, mut installs) in chosen {
        installs.sort_by(|a, b| {
            a.identity
                .cmp(&b.identity)
                .then_with(|| a.data_dir.cmp(&b.data_dir))
        });
        for (index, mut install) in installs.into_iter().enumerate() {
            if index > 0 {
                let suffix = install
                    .identity
                    .clone()
                    .unwrap_or_else(|| index.to_string());
                install.id = format!("{catalog_id}#{suffix}");
            }
            named.push(install);
        }
    }

    let mut records: Vec<GameRecord> = Vec::new();
    for install in named {
        if let Some(record) = records
            .iter_mut()
            .find(|record| record.data_dir.as_ref() == Some(&install.data_dir))
        {
            record.executables.extend(install.executables);
            record.executables.sort();
            record.executables.dedup();
            record.candidates.extend(install.candidates);
            record.candidates.sort();
            record.candidates.dedup();
            if let Some(identity) = install.identity {
                record.install_ids.push(identity);
            }
            continue;
        }
        records.push(GameRecord {
            game_id: install.id,
            catalog_id: install.catalog_id,
            data_dir: Some(install.data_dir),
            executables: install.executables,
            candidates: install.candidates,
            install_ids: install.identity.into_iter().collect(),
            unsupported: None,
        });
    }
    records.extend(unsupported);
    records
}

struct Named {
    id: String,
    catalog_id: String,
    identity: Option<String>,
    executables: Vec<PathBuf>,
    data_dir: PathBuf,
    candidates: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Detect, IdList, SaveCandidate, When};

    #[derive(Default)]
    struct FakeProbe {
        dirs: std::collections::BTreeSet<PathBuf>,
        activity: BTreeMap<PathBuf, u64>,
        identities: BTreeMap<PathBuf, String>,
        children: BTreeMap<PathBuf, Vec<String>>,
    }
    impl FakeProbe {
        fn with_dirs(dirs: &[&str]) -> Self {
            Self {
                dirs: dirs.iter().map(PathBuf::from).collect(),
                ..Default::default()
            }
        }
    }
    impl Probe for FakeProbe {
        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains(path)
        }
        fn install_identity(&self, install_dir: &Path) -> Option<String> {
            self.identities.get(install_dir).cloned()
        }
        fn newest_activity(&self, dir: &Path, _globs: &[String]) -> Option<u64> {
            self.activity.get(dir).copied()
        }
        fn child_dirs(&self, dir: &Path) -> Vec<String> {
            self.children.get(dir).cloned().unwrap_or_default()
        }
    }

    fn base_env() -> Environment {
        Environment {
            folders: BTreeMap::from([
                (
                    "APPDATA".into(),
                    PathBuf::from(r"C:\Users\me\AppData\Roaming"),
                ),
                (
                    "LOCALAPPDATA".into(),
                    PathBuf::from(r"C:\Users\me\AppData\Local"),
                ),
                ("HOME".into(), PathBuf::from(r"C:\Users\me")),
                ("PROGRAMFILES".into(), PathBuf::from(r"C:\Program Files")),
            ]),
            steam_userdata: Some(PathBuf::from(r"C:\Steam\root\userdata\12345")),
            current_pick: None,
            store_user_id: Some("12345".into()),
        }
    }

    fn install(candidates: &[(&str, Option<When>)], store: Store) -> (Game, Install) {
        let game = Game {
            id: "steam-1".into(),
            name: "Game".into(),
            info: String::new(),
            detect: Some(Detect {
                steam: Some(IdList::one(1)),
                gog: None,
            }),
            executables: BTreeMap::new(),
            save: candidates
                .iter()
                .map(|(dir, when)| SaveCandidate {
                    when: *when,
                    dir: (*dir).into(),
                })
                .collect(),
        };
        let install = Install {
            catalog_id: "steam-1".into(),
            store,
            platform: Platform::Windows,
            install_dir: PathBuf::from(r"D:\Games\Game"),
            executables: vec![PathBuf::from(r"D:\Games\Game\game.exe")],
            proton_prefix: None,
            identity: Some("vol:42".into()),
        };
        (game, install)
    }

    fn windows() -> Option<When> {
        Some(When {
            os: Some(Platform::Windows),
            store: None,
        })
    }

    #[test]
    fn single_candidate_is_chosen_without_prompt() {
        let (game, install) = install(&[("{INSTALL_DIR}/save", None)], Store::Steam);
        let probe = FakeProbe::with_dirs(&[r"D:\Games\Game\save"]);
        let decision = resolve(&game, &install, &base_env(), &probe);
        match decision {
            Decision::Chosen {
                data_dir, reason, ..
            } => {
                assert_eq!(data_dir, PathBuf::from(r"D:\Games\Game\save"));
                assert_eq!(reason, Reason::OnlyExisting);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn exact_name_breaks_a_multi_target_tie() {
        let (game, install) = install(
            &[
                ("{INSTALL_DIR}/Saves", windows()),
                ("{INSTALL_DIR}/SavesSkirmish", windows()),
                ("{INSTALL_DIR}/Ships", windows()),
            ],
            Store::Steam,
        );
        let probe = FakeProbe::with_dirs(&[
            r"D:\Games\Game\Saves",
            r"D:\Games\Game\SavesSkirmish",
            r"D:\Games\Game\Ships",
        ]);
        match resolve(&game, &install, &base_env(), &probe) {
            Decision::Chosen {
                data_dir, reason, ..
            } => {
                assert_eq!(data_dir, PathBuf::from(r"D:\Games\Game\Saves"));
                assert_eq!(reason, Reason::ExactName);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn newest_activity_wins_when_names_are_not_exact() {
        let (game, install) = install(
            &[
                ("{INSTALL_DIR}/profiles", None),
                ("{INSTALL_DIR}/live_profiles", None),
            ],
            Store::Steam,
        );
        let mut probe =
            FakeProbe::with_dirs(&[r"D:\Games\Game\profiles", r"D:\Games\Game\live_profiles"]);
        probe
            .activity
            .insert(PathBuf::from(r"D:\Games\Game\profiles"), 100);
        probe
            .activity
            .insert(PathBuf::from(r"D:\Games\Game\live_profiles"), 200);
        match resolve(&game, &install, &base_env(), &probe) {
            Decision::Chosen {
                data_dir, reason, ..
            } => {
                assert_eq!(data_dir, PathBuf::from(r"D:\Games\Game\live_profiles"));
                assert_eq!(reason, Reason::NewestActivity);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sticky_pick_is_kept_while_it_exists_and_re_resolved_otherwise() {
        let (game, install) = install(
            &[("{INSTALL_DIR}/old", None), ("{INSTALL_DIR}/new", None)],
            Store::Steam,
        );
        let mut env = base_env();
        env.current_pick = Some(PathBuf::from(r"D:\Games\Game\old"));
        let probe = FakeProbe::with_dirs(&[r"D:\Games\Game\old", r"D:\Games\Game\new"]);
        match resolve(&game, &install, &env, &probe) {
            Decision::Chosen {
                data_dir, reason, ..
            } => {
                assert_eq!(data_dir, PathBuf::from(r"D:\Games\Game\old"));
                assert_eq!(reason, Reason::Sticky);
            }
            other => panic!("{other:?}"),
        }
        // Deleted pick re-runs the ladder: only the new dir exists.
        let probe = FakeProbe::with_dirs(&[r"D:\Games\Game\new"]);
        match resolve(&game, &install, &env, &probe) {
            Decision::Chosen {
                data_dir, reason, ..
            } => {
                assert_eq!(data_dir, PathBuf::from(r"D:\Games\Game\new"));
                assert_eq!(reason, Reason::OnlyExisting);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn store_scoped_candidates_filter_by_install_store() {
        let (game, steam_install) = install(
            &[
                (
                    "{APPDATA}/Returnal/steam",
                    Some(When {
                        os: None,
                        store: Some(Store::Steam),
                    }),
                ),
                (
                    "{APPDATA}/Returnal/epic",
                    Some(When {
                        os: None,
                        store: Some(Store::Epic),
                    }),
                ),
            ],
            Store::Steam,
        );
        let probe = FakeProbe::with_dirs(&[]);
        match resolve(&game, &steam_install, &base_env(), &probe) {
            Decision::Chosen { candidates, .. } => {
                assert_eq!(
                    candidates,
                    vec![PathBuf::from(r"C:\Users\me\AppData\Roaming\Returnal\steam")]
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fresh_install_falls_back_to_exact_name_then_first() {
        let (game, install) = install(
            &[("{INSTALL_DIR}/data", None), ("{INSTALL_DIR}/save", None)],
            Store::Steam,
        );
        let probe = FakeProbe::with_dirs(&[]);
        match resolve(&game, &install, &base_env(), &probe) {
            Decision::Chosen {
                data_dir, reason, ..
            } => {
                assert_eq!(data_dir, PathBuf::from(r"D:\Games\Game\save"));
                assert_eq!(reason, Reason::ExactName);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unsupported_when_no_candidate_applies() {
        let (game, install) = install(
            &[(
                "{APPDATA}/game",
                Some(When {
                    os: None,
                    store: Some(Store::Gog),
                }),
            )],
            Store::Steam,
        );
        let probe = FakeProbe::with_dirs(&[]);
        assert!(matches!(
            resolve(&game, &install, &base_env(), &probe),
            Decision::Unsupported { .. }
        ));
    }

    #[test]
    fn safe_ancestor_stands_in_for_the_whole_set() {
        let (game, install) = install(
            &[("{HOME}/Game", None), ("{HOME}/Game/profiles", None)],
            Store::Steam,
        );
        let probe = FakeProbe::with_dirs(&[r"C:\Users\me\Game", r"C:\Users\me\Game\profiles"]);
        match resolve(&game, &install, &base_env(), &probe) {
            Decision::Chosen {
                data_dir, reason, ..
            } => {
                assert_eq!(data_dir, PathBuf::from(r"C:\Users\me\Game"));
                assert_eq!(reason, Reason::Ancestor);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn assign_games_splits_same_store_installs_and_merges_shared_dirs() {
        let decisions = vec![
            Decision::Chosen {
                game_id: "steam-1".into(),
                catalog_id: "steam-1".into(),
                install_identity: Some("vol:a".into()),
                executables: vec![PathBuf::from(r"D:\A\game.exe")],
                data_dir: PathBuf::from(r"C:\Saves\Game"),
                candidates: vec![],
                reason: Reason::OnlyExisting,
            },
            Decision::Chosen {
                game_id: "steam-1".into(),
                catalog_id: "steam-1".into(),
                install_identity: Some("vol:b".into()),
                executables: vec![PathBuf::from(r"E:\B\game.exe")],
                data_dir: PathBuf::from(r"C:\Saves\Game"),
                candidates: vec![],
                reason: Reason::OnlyExisting,
            },
            Decision::Chosen {
                game_id: "steam-1".into(),
                catalog_id: "steam-1".into(),
                install_identity: Some("vol:c".into()),
                executables: vec![PathBuf::from(r"F:\C\game.exe")],
                data_dir: PathBuf::from(r"C:\Saves\Other"),
                candidates: vec![],
                reason: Reason::OnlyExisting,
            },
        ];
        let records = assign_games(&decisions);
        assert_eq!(records.len(), 2);
        let merged = records
            .iter()
            .find(|record| record.data_dir.as_ref() == Some(&PathBuf::from(r"C:\Saves\Game")))
            .unwrap();
        assert_eq!(merged.game_id, "steam-1");
        assert_eq!(
            merged.executables,
            vec![
                PathBuf::from(r"D:\A\game.exe"),
                PathBuf::from(r"E:\B\game.exe")
            ]
        );
        assert_eq!(merged.install_ids, vec!["vol:a", "vol:b"]);
        let split = records
            .iter()
            .find(|record| record.data_dir.as_ref() == Some(&PathBuf::from(r"C:\Saves\Other")))
            .unwrap();
        assert_eq!(split.game_id, "steam-1#vol:c");
    }
}
