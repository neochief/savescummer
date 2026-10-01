//! What a target matches on disk: its filter, minus its excludes and the
//! built-in excludes. Links and special files are never followed or copied;
//! meeting one fails the walk. So does nesting deeper than [`MAX_DEPTH`]: a
//! loop that isn't a link (a Linux bind mount of a parent inside its own
//! child) looks like ordinary folders, and the depth is what stops it.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use savescummer_catalog::glob::{could_match_below, match_path};
use savescummer_core::{ErrorKind, Failure, Filter, builtin_excluded};

/// How many folders below the root a walk goes before it fails. No game
/// nests saves anywhere near this deep.
pub const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Root-relative path segments.
    pub rel: Vec<String>,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl Entry {
    pub fn rel_text(&self) -> String {
        self.rel.join("/")
    }
}

/// Everything a target matches, files and folders, in a stable order.
/// A missing root matches nothing.
pub fn walk_target(root: &Path, filter: &Filter, excludes: &[String], ci: bool) -> Result<Vec<Entry>, Failure> {
    if crate::fsx::is_guarded(root) {
        return Err(crate::fsx::guarded_failure(root));
    }
    let mut out = Vec::new();
    match fs::symlink_metadata(root) {
        Err(_) => return Ok(out),
        Ok(meta) if !meta.is_dir() => {
            return Err(Failure::new(ErrorKind::InvalidTarget, "the save location's root isn't a folder")
                .path(root)
                .target_cause(savescummer_core::TargetCause::NotDirectory));
        }
        Ok(_) => {}
    }
    let walker = Walker { excludes, ci };
    match filter {
        Filter::All => walker.everything(root, &mut Vec::new(), &mut out)?,
        Filter::Exact(name) => {
            let path = root.join(name);
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Failure::new(ErrorKind::ReadFailed, e.to_string()).path(&path)),
                Ok(meta) => {
                    let mut rel = vec![name.clone()];
                    walker.visit(&path, &meta, &mut rel, &mut out, None)?;
                }
            }
        }
        Filter::Pattern(pattern) => walker.pattern(root, pattern, &mut Vec::new(), &mut out)?,
    }
    Ok(out)
}

struct Walker<'a> {
    excludes: &'a [String],
    ci: bool,
}

impl Walker<'_> {
    fn excluded(&self, rel: &[String], is_dir: bool) -> bool {
        let name = rel.last().map(String::as_str).unwrap_or("");
        if builtin_excluded(name, is_dir) {
            return true;
        }
        let text = rel.join("/");
        self.excludes.iter().any(|ex| match_path(ex, &text, self.ci))
    }

    /// Adds one entry that the filter matched, and everything inside it.
    fn visit(
        &self,
        path: &Path,
        meta: &fs::Metadata,
        rel: &mut Vec<String>,
        out: &mut Vec<Entry>,
        _pattern: Option<&str>,
    ) -> Result<(), Failure> {
        crate::fsx::advance();
        let file_type = meta.file_type();
        if self.excluded(rel, file_type.is_dir()) {
            return Ok(());
        }
        if file_type.is_symlink() {
            return Err(Failure::new(ErrorKind::LinkInTarget, "a link inside the save location").path(path));
        }
        if file_type.is_dir() {
            out.push(Entry { rel: rel.clone(), is_dir: true, size: 0, modified: meta.modified().ok() });
            self.everything(path, rel, out)
        } else if file_type.is_file() {
            out.push(Entry { rel: rel.clone(), is_dir: false, size: meta.len(), modified: meta.modified().ok() });
            Ok(())
        } else {
            Err(Failure::new(ErrorKind::LinkInTarget, "a special file inside the save location").path(path))
        }
    }

    /// The entries of `dir`, which sits `depth` folders below the root.
    fn children(&self, dir: &Path, depth: usize) -> Result<Vec<(String, std::path::PathBuf, fs::Metadata)>, Failure> {
        if depth >= MAX_DEPTH {
            return Err(Failure::new(
                ErrorKind::LinkInTarget,
                format!("folders nest more than {MAX_DEPTH} levels deep, likely a loop"),
            )
            .path(dir));
        }
        let read = fs::read_dir(dir).map_err(|e| read_failure(&e, dir))?;
        let mut children = Vec::new();
        for entry in read {
            let entry = entry.map_err(|e| read_failure(&e, dir))?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|e| read_failure(&e, &path))?;
            children.push((entry.file_name().to_string_lossy().into_owned(), path, meta));
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(children)
    }

    fn everything(&self, dir: &Path, rel: &mut Vec<String>, out: &mut Vec<Entry>) -> Result<(), Failure> {
        for (name, path, meta) in self.children(dir, rel.len())? {
            rel.push(name);
            self.visit(&path, &meta, rel, out, None)?;
            rel.pop();
        }
        Ok(())
    }

    fn pattern(&self, dir: &Path, pattern: &str, rel: &mut Vec<String>, out: &mut Vec<Entry>) -> Result<(), Failure> {
        for (name, path, meta) in self.children(dir, rel.len())? {
            rel.push(name);
            let text = rel.join("/");
            if match_path(pattern, &text, self.ci) {
                self.visit(&path, &meta, rel, out, Some(pattern))?;
            } else if meta.is_dir()
                && !meta.file_type().is_symlink()
                && could_match_below(pattern, &text, self.ci)
                && !self.excluded(rel, true)
            {
                self.pattern(&path, pattern, rel, out)?;
            }
            rel.pop();
        }
        Ok(())
    }
}

fn read_failure(e: &std::io::Error, path: &Path) -> Failure {
    let kind = if crate::fsx::is_unavailable(e) { ErrorKind::TargetUnavailable } else { ErrorKind::ReadFailed };
    Failure::new(kind, e.to_string()).path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let path = dir.path().join(f);
            if f.ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, f.as_bytes()).unwrap();
            }
        }
        dir
    }

    fn rels(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| format!("{}{}", e.rel_text(), if e.is_dir { "/" } else { "" })).collect()
    }

    #[test]
    fn everything_minus_builtin_excludes() {
        let dir = tree(&[
            "a.sav",
            "Player.log",
            "logs/x.txt",
            "Crashes/dump.dmp",
            "steam_autocloud.vdf",
            "b.ssold",
            "sub/c.sav",
        ]);
        let entries = walk_target(dir.path(), &Filter::All, &[], true).unwrap();
        assert_eq!(rels(&entries), vec!["a.sav", "sub/", "sub/c.sav"]);
    }

    #[test]
    fn exact_name_file_or_folder() {
        let dir = tree(&["data/save_data.xml", "data/asset.bin", "saves/1.sav"]);
        let file = walk_target(&dir.path().join("data"), &Filter::Exact("save_data.xml".into()), &[], true).unwrap();
        assert_eq!(rels(&file), vec!["save_data.xml"]);
        let folder = walk_target(dir.path(), &Filter::Exact("saves".into()), &[], true).unwrap();
        assert_eq!(rels(&folder), vec!["saves/", "saves/1.sav"]);
        let missing = walk_target(dir.path(), &Filter::Exact("nope".into()), &[], true).unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn patterns_and_excludes() {
        let dir = tree(&["user_0.dat", "user_1.dat", "dc_options.json", "C1/SGS1", "C1/other", "D1/SGS1"]);
        let p = walk_target(dir.path(), &Filter::Pattern("user_*.dat".into()), &[], true).unwrap();
        assert_eq!(rels(&p), vec!["user_0.dat", "user_1.dat"]);
        let deep = walk_target(dir.path(), &Filter::Pattern("C*/SGS*".into()), &[], true).unwrap();
        assert_eq!(rels(&deep), vec!["C1/SGS1"]);
        let ex = walk_target(dir.path(), &Filter::All, &["dc_options.json".into(), "C1/other".into()], true).unwrap();
        assert!(!rels(&ex).contains(&"dc_options.json".to_string()));
        assert!(!rels(&ex).contains(&"C1/other".to_string()));
    }

    #[test]
    fn a_missing_root_matches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(walk_target(&dir.path().join("x"), &Filter::All, &[], true).unwrap().is_empty());
    }

    #[test]
    fn double_star_spans_zero_one_or_many_folders() {
        let dir = tree(&[
            "U1/localhost/NEO.exe/s.sol",
            "U1/localhost/a/NEO.exe/s.sol",
            "U1/localhost/a/NEO.exe/other.sol",
            "U1/localhost/a/b/c/NEO.exe/s.sol",
            "U1/www.site/NEO.exe/s.sol",
            "U1/s.sol",
            "U2/localhost/x/NEO.exe/s.sol",
        ]);
        let found = walk_target(dir.path(), &Filter::Pattern("*/localhost/**/NEO.exe/s.sol".into()), &[], false);
        assert_eq!(
            rels(&found.unwrap()),
            vec![
                "U1/localhost/NEO.exe/s.sol",
                "U1/localhost/a/NEO.exe/s.sol",
                "U1/localhost/a/b/c/NEO.exe/s.sol",
                "U2/localhost/x/NEO.exe/s.sol",
            ]
        );
        let folded = Filter::Pattern("*/LOCALHOST/**/neo.EXE/S.sol".into());
        assert_eq!(walk_target(dir.path(), &folded, &[], true).unwrap().len(), 4);
        assert!(walk_target(dir.path(), &folded, &[], false).unwrap().is_empty());
    }

    // ---- Links. Windows makes junctions without admin rights, but symlinks
    // only with Developer Mode or admin: where the machine can't make a kind,
    // that kind is skipped, and every machine makes at least one.

    #[derive(Debug, Clone, Copy)]
    enum Link {
        Symlink,
        Junction,
    }

    const LINKS: [Link; 2] = [Link::Symlink, Link::Junction];

    /// Makes a folder link, or returns false where this machine can't make
    /// that kind.
    fn link_dir(kind: Link, target: &Path, link: &Path) -> bool {
        match kind {
            #[cfg(unix)]
            Link::Symlink => std::os::unix::fs::symlink(target, link).is_ok(),
            #[cfg(windows)]
            Link::Symlink => std::os::windows::fs::symlink_dir(target, link).is_ok(),
            #[cfg(windows)]
            Link::Junction => std::process::Command::new("cmd")
                .arg("/C")
                .arg("mklink")
                .arg("/J")
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|o| o.status.success()),
            #[cfg(unix)]
            Link::Junction => false,
        }
    }

    /// Runs `check` once per link kind this machine can make, on a fresh
    /// tree with a link at `link` pointing to `target` (both tree-relative).
    fn each_link(files: &[&str], target: &str, link: &str, check: impl Fn(Link, &Path)) {
        let mut made = 0;
        for kind in LINKS {
            let dir = tree(files);
            // Joined by segment: `mklink` doesn't take forward slashes.
            let at =
                |rel: &str| rel.split('/').filter(|s| !s.is_empty()).fold(dir.path().to_path_buf(), |p, s| p.join(s));
            if !link_dir(kind, &at(target), &at(link)) {
                continue;
            }
            made += 1;
            check(kind, dir.path());
        }
        assert!(made > 0, "this machine made no folder link at all");
    }

    fn kind_of(result: Result<Vec<Entry>, Failure>) -> ErrorKind {
        result.expect_err("the walk should fail").kind
    }

    #[test]
    fn a_link_loop_under_double_star_is_not_entered() {
        // A link back up to its own parent, and one back to the root.
        let files = ["a/x.sav", "a/b/y.sav"];
        each_link(&files, "a", "a/b/loop", |kind, root| {
            let found = walk_target(root, &Filter::Pattern("**/*.sav".into()), &[], true).unwrap();
            assert_eq!(rels(&found), vec!["a/b/y.sav", "a/x.sav"], "{kind:?}");
        });
        each_link(&files, "", "a/b/up", |kind, root| {
            let found = walk_target(root, &Filter::Pattern("**/*.sav".into()), &[], true).unwrap();
            assert_eq!(rels(&found), vec!["a/b/y.sav", "a/x.sav"], "{kind:?}");
        });
    }

    #[test]
    fn a_link_the_filter_matches_fails_the_walk() {
        each_link(&["real/s.sol"], "real", "saves", |kind, root| {
            let pattern = walk_target(root, &Filter::Pattern("sav*".into()), &[], true);
            assert_eq!(kind_of(pattern), ErrorKind::LinkInTarget, "{kind:?}");
            let exact = walk_target(root, &Filter::Exact("saves".into()), &[], true);
            assert_eq!(kind_of(exact), ErrorKind::LinkInTarget, "{kind:?}");
        });
    }

    #[test]
    fn a_link_inside_a_matched_folder_fails_the_walk() {
        each_link(&["saves/1.sav", "elsewhere/x.sav"], "elsewhere", "saves/linked", |kind, root| {
            let exact = walk_target(root, &Filter::Exact("saves".into()), &[], true);
            assert_eq!(kind_of(exact), ErrorKind::LinkInTarget, "{kind:?}");
            let all = walk_target(&root.join("saves"), &Filter::All, &[], true);
            assert_eq!(kind_of(all), ErrorKind::LinkInTarget, "{kind:?}");
            // A pattern that only searches past the link leaves it out.
            let search = walk_target(&root.join("saves"), &Filter::Pattern("**/*.sav".into()), &[], true);
            assert_eq!(rels(&search.unwrap()), vec!["1.sav"], "{kind:?}");
        });
    }

    #[test]
    fn broken_links_are_skipped_while_searching_and_fail_when_matched() {
        each_link(&["a/1.sav", "gone/"], "gone", "a/dangling", |kind, root| {
            fs::remove_dir(root.join("gone")).unwrap();
            let search = walk_target(root, &Filter::Pattern("**/*.sav".into()), &[], true);
            assert_eq!(rels(&search.unwrap()), vec!["a/1.sav"], "{kind:?}");
            let matched = walk_target(root, &Filter::Pattern("a/dang*".into()), &[], true);
            assert_eq!(kind_of(matched), ErrorKind::LinkInTarget, "{kind:?}");
            let all = walk_target(&root.join("a"), &Filter::All, &[], true);
            assert_eq!(kind_of(all), ErrorKind::LinkInTarget, "{kind:?}");
        });
    }

    #[test]
    fn a_linked_root_is_walked_through_its_real_path() {
        // Roots reach the walk resolved (the save set safety check). The walk
        // itself refuses a link as its root rather than following it.
        each_link(&["real/1.sav"], "real", "root", |kind, dir| {
            let link = dir.join("root");
            assert_eq!(kind_of(walk_target(&link, &Filter::All, &[], true)), ErrorKind::InvalidTarget, "{kind:?}");
            let real = crate::fsx::real_path(&link).unwrap();
            assert_eq!(real, crate::fsx::real_path(&dir.join("real")).unwrap(), "{kind:?}");
            assert_eq!(rels(&walk_target(&real, &Filter::All, &[], true).unwrap()), vec!["1.sav"], "{kind:?}");
            // A broken root link can't be resolved, so the target is invalid.
            fs::remove_dir_all(dir.join("real")).unwrap();
            assert!(crate::fsx::real_path(&link).is_err(), "{kind:?}");
        });
    }

    #[test]
    fn nesting_past_max_depth_fails_instead_of_running_on() {
        let nested = |levels: usize| format!("{}x.sav", "d/".repeat(levels));
        let deepest_allowed = tree(&[&nested(MAX_DEPTH - 1)]);
        for filter in [Filter::All, Filter::Pattern("**/x.sav".into())] {
            assert_eq!(
                walk_target(deepest_allowed.path(), &filter, &[], true).unwrap().last().unwrap().rel.len(),
                MAX_DEPTH
            );
        }
        let too_deep = tree(&[&nested(MAX_DEPTH)]);
        for filter in [Filter::All, Filter::Pattern("**/x.sav".into())] {
            assert_eq!(kind_of(walk_target(too_deep.path(), &filter, &[], true)), ErrorKind::LinkInTarget);
        }
    }
}
