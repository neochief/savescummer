//! Save set safety (PLAN-HOST.md): every target, from the catalog, an
//! override or a custom game, passes the same checks. They're about what a
//! Load may overwrite or delete, so a target is judged by its root and its
//! filter together.

use std::path::{Component, Path, PathBuf};

use crate::common::{is_within, path_key};
use crate::{ErrorKind, Failure, Filter, Target, has_reserved_suffix};

/// The file system facts the checks need.
pub trait RealPaths {
    /// The real path: links, junctions and case resolved for the part that
    /// exists, the rest appended as given. An error means a link that can't
    /// be resolved.
    fn real(&self, path: &Path) -> Result<PathBuf, String>;
    fn case_insensitive(&self) -> bool;
}

/// Another game's targets, for the overlap check.
pub struct OtherGame<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub targets: &'a [Target],
}

pub struct SafetyInput<'a> {
    pub game_id: &'a str,
    pub targets: &'a [Target],
    /// Shared folders other programs use, as real paths: OS folders, AppData
    /// roots, Documents, Steam library roots and the like.
    pub broad: &'a [PathBuf],
    /// The game's install folder (for a custom game, the folder holding its
    /// executable). It holds the game's own files, so it's broad too.
    pub install_dir: Option<&'a Path>,
    pub executables: &'a [PathBuf],
    pub others: &'a [OtherGame<'a>],
}

/// Checks every target and returns them with real roots, or the first
/// problem. These are hard errors, not warnings.
pub fn check(input: &SafetyInput<'_>, paths: &dyn RealPaths) -> Result<Vec<Target>, Failure> {
    let ci = paths.case_insensitive();
    let mut broad: Vec<PathBuf> = input.broad.to_vec();
    if let Some(dir) = input.install_dir {
        broad.push(real(paths, dir)?);
    }
    let mut checked = Vec::new();
    for target in input.targets {
        checked.push(check_one(input, target, &broad, paths, ci)?);
    }
    Ok(checked)
}

fn real(paths: &dyn RealPaths, path: &Path) -> Result<PathBuf, Failure> {
    paths
        .real(path)
        .map_err(|why| Failure::new(ErrorKind::InvalidTarget, format!("a link can't be resolved: {why}")).path(path))
}

fn check_one(
    input: &SafetyInput<'_>,
    target: &Target,
    broad: &[PathBuf],
    paths: &dyn RealPaths,
    ci: bool,
) -> Result<Target, Failure> {
    let fail = |kind: ErrorKind, detail: String| Failure::new(kind, detail).path(&target.root).game(input.game_id);
    if !target.root.is_absolute() {
        return Err(fail(ErrorKind::InvalidConfig, "use a full path".into()));
    }
    match &target.filter {
        Filter::Exact(name) if name.contains(['/', '\\']) || name == "." || name == ".." => {
            return Err(fail(ErrorKind::InvalidConfig, format!("{name:?} isn't a single name")));
        }
        _ => {}
    }
    if names_reserved(&target.filter) {
        return Err(fail(ErrorKind::InvalidTarget, "the filter names a reserved suffix (.ssnew, .ssold)".into()));
    }

    let root = real(paths, &target.root)?;
    let claim = match &target.filter {
        Filter::Exact(name) => root.join(name),
        _ => root.clone(),
    };

    // Drive, volume and share roots are broad for every target.
    let mut broad: Vec<PathBuf> = broad.to_vec();
    if let Some(drive) = drive_root(&claim) {
        broad.push(drive);
    }
    for folder in &broad {
        // Taking a broad folder whole, or anything that contains one.
        if is_within(folder, &claim, ci) {
            return Err(fail(ErrorKind::InvalidTarget, format!("takes the shared folder {}", folder.display())));
        }
        // A wildcard directly in a broad folder.
        if matches!(target.filter, Filter::Pattern(_)) && path_key(&root, ci) == path_key(folder, ci) {
            return Err(fail(
                ErrorKind::InvalidTarget,
                format!("a wildcard directly in the shared folder {}", folder.display()),
            ));
        }
    }

    for exe in input.executables {
        let Ok(exe) = paths.real(exe) else { continue };
        if let Some(relative) = relative(&exe, &root, ci) {
            let parts: Vec<&str> = relative.iter().map(String::as_str).collect();
            if target.filter.covers(&parts, ci) {
                return Err(
                    fail(ErrorKind::InvalidTarget, "the filter would match the game's executable".into()).path(&exe)
                );
            }
        }
    }

    for other in input.others {
        if other.id == input.game_id {
            continue;
        }
        for theirs in other.targets {
            let their_root = paths.real(&theirs.root).unwrap_or_else(|_| theirs.root.clone());
            let their_claim = match &theirs.filter {
                Filter::Exact(name) => their_root.join(name),
                _ => their_root,
            };
            if is_within(&claim, &their_claim, ci) || is_within(&their_claim, &claim, ci) {
                return Err(Failure::new(
                    ErrorKind::InvalidTarget,
                    format!("overlaps the saves of {} ({})", other.name, other.id),
                )
                .path(&target.root)
                .path(&theirs.root)
                .game(input.game_id));
            }
        }
    }

    Ok(Target { root, filter: target.filter.clone(), excludes: target.excludes.clone(), presence: target.presence })
}

/// Whether a filter could match a reserved suffix on its own: an exact name
/// or a pattern whose last segment ends in `.ssnew`/`.ssold` literally.
/// (`save*` is fine: reserved names are excluded from every filter.)
fn names_reserved(filter: &Filter) -> bool {
    match filter {
        Filter::All => false,
        Filter::Exact(name) => has_reserved_suffix(name),
        Filter::Pattern(pattern) => pattern.split('/').next_back().is_some_and(has_reserved_suffix),
    }
}

fn drive_root(path: &Path) -> Option<PathBuf> {
    let mut root = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => root.push(component.as_os_str()),
            _ => break,
        }
    }
    if root.as_os_str().is_empty() { None } else { Some(root) }
}

fn relative(path: &Path, root: &Path, ci: bool) -> Option<Vec<String>> {
    let p = path_key(path, ci);
    let r = path_key(root, ci);
    let prefix = if r.ends_with('/') { r } else { format!("{r}/") };
    let rest = p.strip_prefix(&prefix)?;
    // Take the original spelling's tail with the same number of segments.
    let count = rest.split('/').filter(|s| !s.is_empty()).count();
    let original: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    Some(original[original.len().saturating_sub(count)..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Presence;

    struct Plain {
        ci: bool,
        links: Vec<(PathBuf, PathBuf)>,
    }

    impl RealPaths for Plain {
        fn real(&self, path: &Path) -> Result<PathBuf, String> {
            for (link, target) in &self.links {
                if let Ok(rest) = path.strip_prefix(link) {
                    return Ok(if rest.as_os_str().is_empty() { target.clone() } else { target.join(rest) });
                }
            }
            Ok(path.to_path_buf())
        }
        fn case_insensitive(&self) -> bool {
            self.ci
        }
    }

    fn t(root: &str, filter: Filter) -> Target {
        Target { root: root.into(), filter, excludes: vec![], presence: Presence::Present }
    }

    fn exact(root: &str, name: &str) -> Target {
        t(root, Filter::Exact(name.into()))
    }

    fn pat(root: &str, p: &str) -> Target {
        t(root, Filter::Pattern(p.into()))
    }

    fn broad() -> Vec<PathBuf> {
        ["C:/Users/u", "C:/Users/u/Documents", "C:/Users/u/AppData/Roaming", "C:/Program Files"]
            .iter()
            .map(PathBuf::from)
            .collect()
    }

    fn run(targets: &[Target], others: &[OtherGame<'_>]) -> Result<Vec<Target>, Failure> {
        let broad = broad();
        let exe = [PathBuf::from("D:/Game/Game.exe")];
        let input = SafetyInput {
            game_id: "g",
            targets,
            broad: &broad,
            install_dir: Some(Path::new("D:/Game")),
            executables: &exe,
            others,
        };
        check(&input, &Plain { ci: true, links: vec![] })
    }

    fn kind(result: Result<Vec<Target>, Failure>) -> Option<ErrorKind> {
        result.err().map(|f| f.kind)
    }

    #[test]
    fn exact_names_in_broad_folders_are_allowed() {
        assert!(run(&[exact("C:/Users/u/Documents", "mygame.sav")], &[]).is_ok());
        assert!(run(&[exact("D:/Game", "Save.ini")], &[]).is_ok());
        assert!(run(&[exact("C:/Users/u/AppData/Roaming", "Void_War")], &[]).is_ok());
        assert!(run(&[pat("D:/Game/saves", "*.sav")], &[]).is_ok());
    }

    #[test]
    fn broad_folders_whole_or_with_a_wildcard_are_rejected() {
        assert_eq!(kind(run(&[exact("C:/Users/u", "Documents")], &[])), Some(ErrorKind::InvalidTarget));
        assert_eq!(kind(run(&[pat("C:/Users/u/Documents", "*.sav")], &[])), Some(ErrorKind::InvalidTarget));
        assert_eq!(kind(run(&[pat("D:/Game", "save*")], &[])), Some(ErrorKind::InvalidTarget));
        assert_eq!(kind(run(&[exact("D:/", "Game")], &[])), Some(ErrorKind::InvalidTarget), "the install folder whole");
        assert_eq!(
            kind(run(&[t("C:/Users", Filter::All)], &[])),
            Some(ErrorKind::InvalidTarget),
            "contains a broad folder"
        );
        assert_eq!(kind(run(&[pat("E:/", "*")], &[])), Some(ErrorKind::InvalidTarget), "a drive root");
    }

    #[test]
    fn relative_paths_and_reserved_names_are_rejected() {
        assert_eq!(kind(run(&[exact("saves", "a")], &[])), Some(ErrorKind::InvalidConfig));
        assert_eq!(kind(run(&[pat("D:/Game/saves", "*.ssnew")], &[])), Some(ErrorKind::InvalidTarget));
        assert_eq!(kind(run(&[exact("D:/Game/saves", "save.ssold")], &[])), Some(ErrorKind::InvalidTarget));
        assert!(run(&[pat("D:/Game/saves", "save*")], &[]).is_ok(), "reserved names are excluded anyway");
    }

    #[test]
    fn a_filter_matching_the_executable_is_rejected() {
        assert_eq!(kind(run(&[exact("D:/Game", "Game.exe")], &[])), Some(ErrorKind::InvalidTarget));
    }

    #[test]
    fn overlaps_between_games_are_rejected_naming_the_other_game() {
        let theirs = [exact("C:/Users/u/Documents", "a.sav"), t("E:/Saves/Other", Filter::All)];
        let others = [OtherGame { id: "o", name: "Other", targets: &theirs }];
        assert!(
            run(&[exact("C:/Users/u/Documents", "b.sav")], &others).is_ok(),
            "distinct exact names may share a root"
        );
        let err = run(&[exact("C:/Users/u/Documents", "A.sav")], &others).unwrap_err();
        assert!(err.detail.contains("Other"));
        assert!(run(&[t("E:/Saves/Other/sub", Filter::All)], &others).is_err(), "sits inside");
        assert!(run(&[exact("E:/Saves", "Other")], &others).is_err(), "contains");
        assert!(run(&[exact("E:/Saves", "Other2")], &others).is_ok(), "Game and Game2 don't conflict");
    }

    #[test]
    fn links_resolve_to_their_target() {
        let paths = Plain { ci: true, links: vec![("C:/Link".into(), "C:/Users/u/Documents".into())] };
        let broad = broad();
        let targets = [pat("C:/Link", "*.sav")];
        let input = SafetyInput {
            game_id: "g",
            targets: &targets,
            broad: &broad,
            install_dir: None,
            executables: &[],
            others: &[],
        };
        assert!(check(&input, &paths).is_err(), "a junction to Documents is Documents");
        let targets = [exact("C:/Link", "g.sav")];
        let input = SafetyInput {
            game_id: "g",
            targets: &targets,
            broad: &broad,
            install_dir: None,
            executables: &[],
            others: &[],
        };
        let checked = check(&input, &paths).unwrap();
        assert_eq!(checked[0].root, PathBuf::from("C:/Users/u/Documents"), "operations run on the real folder");
    }

    #[test]
    fn case_rules() {
        let broad: Vec<PathBuf> = vec![];
        let (game, lower) =
            if cfg!(windows) { ("C:/home/u/Game", "C:/home/u/game") } else { ("/home/u/Game", "/home/u/game") };
        let theirs = [t(game, Filter::All)];
        let others = [OtherGame { id: "o", name: "Other", targets: &theirs }];
        let targets = [t(lower, Filter::All)];
        let input = SafetyInput {
            game_id: "g",
            targets: &targets,
            broad: &broad,
            install_dir: None,
            executables: &[],
            others: &others,
        };
        assert!(
            check(&input, &Plain { ci: false, links: vec![] }).is_ok(),
            "case-different folders on Linux are distinct"
        );
        assert!(check(&input, &Plain { ci: true, links: vec![] }).is_err());
    }
}
